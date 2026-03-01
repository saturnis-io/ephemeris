use ephemeris_core::domain::{Action, Epc, EpcisEvent, TransitionSource};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};
use ephemeris_core::service::SerialNumberService;

/// Handles incoming EPCIS events by routing them to the appropriate repositories.
///
/// Stores every event via the event repository, updates aggregation hierarchy
/// for aggregation events, and drives serial number state transitions based
/// on the event's bizStep.
pub struct EventHandler<E, A, S: SerialNumberRepository> {
    event_repo: E,
    agg_repo: A,
    sn_service: SerialNumberService<S>,
}

impl<E, A, S> EventHandler<E, A, S>
where
    E: EventRepository + 'static,
    A: AggregationRepository + 'static,
    S: SerialNumberRepository + 'static,
{
    pub fn new(event_repo: E, agg_repo: A, sn_service: SerialNumberService<S>) -> Self {
        Self {
            event_repo,
            agg_repo,
            sn_service,
        }
    }

    /// Handle an incoming EPCIS event.
    ///
    /// 1. Stores the event
    /// 2. Routes aggregation events to the hierarchy repo
    /// 3. Drives SN state transitions based on bizStep
    pub async fn handle_event(&self, event: &EpcisEvent) -> Result<(), RepoError> {
        let stored_id = self.event_repo.store_event(event).await?;

        // Route aggregation events
        if let EpcisEvent::AggregationEvent(data) = event
            && let Some(ref parent_id_str) = data.parent_id
        {
            let parent = Epc::new(parent_id_str);

            match data.action {
                Action::Add | Action::Observe => {
                    for child_epc_str in &data.child_epcs {
                        let child = Epc::new(child_epc_str);
                        self.agg_repo.add_child(&parent, &child, &stored_id).await?;
                    }
                }
                Action::Delete => {
                    for child_epc_str in &data.child_epcs {
                        let child = Epc::new(child_epc_str);
                        self.agg_repo.remove_child(&parent, &child).await?;
                    }
                }
            }
        }

        // Drive SN state transitions from bizStep
        let biz_step = match event {
            EpcisEvent::ObjectEvent(data) => data.common.biz_step.as_deref(),
            EpcisEvent::AggregationEvent(data) => data.common.biz_step.as_deref(),
            EpcisEvent::TransformationEvent(data) => data.common.biz_step.as_deref(),
        };

        if let Some(biz_step) = biz_step {
            let epcs = Self::extract_epcs(event);
            for epc in epcs {
                if let Err(e) = self
                    .sn_service
                    .process_transition(&epc, biz_step, Some(&stored_id), TransitionSource::Mqtt)
                    .await
                {
                    tracing::warn!(epc = %epc, error = %e, "failed to update SN state");
                }
            }
        }

        Ok(())
    }

    /// Extract all EPCs from an event for SN state tracking.
    fn extract_epcs(event: &EpcisEvent) -> Vec<Epc> {
        match event {
            EpcisEvent::ObjectEvent(data) => data.epc_list.iter().map(Epc::new).collect(),
            EpcisEvent::AggregationEvent(data) => {
                let mut epcs: Vec<Epc> = data.child_epcs.iter().map(Epc::new).collect();
                if let Some(ref parent) = data.parent_id {
                    epcs.push(Epc::new(parent));
                }
                epcs
            }
            EpcisEvent::TransformationEvent(data) => {
                let mut epcs: Vec<Epc> = data.input_epc_list.iter().map(Epc::new).collect();
                epcs.extend(data.output_epc_list.iter().map(Epc::new));
                epcs
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephemeris_core::domain::{
        AggregationEventData, AggregationTree, CommonEventFields, EventId, ObjectEventData,
        SerialNumber, SerialNumberQuery, SnState, SnTransition,
    };
    use mockall::mock;
    use std::sync::Mutex;

    mock! {
        pub EventRepo {}

        impl EventRepository for EventRepo {
            async fn store_event(&self, event: &EpcisEvent) -> Result<EventId, RepoError>;
            async fn get_event(&self, id: &EventId) -> Result<Option<EpcisEvent>, RepoError>;
            async fn query_events(
                &self,
                query: &ephemeris_core::domain::EventQuery,
            ) -> Result<Vec<EpcisEvent>, RepoError>;
        }
    }

    mock! {
        pub AggRepo {}

        impl AggregationRepository for AggRepo {
            async fn add_child(
                &self,
                parent: &Epc,
                child: &Epc,
                event_id: &EventId,
            ) -> Result<(), RepoError>;

            async fn remove_child(&self, parent: &Epc, child: &Epc) -> Result<(), RepoError>;

            async fn get_children(&self, parent: &Epc) -> Result<Vec<Epc>, RepoError>;

            async fn get_ancestors(&self, child: &Epc) -> Result<Vec<Epc>, RepoError>;

            async fn get_full_hierarchy(
                &self,
                root: &Epc,
            ) -> Result<AggregationTree, RepoError>;
        }
    }

    /// In-memory stub for SerialNumberRepository (avoids mockall lifetime issues with Option<&str>).
    struct StubSnRepo {
        transitions: Mutex<Vec<SnTransition>>,
    }

    impl StubSnRepo {
        fn new() -> Self {
            Self {
                transitions: Mutex::new(Vec::new()),
            }
        }
    }

    impl SerialNumberRepository for StubSnRepo {
        async fn upsert_state(
            &self,
            _epc: &Epc,
            _state: SnState,
            _sid_class: Option<&str>,
            _pool_id: Option<&str>,
        ) -> Result<(), RepoError> {
            Ok(())
        }

        async fn get_state(&self, _epc: &Epc) -> Result<Option<SerialNumber>, RepoError> {
            Ok(None)
        }

        async fn query(&self, _query: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError> {
            Ok(vec![])
        }

        async fn record_transition(&self, transition: &SnTransition) -> Result<(), RepoError> {
            self.transitions.lock().unwrap().push(transition.clone());
            Ok(())
        }

        async fn get_history(
            &self,
            _epc: &Epc,
            _limit: u32,
        ) -> Result<Vec<SnTransition>, RepoError> {
            Ok(self.transitions.lock().unwrap().clone())
        }
    }

    fn make_common() -> CommonEventFields {
        use chrono::FixedOffset;
        CommonEventFields {
            event_id: Some("test-event-1".to_string()),
            event_time: chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00")
                .unwrap()
                .with_timezone(&FixedOffset::east_opt(0).unwrap()),
            event_time_zone_offset: "+00:00".to_string(),
            record_time: None,
            biz_step: None,
            disposition: None,
            read_point: None,
            biz_location: None,
            biz_transaction_list: vec![],
            source_list: vec![],
            destination_list: vec![],
        }
    }

    #[tokio::test]
    async fn test_handle_object_event_no_bizstep() {
        let mut mock_event = MockEventRepo::new();
        let mock_agg = MockAggRepo::new();

        mock_event
            .expect_store_event()
            .times(1)
            .returning(|_| Ok(EventId::new()));

        let sn_service = SerialNumberService::new(StubSnRepo::new());
        let handler = EventHandler::new(mock_event, mock_agg, sn_service);
        let event = EpcisEvent::ObjectEvent(ObjectEventData {
            common: make_common(),
            action: Action::Observe,
            epc_list: vec!["urn:epc:id:sgtin:0614141.107346.2017".to_string()],
            quantity_list: vec![],
        });

        assert!(handler.handle_event(&event).await.is_ok());
    }

    #[tokio::test]
    async fn test_handle_object_event_with_commissioning() {
        let mut mock_event = MockEventRepo::new();
        let mock_agg = MockAggRepo::new();

        mock_event
            .expect_store_event()
            .times(1)
            .returning(|_| Ok(EventId::new()));

        let sn_service = SerialNumberService::new(StubSnRepo::new());
        let handler = EventHandler::new(mock_event, mock_agg, sn_service);

        let mut common = make_common();
        common.biz_step = Some("commissioning".to_string());

        let event = EpcisEvent::ObjectEvent(ObjectEventData {
            common,
            action: Action::Observe,
            epc_list: vec!["urn:epc:id:sgtin:0614141.107346.2017".to_string()],
            quantity_list: vec![],
        });

        assert!(handler.handle_event(&event).await.is_ok());
    }

    #[tokio::test]
    async fn test_handle_aggregation_add() {
        let mut mock_event = MockEventRepo::new();
        let mut mock_agg = MockAggRepo::new();

        mock_event
            .expect_store_event()
            .times(1)
            .returning(|_| Ok(EventId::new()));

        mock_agg
            .expect_add_child()
            .times(2)
            .returning(|_, _, _| Ok(()));

        let sn_service = SerialNumberService::new(StubSnRepo::new());
        let handler = EventHandler::new(mock_event, mock_agg, sn_service);

        let mut common = make_common();
        common.biz_step = Some("packing".to_string());

        let event = EpcisEvent::AggregationEvent(AggregationEventData {
            common,
            action: Action::Add,
            parent_id: Some("urn:epc:id:sscc:0614141.1234567890".to_string()),
            child_epcs: vec![
                "urn:epc:id:sgtin:0614141.107346.2017".to_string(),
                "urn:epc:id:sgtin:0614141.107346.2018".to_string(),
            ],
            child_quantity_list: vec![],
        });

        assert!(handler.handle_event(&event).await.is_ok());
    }

    #[tokio::test]
    async fn test_handle_aggregation_delete() {
        let mut mock_event = MockEventRepo::new();
        let mut mock_agg = MockAggRepo::new();

        mock_event
            .expect_store_event()
            .times(1)
            .returning(|_| Ok(EventId::new()));

        mock_agg
            .expect_remove_child()
            .times(2)
            .returning(|_, _| Ok(()));

        let sn_service = SerialNumberService::new(StubSnRepo::new());
        let handler = EventHandler::new(mock_event, mock_agg, sn_service);

        let event = EpcisEvent::AggregationEvent(AggregationEventData {
            common: make_common(),
            action: Action::Delete,
            parent_id: Some("urn:epc:id:sscc:0614141.1234567890".to_string()),
            child_epcs: vec![
                "urn:epc:id:sgtin:0614141.107346.2017".to_string(),
                "urn:epc:id:sgtin:0614141.107346.2018".to_string(),
            ],
            child_quantity_list: vec![],
        });

        assert!(handler.handle_event(&event).await.is_ok());
    }
}
