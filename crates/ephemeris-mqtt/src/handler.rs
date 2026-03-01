use ephemeris_core::domain::{Action, Epc, EpcisEvent};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::{AggregationRepository, EventRepository};

/// Handles incoming EPCIS events by routing them to the appropriate repositories.
///
/// Stores every event via the event repository, and for aggregation events
/// with a parent_id, updates the aggregation hierarchy accordingly.
pub struct EventHandler<E, A> {
    event_repo: E,
    agg_repo: A,
}

impl<E, A> EventHandler<E, A>
where
    E: EventRepository + 'static,
    A: AggregationRepository + 'static,
{
    pub fn new(event_repo: E, agg_repo: A) -> Self {
        Self {
            event_repo,
            agg_repo,
        }
    }

    /// Handle an incoming EPCIS event.
    ///
    /// Stores the event and, for aggregation events with a parent_id,
    /// updates child relationships based on the action type.
    pub async fn handle_event(&self, event: &EpcisEvent) -> Result<(), RepoError> {
        let stored_id = self.event_repo.store_event(event).await?;

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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephemeris_core::domain::{
        AggregationEventData, AggregationTree, CommonEventFields, EventId, ObjectEventData,
    };
    use mockall::mock;

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
    async fn test_handle_object_event() {
        let mut mock_event = MockEventRepo::new();
        let mock_agg = MockAggRepo::new();

        mock_event
            .expect_store_event()
            .times(1)
            .returning(|_| Ok(EventId::new()));

        // No aggregation calls expected for object events

        let handler = EventHandler::new(mock_event, mock_agg);
        let event = EpcisEvent::ObjectEvent(ObjectEventData {
            common: make_common(),
            action: Action::Observe,
            epc_list: vec!["urn:epc:id:sgtin:0614141.107346.2017".to_string()],
            quantity_list: vec![],
        });

        let result = handler.handle_event(&event).await;
        assert!(result.is_ok());
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

        let handler = EventHandler::new(mock_event, mock_agg);
        let event = EpcisEvent::AggregationEvent(AggregationEventData {
            common: make_common(),
            action: Action::Add,
            parent_id: Some("urn:epc:id:sscc:0614141.1234567890".to_string()),
            child_epcs: vec![
                "urn:epc:id:sgtin:0614141.107346.2017".to_string(),
                "urn:epc:id:sgtin:0614141.107346.2018".to_string(),
            ],
            child_quantity_list: vec![],
        });

        let result = handler.handle_event(&event).await;
        assert!(result.is_ok());
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

        let handler = EventHandler::new(mock_event, mock_agg);
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

        let result = handler.handle_event(&event).await;
        assert!(result.is_ok());
    }
}
