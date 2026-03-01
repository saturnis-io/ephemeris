use crate::domain::{
    Epc, EventId, SerialNumber, SerialNumberQuery, SnState, SnTransition, TransitionSource,
    biz_step_to_target_state, is_valid_transition,
};
use crate::error::RepoError;
use crate::repository::SerialNumberRepository;

/// Service layer for serial number lifecycle management.
///
/// Contains business logic: state machine transitions, validation (permissive),
/// and audit logging. Delegates storage to the underlying repository.
pub struct SerialNumberService<S: SerialNumberRepository> {
    repo: S,
}

impl<S: SerialNumberRepository> SerialNumberService<S> {
    pub fn new(repo: S) -> Self {
        Self { repo }
    }

    /// Process a state transition triggered by a bizStep.
    ///
    /// Returns the new state if the bizStep maps to a state change,
    /// or None if the bizStep doesn't affect SN state (e.g., packing).
    /// Permissive: warns on invalid transitions but applies them anyway.
    pub async fn process_transition(
        &self,
        epc: &Epc,
        biz_step: &str,
        event_id: Option<&EventId>,
        source: TransitionSource,
    ) -> Result<Option<SnState>, RepoError> {
        let target = match biz_step_to_target_state(biz_step) {
            Some(t) => t,
            None => return Ok(None),
        };

        let current = self
            .repo
            .get_state(epc)
            .await?
            .map(|sn| sn.state)
            .unwrap_or(SnState::Unassigned);

        if !is_valid_transition(current, target) {
            tracing::warn!(
                epc = %epc,
                from = %current,
                to = %target,
                biz_step = %biz_step,
                "invalid SN state transition (permissive — applying anyway)"
            );
        }

        self.repo.upsert_state(epc, target, None, None).await?;

        let transition = SnTransition {
            epc: epc.clone(),
            from_state: current,
            to_state: target,
            biz_step: biz_step.to_string(),
            event_id: event_id.cloned(),
            source,
            timestamp: chrono::Utc::now().fixed_offset(),
        };
        self.repo.record_transition(&transition).await?;

        Ok(Some(target))
    }

    /// Manual state override for operator corrections.
    pub async fn manual_override(
        &self,
        epc: &Epc,
        target_state: SnState,
        reason: &str,
    ) -> Result<SnState, RepoError> {
        let current = self
            .repo
            .get_state(epc)
            .await?
            .map(|sn| sn.state)
            .unwrap_or(SnState::Unassigned);

        self.repo
            .upsert_state(epc, target_state, None, None)
            .await?;

        let transition = SnTransition {
            epc: epc.clone(),
            from_state: current,
            to_state: target_state,
            biz_step: format!("manual_override:{reason}"),
            event_id: None,
            source: TransitionSource::RestApi,
            timestamp: chrono::Utc::now().fixed_offset(),
        };
        self.repo.record_transition(&transition).await?;

        Ok(target_state)
    }

    /// Get current state of a serial number.
    pub async fn get_state(&self, epc: &Epc) -> Result<Option<SerialNumber>, RepoError> {
        self.repo.get_state(epc).await
    }

    /// Get transition history.
    pub async fn get_history(&self, epc: &Epc, limit: u32) -> Result<Vec<SnTransition>, RepoError> {
        self.repo.get_history(epc, limit).await
    }

    /// Query serial numbers with filters.
    pub async fn query(&self, query: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError> {
        self.repo.query(query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Epc;
    use std::sync::Mutex;

    /// In-memory test double for SerialNumberRepository.
    /// Avoids mockall lifetime issues with `Option<&str>` params.
    struct StubSnRepo {
        state: Mutex<Option<SerialNumber>>,
        transitions: Mutex<Vec<SnTransition>>,
    }

    impl StubSnRepo {
        fn empty() -> Self {
            Self {
                state: Mutex::new(None),
                transitions: Mutex::new(Vec::new()),
            }
        }

        fn with_state(sn: SerialNumber) -> Self {
            Self {
                state: Mutex::new(Some(sn)),
                transitions: Mutex::new(Vec::new()),
            }
        }
    }

    impl SerialNumberRepository for StubSnRepo {
        async fn upsert_state(
            &self,
            epc: &Epc,
            state: SnState,
            _sid_class: Option<&str>,
            _pool_id: Option<&str>,
        ) -> Result<(), RepoError> {
            let mut current = self.state.lock().unwrap();
            let now = chrono::Utc::now().fixed_offset();
            *current = Some(SerialNumber {
                epc: epc.clone(),
                state,
                sid_class: None,
                pool_id: None,
                updated_at: now,
                created_at: current.as_ref().map(|s| s.created_at).unwrap_or(now),
            });
            Ok(())
        }

        async fn get_state(&self, _epc: &Epc) -> Result<Option<SerialNumber>, RepoError> {
            Ok(self.state.lock().unwrap().clone())
        }

        async fn query(&self, _query: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError> {
            Ok(self.state.lock().unwrap().iter().cloned().collect())
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

    fn make_sn(state: SnState) -> SerialNumber {
        SerialNumber {
            epc: Epc::new("urn:epc:id:sgtin:0614141.107346.2017"),
            state,
            sid_class: None,
            pool_id: None,
            updated_at: chrono::Utc::now().fixed_offset(),
            created_at: chrono::Utc::now().fixed_offset(),
        }
    }

    #[tokio::test]
    async fn test_process_transition_valid() {
        let repo = StubSnRepo::with_state(make_sn(SnState::Encoded));

        let service = SerialNumberService::new(repo);
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        let result = service
            .process_transition(&epc, "commissioning", None, TransitionSource::Mqtt)
            .await
            .unwrap();

        assert_eq!(result, Some(SnState::Commissioned));
    }

    #[tokio::test]
    async fn test_process_transition_invalid_still_applies() {
        // No prior state — defaults to Unassigned, jumping to Commissioned is invalid
        let repo = StubSnRepo::empty();

        let service = SerialNumberService::new(repo);
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        let result = service
            .process_transition(&epc, "commissioning", None, TransitionSource::Mqtt)
            .await
            .unwrap();

        // Still applied despite being invalid (permissive)
        assert_eq!(result, Some(SnState::Commissioned));
    }

    #[tokio::test]
    async fn test_process_transition_no_state_change() {
        let repo = StubSnRepo::empty();
        // packing doesn't change SN state — no repo calls beyond the early return

        let service = SerialNumberService::new(repo);
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        let result = service
            .process_transition(&epc, "packing", None, TransitionSource::Mqtt)
            .await
            .unwrap();

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_manual_override() {
        let repo = StubSnRepo::with_state(make_sn(SnState::Commissioned));

        let service = SerialNumberService::new(repo);
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        let result = service
            .manual_override(&epc, SnState::Destroyed, "line scanner missed event")
            .await
            .unwrap();

        assert_eq!(result, SnState::Destroyed);
    }
}
