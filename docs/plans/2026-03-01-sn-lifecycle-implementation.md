# Serial Number Lifecycle — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add OPEN-SCS-aligned serial number state tracking to Ephemeris with a service layer, PostgreSQL backend, MQTT integration, and REST API.

**Architecture:** `SerialNumberService<S>` in `ephemeris-core` holds state machine business logic. Thin `SerialNumberRepository` trait with `PgSerialNumberRepository` implementation. Handler gains third generic `S`. `AppState` gains `sn_service` field. Four new API routes.

**Tech Stack:** Rust 2024 edition, tokio-postgres/deadpool-postgres, axum 0.8, rumqttc 0.25, serde, chrono, tracing, mockall, testcontainers

---

## Task 1: Domain Types — SnState Enum

**Files:**
- Create: `crates/ephemeris-core/src/domain/serial_number.rs`
- Modify: `crates/ephemeris-core/src/domain/mod.rs`

**Step 1: Write the failing tests**

Add to the bottom of `crates/ephemeris-core/src/domain/serial_number.rs`:

```rust
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The 12 serial number states from OPEN-SCS PSS §5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnState {
    Unassigned,
    Unallocated,
    Allocated,
    SnInvalid,
    Encoded,
    LabelSampled,
    LabelScrapped,
    Commissioned,
    Sampled,
    Inactive,
    Destroyed,
    Released,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sn_state_display_roundtrip() {
        let states = [
            (SnState::Unassigned, "unassigned"),
            (SnState::Unallocated, "unallocated"),
            (SnState::Allocated, "allocated"),
            (SnState::SnInvalid, "sn_invalid"),
            (SnState::Encoded, "encoded"),
            (SnState::LabelSampled, "label_sampled"),
            (SnState::LabelScrapped, "label_scrapped"),
            (SnState::Commissioned, "commissioned"),
            (SnState::Sampled, "sampled"),
            (SnState::Inactive, "inactive"),
            (SnState::Destroyed, "destroyed"),
            (SnState::Released, "released"),
        ];

        for (state, expected_str) in &states {
            assert_eq!(state.to_string(), *expected_str);
            assert_eq!(SnState::from_str(expected_str).unwrap(), *state);
        }
    }

    #[test]
    fn test_sn_state_from_str_invalid() {
        assert!(SnState::from_str("bogus").is_err());
        assert!(SnState::from_str("").is_err());
    }

    #[test]
    fn test_sn_state_serde_roundtrip() {
        let state = SnState::Commissioned;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"commissioned\"");
        let back: SnState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ephemeris-core --lib domain::serial_number`
Expected: FAIL — `Display` and `FromStr` not implemented yet.

**Step 3: Implement Display and FromStr**

Add above the `#[cfg(test)]` block in `serial_number.rs`:

```rust
impl fmt::Display for SnState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Unassigned => "unassigned",
            Self::Unallocated => "unallocated",
            Self::Allocated => "allocated",
            Self::SnInvalid => "sn_invalid",
            Self::Encoded => "encoded",
            Self::LabelSampled => "label_sampled",
            Self::LabelScrapped => "label_scrapped",
            Self::Commissioned => "commissioned",
            Self::Sampled => "sampled",
            Self::Inactive => "inactive",
            Self::Destroyed => "destroyed",
            Self::Released => "released",
        };
        write!(f, "{s}")
    }
}

impl FromStr for SnState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unassigned" => Ok(Self::Unassigned),
            "unallocated" => Ok(Self::Unallocated),
            "allocated" => Ok(Self::Allocated),
            "sn_invalid" => Ok(Self::SnInvalid),
            "encoded" => Ok(Self::Encoded),
            "label_sampled" => Ok(Self::LabelSampled),
            "label_scrapped" => Ok(Self::LabelScrapped),
            "commissioned" => Ok(Self::Commissioned),
            "sampled" => Ok(Self::Sampled),
            "inactive" => Ok(Self::Inactive),
            "destroyed" => Ok(Self::Destroyed),
            "released" => Ok(Self::Released),
            other => Err(format!("unknown SnState: {other}")),
        }
    }
}
```

Also register the module in `crates/ephemeris-core/src/domain/mod.rs`. Add:
```rust
pub mod serial_number;
pub use serial_number::*;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p ephemeris-core --lib domain::serial_number`
Expected: 3 tests PASS

**Step 5: Commit**

```bash
git add crates/ephemeris-core/src/domain/serial_number.rs crates/ephemeris-core/src/domain/mod.rs
git commit -m "feat(core): add SnState enum with 12 OPEN-SCS states"
```

---

## Task 2: Domain Types — BizStep Mapping & Transition Validation

**Files:**
- Modify: `crates/ephemeris-core/src/domain/serial_number.rs`

**Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `serial_number.rs`:

```rust
    #[test]
    fn test_biz_step_to_target_state_shorthand() {
        assert_eq!(biz_step_to_target_state("provisioning"), Some(SnState::Unallocated));
        assert_eq!(biz_step_to_target_state("sn_returning"), Some(SnState::Unassigned));
        assert_eq!(biz_step_to_target_state("sn_allocating"), Some(SnState::Allocated));
        assert_eq!(biz_step_to_target_state("sn_deallocating"), Some(SnState::Unallocated));
        assert_eq!(biz_step_to_target_state("sn_invalidating"), Some(SnState::SnInvalid));
        assert_eq!(biz_step_to_target_state("sn_encoding"), Some(SnState::Encoded));
        assert_eq!(biz_step_to_target_state("label_sampling"), Some(SnState::LabelSampled));
        assert_eq!(biz_step_to_target_state("label_scrapping"), Some(SnState::LabelScrapped));
        assert_eq!(biz_step_to_target_state("commissioning"), Some(SnState::Commissioned));
        assert_eq!(biz_step_to_target_state("inspecting"), Some(SnState::Sampled));
        assert_eq!(biz_step_to_target_state("shipping"), Some(SnState::Released));
        assert_eq!(biz_step_to_target_state("decommissioning"), Some(SnState::Inactive));
        assert_eq!(biz_step_to_target_state("destroying"), Some(SnState::Destroyed));
    }

    #[test]
    fn test_biz_step_no_state_change() {
        assert_eq!(biz_step_to_target_state("packing"), None);
        assert_eq!(biz_step_to_target_state("unpacking"), None);
        assert_eq!(biz_step_to_target_state("label_inspecting"), None);
        assert_eq!(biz_step_to_target_state("unknown_step"), None);
    }

    #[test]
    fn test_biz_step_with_uri_prefix() {
        assert_eq!(
            biz_step_to_target_state("urn:epcglobal:cbv:bizstep:commissioning"),
            Some(SnState::Commissioned)
        );
        assert_eq!(
            biz_step_to_target_state("urn:epcglobal:cbv:bizstep:shipping"),
            Some(SnState::Released)
        );
        assert_eq!(
            biz_step_to_target_state("http://open-scs.org/bizstep/sn_encoding"),
            Some(SnState::Encoded)
        );
    }

    #[test]
    fn test_valid_transitions() {
        // Unassigned -> Unallocated
        assert!(is_valid_transition(SnState::Unassigned, SnState::Unallocated));
        assert!(!is_valid_transition(SnState::Unassigned, SnState::Commissioned));

        // Unallocated -> Unassigned, Allocated, SnInvalid
        assert!(is_valid_transition(SnState::Unallocated, SnState::Unassigned));
        assert!(is_valid_transition(SnState::Unallocated, SnState::Allocated));
        assert!(is_valid_transition(SnState::Unallocated, SnState::SnInvalid));
        assert!(!is_valid_transition(SnState::Unallocated, SnState::Commissioned));

        // Allocated -> Unallocated, Encoded, SnInvalid
        assert!(is_valid_transition(SnState::Allocated, SnState::Unallocated));
        assert!(is_valid_transition(SnState::Allocated, SnState::Encoded));
        assert!(is_valid_transition(SnState::Allocated, SnState::SnInvalid));
        assert!(!is_valid_transition(SnState::Allocated, SnState::Released));

        // Encoded -> LabelSampled, LabelScrapped, Commissioned
        assert!(is_valid_transition(SnState::Encoded, SnState::LabelSampled));
        assert!(is_valid_transition(SnState::Encoded, SnState::LabelScrapped));
        assert!(is_valid_transition(SnState::Encoded, SnState::Commissioned));
        assert!(!is_valid_transition(SnState::Encoded, SnState::Released));

        // Commissioned -> Sampled, Inactive, Destroyed, Released
        assert!(is_valid_transition(SnState::Commissioned, SnState::Sampled));
        assert!(is_valid_transition(SnState::Commissioned, SnState::Inactive));
        assert!(is_valid_transition(SnState::Commissioned, SnState::Destroyed));
        assert!(is_valid_transition(SnState::Commissioned, SnState::Released));
        assert!(!is_valid_transition(SnState::Commissioned, SnState::Encoded));

        // Terminal states have no valid outbound transitions
        assert!(!is_valid_transition(SnState::Destroyed, SnState::Commissioned));
        assert!(!is_valid_transition(SnState::Released, SnState::Commissioned));
        assert!(!is_valid_transition(SnState::SnInvalid, SnState::Unassigned));
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ephemeris-core --lib domain::serial_number`
Expected: FAIL — `biz_step_to_target_state` and `is_valid_transition` not defined.

**Step 3: Implement the functions**

Add above the `impl fmt::Display` block in `serial_number.rs`:

```rust
/// Map a bizStep string to the target SN state.
/// Returns None for events that don't change SN state (packing, unpacking, label_inspecting).
/// Accepts both shorthand ("commissioning") and URI ("urn:epcglobal:cbv:bizstep:commissioning").
pub fn biz_step_to_target_state(biz_step: &str) -> Option<SnState> {
    // Strip known URI prefixes to get the shorthand
    let shorthand = biz_step
        .strip_prefix("urn:epcglobal:cbv:bizstep:")
        .or_else(|| biz_step.strip_prefix("http://open-scs.org/bizstep/"))
        .unwrap_or(biz_step);

    match shorthand {
        "provisioning" => Some(SnState::Unallocated),
        "sn_returning" => Some(SnState::Unassigned),
        "sn_allocating" => Some(SnState::Allocated),
        "sn_deallocating" => Some(SnState::Unallocated),
        "sn_invalidating" => Some(SnState::SnInvalid),
        "sn_encoding" => Some(SnState::Encoded),
        "label_sampling" => Some(SnState::LabelSampled),
        "label_scrapping" => Some(SnState::LabelScrapped),
        "commissioning" => Some(SnState::Commissioned),
        "inspecting" => Some(SnState::Sampled),
        "shipping" => Some(SnState::Released),
        "decommissioning" => Some(SnState::Inactive),
        "destroying" => Some(SnState::Destroyed),
        // These don't change SN state
        "packing" | "unpacking" | "label_inspecting" => None,
        _ => None,
    }
}

/// Check if a state transition is valid per OPEN-SCS PSS §5 Figure 4.
/// Used for permissive warnings, not enforcement.
pub fn is_valid_transition(from: SnState, to: SnState) -> bool {
    matches!(
        (from, to),
        // Unassigned -> Unallocated
        (SnState::Unassigned, SnState::Unallocated)
        // Unallocated -> Unassigned, Allocated, SnInvalid
        | (SnState::Unallocated, SnState::Unassigned)
        | (SnState::Unallocated, SnState::Allocated)
        | (SnState::Unallocated, SnState::SnInvalid)
        // Allocated -> Unallocated, Encoded, SnInvalid
        | (SnState::Allocated, SnState::Unallocated)
        | (SnState::Allocated, SnState::Encoded)
        | (SnState::Allocated, SnState::SnInvalid)
        // Encoded -> LabelSampled, LabelScrapped, Commissioned
        | (SnState::Encoded, SnState::LabelSampled)
        | (SnState::Encoded, SnState::LabelScrapped)
        | (SnState::Encoded, SnState::Commissioned)
        // Commissioned -> Sampled, Inactive, Destroyed, Released
        | (SnState::Commissioned, SnState::Sampled)
        | (SnState::Commissioned, SnState::Inactive)
        | (SnState::Commissioned, SnState::Destroyed)
        | (SnState::Commissioned, SnState::Released)
    )
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p ephemeris-core --lib domain::serial_number`
Expected: 7 tests PASS

**Step 5: Commit**

```bash
git add crates/ephemeris-core/src/domain/serial_number.rs
git commit -m "feat(core): add bizStep mapping and transition validation"
```

---

## Task 3: Domain Types — SerialNumber, SnTransition, TransitionSource, SerialNumberQuery

**Files:**
- Modify: `crates/ephemeris-core/src/domain/serial_number.rs`

**Step 1: Add the data types**

Add after the `is_valid_transition` function, before `impl fmt::Display`:

```rust
use chrono::{DateTime, FixedOffset};
use super::epc::Epc;
use super::event::EventId;

/// A tracked serial number with its current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialNumber {
    pub epc: Epc,
    pub state: SnState,
    pub sid_class: Option<String>,
    pub pool_id: Option<String>,
    pub updated_at: DateTime<FixedOffset>,
    pub created_at: DateTime<FixedOffset>,
}

/// Source of a state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionSource {
    Mqtt,
    RestApi,
    System,
}

/// Audit record of a single state transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnTransition {
    pub epc: Epc,
    pub from_state: SnState,
    pub to_state: SnState,
    pub biz_step: String,
    pub event_id: Option<EventId>,
    pub source: TransitionSource,
    pub timestamp: DateTime<FixedOffset>,
}

/// Query parameters for serial number searches.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SerialNumberQuery {
    pub state: Option<SnState>,
    pub sid_class: Option<String>,
    pub pool_id: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}
```

Move the `use` imports to the top of the file (consolidate with existing ones).

**Step 2: Add a quick serde roundtrip test**

Add to the test module:

```rust
    #[test]
    fn test_serial_number_serde() {
        let sn = SerialNumber {
            epc: Epc::new("urn:epc:id:sgtin:0614141.107346.2017"),
            state: SnState::Commissioned,
            sid_class: Some("sgtin".to_string()),
            pool_id: None,
            updated_at: chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00").unwrap(),
            created_at: chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00").unwrap(),
        };
        let json = serde_json::to_string(&sn).unwrap();
        let back: SerialNumber = serde_json::from_str(&json).unwrap();
        assert_eq!(back.state, SnState::Commissioned);
        assert_eq!(back.epc, sn.epc);
    }

    #[test]
    fn test_transition_source_serde() {
        let json = serde_json::to_string(&TransitionSource::Mqtt).unwrap();
        assert_eq!(json, "\"mqtt\"");
        let back: TransitionSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TransitionSource::Mqtt);
    }
```

**Step 3: Run tests**

Run: `cargo test -p ephemeris-core --lib domain::serial_number`
Expected: 9 tests PASS

**Step 4: Commit**

```bash
git add crates/ephemeris-core/src/domain/serial_number.rs
git commit -m "feat(core): add SerialNumber, SnTransition, and query types"
```

---

## Task 4: Repository Trait — SerialNumberRepository

**Files:**
- Create: `crates/ephemeris-core/src/repository/serial_number.rs`
- Modify: `crates/ephemeris-core/src/repository/mod.rs`

**Step 1: Create the trait**

Create `crates/ephemeris-core/src/repository/serial_number.rs`:

```rust
use crate::domain::{Epc, SerialNumber, SerialNumberQuery, SnState, SnTransition};
use crate::error::RepoError;

/// Repository for serial number state tracking and audit history.
///
/// Implementations store current SN state and a transition audit log.
/// Business logic (validation, state machine) lives in SerialNumberService, not here.
#[trait_variant::make(Send)]
pub trait SerialNumberRepository: Sync {
    /// Upsert a serial number's current state.
    /// Creates the record on first encounter, updates on subsequent calls.
    async fn upsert_state(
        &self,
        epc: &Epc,
        state: SnState,
        sid_class: Option<&str>,
        pool_id: Option<&str>,
    ) -> Result<(), RepoError>;

    /// Get current state of a serial number. Returns None if never tracked.
    async fn get_state(&self, epc: &Epc) -> Result<Option<SerialNumber>, RepoError>;

    /// Query serial numbers with filters.
    async fn query(&self, query: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError>;

    /// Record a state transition in the audit log.
    async fn record_transition(&self, transition: &SnTransition) -> Result<(), RepoError>;

    /// Get transition history for an EPC, newest-first.
    async fn get_history(&self, epc: &Epc, limit: u32) -> Result<Vec<SnTransition>, RepoError>;
}
```

**Step 2: Register the module**

In `crates/ephemeris-core/src/repository/mod.rs`, add:

```rust
pub mod serial_number;
pub use serial_number::*;
```

**Step 3: Verify it compiles**

Run: `cargo check -p ephemeris-core`
Expected: PASS (no errors)

**Step 4: Commit**

```bash
git add crates/ephemeris-core/src/repository/serial_number.rs crates/ephemeris-core/src/repository/mod.rs
git commit -m "feat(core): add SerialNumberRepository trait"
```

---

## Task 5: Service Layer — SerialNumberService

**Files:**
- Create: `crates/ephemeris-core/src/service/mod.rs`
- Create: `crates/ephemeris-core/src/service/serial_number.rs`
- Modify: `crates/ephemeris-core/src/lib.rs`
- Modify: `crates/ephemeris-core/Cargo.toml` (add tracing dependency)

**Step 1: Add tracing to ephemeris-core deps**

In `crates/ephemeris-core/Cargo.toml`, add to `[dependencies]`:

```toml
tracing = { workspace = true }
```

**Step 2: Write the failing tests**

Create `crates/ephemeris-core/src/service/serial_number.rs` with the service struct and tests:

```rust
use crate::domain::{
    biz_step_to_target_state, is_valid_transition, Epc, EventId, SerialNumber, SerialNumberQuery,
    SnState, SnTransition, TransitionSource,
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

        self.repo.upsert_state(epc, target_state, None, None).await?;

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
    pub async fn get_history(
        &self,
        epc: &Epc,
        limit: u32,
    ) -> Result<Vec<SnTransition>, RepoError> {
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
    use mockall::mock;

    mock! {
        pub SnRepo {}

        impl SerialNumberRepository for SnRepo {
            async fn upsert_state(
                &self,
                epc: &Epc,
                state: SnState,
                sid_class: Option<&str>,
                pool_id: Option<&str>,
            ) -> Result<(), RepoError>;

            async fn get_state(&self, epc: &Epc) -> Result<Option<SerialNumber>, RepoError>;

            async fn query(&self, query: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError>;

            async fn record_transition(&self, transition: &SnTransition) -> Result<(), RepoError>;

            async fn get_history(
                &self,
                epc: &Epc,
                limit: u32,
            ) -> Result<Vec<SnTransition>, RepoError>;
        }
    }

    #[tokio::test]
    async fn test_process_transition_valid() {
        let mut mock = MockSnRepo::new();

        // get_state returns Encoded
        mock.expect_get_state().times(1).returning(|_| {
            Ok(Some(SerialNumber {
                epc: Epc::new("urn:epc:id:sgtin:0614141.107346.2017"),
                state: SnState::Encoded,
                sid_class: None,
                pool_id: None,
                updated_at: chrono::Utc::now().fixed_offset(),
                created_at: chrono::Utc::now().fixed_offset(),
            }))
        });
        mock.expect_upsert_state().times(1).returning(|_, _, _, _| Ok(()));
        mock.expect_record_transition().times(1).returning(|_| Ok(()));

        let service = SerialNumberService::new(mock);
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        let result = service
            .process_transition(&epc, "commissioning", None, TransitionSource::Mqtt)
            .await
            .unwrap();

        assert_eq!(result, Some(SnState::Commissioned));
    }

    #[tokio::test]
    async fn test_process_transition_invalid_still_applies() {
        let mut mock = MockSnRepo::new();

        // get_state returns Unassigned — jumping to Commissioned is invalid
        mock.expect_get_state().times(1).returning(|_| Ok(None));
        mock.expect_upsert_state().times(1).returning(|_, _, _, _| Ok(()));
        mock.expect_record_transition().times(1).returning(|_| Ok(()));

        let service = SerialNumberService::new(mock);
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
        let mock = MockSnRepo::new();
        // No repo calls expected — packing doesn't change SN state

        let service = SerialNumberService::new(mock);
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        let result = service
            .process_transition(&epc, "packing", None, TransitionSource::Mqtt)
            .await
            .unwrap();

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_manual_override() {
        let mut mock = MockSnRepo::new();

        mock.expect_get_state().times(1).returning(|_| {
            Ok(Some(SerialNumber {
                epc: Epc::new("urn:epc:id:sgtin:0614141.107346.2017"),
                state: SnState::Commissioned,
                sid_class: None,
                pool_id: None,
                updated_at: chrono::Utc::now().fixed_offset(),
                created_at: chrono::Utc::now().fixed_offset(),
            }))
        });
        mock.expect_upsert_state().times(1).returning(|_, _, _, _| Ok(()));
        mock.expect_record_transition().times(1).returning(|_| Ok(()));

        let service = SerialNumberService::new(mock);
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        let result = service
            .manual_override(&epc, SnState::Destroyed, "line scanner missed event")
            .await
            .unwrap();

        assert_eq!(result, SnState::Destroyed);
    }
}
```

**Step 3: Create the module files**

Create `crates/ephemeris-core/src/service/mod.rs`:

```rust
pub mod serial_number;

pub use serial_number::SerialNumberService;
```

Add to `crates/ephemeris-core/src/lib.rs`:

```rust
pub mod service;
```

**Step 4: Run tests**

Run: `cargo test -p ephemeris-core --lib service::serial_number`
Expected: 4 tests PASS

**Step 5: Commit**

```bash
git add crates/ephemeris-core/src/service/ crates/ephemeris-core/src/lib.rs crates/ephemeris-core/Cargo.toml
git commit -m "feat(core): add SerialNumberService with state machine logic"
```

---

## Task 6: PostgreSQL Schema — SN Tables

**Files:**
- Modify: `crates/ephemeris-pg/src/schema.rs`

**Step 1: Add SN tables to the schema**

In `crates/ephemeris-pg/src/schema.rs`, append to the `INIT_SCHEMA` string (before the closing `"#;`):

```sql

-- Serial Number State Tracking (OPEN-SCS lifecycle)
CREATE TABLE IF NOT EXISTS serial_numbers (
    epc         TEXT PRIMARY KEY,
    state       TEXT NOT NULL,
    sid_class   TEXT,
    pool_id     TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_sn_state ON serial_numbers (state);
CREATE INDEX IF NOT EXISTS idx_sn_sid_class ON serial_numbers (sid_class) WHERE sid_class IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_sn_pool ON serial_numbers (pool_id) WHERE pool_id IS NOT NULL;

-- Serial Number Transition Audit Log
CREATE TABLE IF NOT EXISTS sn_transitions (
    id          SERIAL PRIMARY KEY,
    epc         TEXT NOT NULL,
    from_state  TEXT NOT NULL,
    to_state    TEXT NOT NULL,
    biz_step    TEXT NOT NULL,
    event_id    UUID REFERENCES epcis_events(id),
    source      TEXT NOT NULL,
    timestamp   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_snt_epc ON sn_transitions (epc, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_snt_event ON sn_transitions (event_id) WHERE event_id IS NOT NULL;
```

**Step 2: Verify it compiles**

Run: `cargo check -p ephemeris-pg`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/ephemeris-pg/src/schema.rs
git commit -m "feat(pg): add serial_numbers and sn_transitions schema"
```

---

## Task 7: PostgreSQL Implementation — PgSerialNumberRepository

**Files:**
- Create: `crates/ephemeris-pg/src/serial_number_repo.rs`
- Modify: `crates/ephemeris-pg/src/lib.rs`

**Step 1: Write the integration tests**

Create `crates/ephemeris-pg/src/serial_number_repo.rs`:

```rust
use std::str::FromStr;

use deadpool_postgres::Pool;
use ephemeris_core::domain::{
    Epc, EventId, SerialNumber, SerialNumberQuery, SnState, SnTransition, TransitionSource,
};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::SerialNumberRepository;

/// PostgreSQL-backed serial number state repository.
#[derive(Clone)]
pub struct PgSerialNumberRepository {
    pool: Pool,
}

impl PgSerialNumberRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

impl SerialNumberRepository for PgSerialNumberRepository {
    async fn upsert_state(
        &self,
        epc: &Epc,
        state: SnState,
        sid_class: Option<&str>,
        pool_id: Option<&str>,
    ) -> Result<(), RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        client
            .execute(
                "INSERT INTO serial_numbers (epc, state, sid_class, pool_id)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (epc) DO UPDATE SET
                     state = EXCLUDED.state,
                     sid_class = COALESCE(EXCLUDED.sid_class, serial_numbers.sid_class),
                     pool_id = COALESCE(EXCLUDED.pool_id, serial_numbers.pool_id),
                     updated_at = now()",
                &[&epc.as_str(), &state.to_string(), &sid_class, &pool_id],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_state(&self, epc: &Epc) -> Result<Option<SerialNumber>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let row = client
            .query_opt(
                "SELECT epc, state, sid_class, pool_id, created_at, updated_at
                 FROM serial_numbers WHERE epc = $1",
                &[&epc.as_str()],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        match row {
            Some(row) => {
                let state_str: String = row.get(1);
                Ok(Some(SerialNumber {
                    epc: Epc::new(row.get::<_, &str>(0)),
                    state: SnState::from_str(&state_str)
                        .map_err(|e| RepoError::Serialization(e))?,
                    sid_class: row.get(2),
                    pool_id: row.get(3),
                    created_at: row.get(4),
                    updated_at: row.get(5),
                }))
            }
            None => Ok(None),
        }
    }

    async fn query(&self, query: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let mut sql = String::from(
            "SELECT epc, state, sid_class, pool_id, created_at, updated_at FROM serial_numbers WHERE 1=1",
        );
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = Vec::new();
        let mut idx = 1;

        if let Some(ref state) = query.state {
            sql.push_str(&format!(" AND state = ${idx}"));
            params.push(Box::new(state.to_string()));
            idx += 1;
        }
        if let Some(ref sid_class) = query.sid_class {
            sql.push_str(&format!(" AND sid_class = ${idx}"));
            params.push(Box::new(sid_class.clone()));
            idx += 1;
        }
        if let Some(ref pool_id) = query.pool_id {
            sql.push_str(&format!(" AND pool_id = ${idx}"));
            params.push(Box::new(pool_id.clone()));
            idx += 1;
        }

        let limit = query.limit.unwrap_or(100) as i64;
        let offset = query.offset.unwrap_or(0) as i64;
        sql.push_str(&format!(" ORDER BY updated_at DESC LIMIT ${idx} OFFSET ${}", idx + 1));
        params.push(Box::new(limit));
        params.push(Box::new(offset));

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref() as _).collect();

        let rows = client
            .query(&sql, &param_refs)
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        rows.iter()
            .map(|row| {
                let state_str: String = row.get(1);
                Ok(SerialNumber {
                    epc: Epc::new(row.get::<_, &str>(0)),
                    state: SnState::from_str(&state_str)
                        .map_err(|e| RepoError::Serialization(e))?,
                    sid_class: row.get(2),
                    pool_id: row.get(3),
                    created_at: row.get(4),
                    updated_at: row.get(5),
                })
            })
            .collect()
    }

    async fn record_transition(&self, transition: &SnTransition) -> Result<(), RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let event_id = transition.event_id.as_ref().map(|e| e.0);

        client
            .execute(
                "INSERT INTO sn_transitions (epc, from_state, to_state, biz_step, event_id, source, timestamp)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &transition.epc.as_str(),
                    &transition.from_state.to_string(),
                    &transition.to_state.to_string(),
                    &transition.biz_step,
                    &event_id,
                    &format!("{:?}", transition.source).to_lowercase(),
                    &transition.timestamp,
                ],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_history(
        &self,
        epc: &Epc,
        limit: u32,
    ) -> Result<Vec<SnTransition>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let rows = client
            .query(
                "SELECT epc, from_state, to_state, biz_step, event_id, source, timestamp
                 FROM sn_transitions WHERE epc = $1
                 ORDER BY timestamp DESC LIMIT $2",
                &[&epc.as_str(), &(limit as i64)],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        rows.iter()
            .map(|row| {
                let from_str: String = row.get(1);
                let to_str: String = row.get(2);
                let source_str: String = row.get(5);
                Ok(SnTransition {
                    epc: Epc::new(row.get::<_, &str>(0)),
                    from_state: SnState::from_str(&from_str)
                        .map_err(|e| RepoError::Serialization(e))?,
                    to_state: SnState::from_str(&to_str)
                        .map_err(|e| RepoError::Serialization(e))?,
                    biz_step: row.get(3),
                    event_id: row.get::<_, Option<uuid::Uuid>>(4).map(EventId),
                    source: match source_str.as_str() {
                        "mqtt" => TransitionSource::Mqtt,
                        "restapi" | "rest_api" => TransitionSource::RestApi,
                        _ => TransitionSource::System,
                    },
                    timestamp: row.get(6),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

    async fn setup_test_db() -> (PgSerialNumberRepository, impl std::any::Any) {
        let container = Postgres::default().start().await.unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();

        let mut cfg = deadpool_postgres::Config::new();
        cfg.host = Some(host.to_string());
        cfg.port = Some(port);
        cfg.user = Some("postgres".to_string());
        cfg.password = Some("postgres".to_string());
        cfg.dbname = Some("postgres".to_string());
        let pool = cfg
            .create_pool(
                Some(deadpool_postgres::Runtime::Tokio1),
                tokio_postgres::NoTls,
            )
            .unwrap();

        // Run migrations
        let client = pool.get().await.unwrap();
        client
            .batch_execute(crate::schema::INIT_SCHEMA)
            .await
            .unwrap();

        let repo = PgSerialNumberRepository::new(pool);
        (repo, container)
    }

    #[tokio::test]
    async fn test_upsert_and_get_state() {
        let (repo, _container) = setup_test_db().await;
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");

        // Initially no state
        assert!(repo.get_state(&epc).await.unwrap().is_none());

        // Insert
        repo.upsert_state(&epc, SnState::Commissioned, Some("sgtin"), None)
            .await
            .unwrap();

        let sn = repo.get_state(&epc).await.unwrap().unwrap();
        assert_eq!(sn.state, SnState::Commissioned);
        assert_eq!(sn.sid_class.as_deref(), Some("sgtin"));

        // Update
        repo.upsert_state(&epc, SnState::Released, None, None)
            .await
            .unwrap();

        let sn = repo.get_state(&epc).await.unwrap().unwrap();
        assert_eq!(sn.state, SnState::Released);
        // sid_class should be preserved (COALESCE)
        assert_eq!(sn.sid_class.as_deref(), Some("sgtin"));
    }

    #[tokio::test]
    async fn test_query_by_state() {
        let (repo, _container) = setup_test_db().await;

        repo.upsert_state(
            &Epc::new("urn:epc:id:sgtin:0614141.107346.001"),
            SnState::Commissioned,
            None,
            None,
        )
        .await
        .unwrap();
        repo.upsert_state(
            &Epc::new("urn:epc:id:sgtin:0614141.107346.002"),
            SnState::Released,
            None,
            None,
        )
        .await
        .unwrap();
        repo.upsert_state(
            &Epc::new("urn:epc:id:sgtin:0614141.107346.003"),
            SnState::Commissioned,
            None,
            None,
        )
        .await
        .unwrap();

        let query = SerialNumberQuery {
            state: Some(SnState::Commissioned),
            ..Default::default()
        };
        let results = repo.query(&query).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_record_and_get_history() {
        let (repo, _container) = setup_test_db().await;
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");

        let t1 = SnTransition {
            epc: epc.clone(),
            from_state: SnState::Unassigned,
            to_state: SnState::Commissioned,
            biz_step: "commissioning".to_string(),
            event_id: None,
            source: TransitionSource::Mqtt,
            timestamp: chrono::Utc::now().fixed_offset(),
        };
        repo.record_transition(&t1).await.unwrap();

        let t2 = SnTransition {
            epc: epc.clone(),
            from_state: SnState::Commissioned,
            to_state: SnState::Released,
            biz_step: "shipping".to_string(),
            event_id: None,
            source: TransitionSource::Mqtt,
            timestamp: chrono::Utc::now().fixed_offset(),
        };
        repo.record_transition(&t2).await.unwrap();

        let history = repo.get_history(&epc, 10).await.unwrap();
        assert_eq!(history.len(), 2);
        // Newest first
        assert_eq!(history[0].to_state, SnState::Released);
        assert_eq!(history[1].to_state, SnState::Commissioned);
    }
}
```

**Step 2: Register in lib.rs**

Add to `crates/ephemeris-pg/src/lib.rs`:

```rust
pub mod serial_number_repo;
pub use serial_number_repo::PgSerialNumberRepository;
```

**Step 3: Run integration tests**

Run: `cargo test -p ephemeris-pg --lib serial_number_repo`
Expected: 3 integration tests PASS (requires Docker for testcontainers)

**Step 4: Commit**

```bash
git add crates/ephemeris-pg/src/serial_number_repo.rs crates/ephemeris-pg/src/lib.rs
git commit -m "feat(pg): add PgSerialNumberRepository with integration tests"
```

---

## Task 8: MQTT Handler — Add SN Service Integration

**Files:**
- Modify: `crates/ephemeris-mqtt/src/handler.rs`
- Modify: `crates/ephemeris-mqtt/src/lib.rs`
- Modify: `crates/ephemeris-mqtt/Cargo.toml`

**Step 1: Update Cargo.toml**

`ephemeris-mqtt` needs `chrono` in dev-dependencies (already there). No new deps needed — `tracing` is already a dependency.

**Step 2: Update EventHandler to accept SN service**

Replace the entire `crates/ephemeris-mqtt/src/handler.rs` with:

```rust
use ephemeris_core::domain::{Action, Epc, EpcisEvent};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};
use ephemeris_core::service::SerialNumberService;
use ephemeris_core::domain::TransitionSource;

/// Handles incoming EPCIS events by routing them to the appropriate repositories.
///
/// Stores every event via the event repository, updates aggregation hierarchy
/// for aggregation events, and drives serial number state transitions based
/// on the event's bizStep.
pub struct EventHandler<E, A, S> {
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
                    .process_transition(
                        &epc,
                        biz_step,
                        Some(&stored_id),
                        TransitionSource::Mqtt,
                    )
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
            EpcisEvent::ObjectEvent(data) => {
                data.epc_list.iter().map(|s| Epc::new(s)).collect()
            }
            EpcisEvent::AggregationEvent(data) => {
                let mut epcs: Vec<Epc> = data.child_epcs.iter().map(|s| Epc::new(s)).collect();
                if let Some(ref parent) = data.parent_id {
                    epcs.push(Epc::new(parent));
                }
                epcs
            }
            EpcisEvent::TransformationEvent(data) => {
                let mut epcs: Vec<Epc> =
                    data.input_epc_list.iter().map(|s| Epc::new(s)).collect();
                epcs.extend(data.output_epc_list.iter().map(|s| Epc::new(s)));
                epcs
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephemeris_core::domain::{
        AggregationEventData, AggregationTree, CommonEventFields, EventId, EventQuery,
        ObjectEventData, SerialNumber, SerialNumberQuery, SnState, SnTransition,
    };
    use mockall::mock;

    mock! {
        pub EventRepo {}

        impl EventRepository for EventRepo {
            async fn store_event(&self, event: &EpcisEvent) -> Result<EventId, RepoError>;
            async fn get_event(&self, id: &EventId) -> Result<Option<EpcisEvent>, RepoError>;
            async fn query_events(
                &self,
                query: &EventQuery,
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

    mock! {
        pub SnRepo {}

        impl SerialNumberRepository for SnRepo {
            async fn upsert_state(
                &self,
                epc: &Epc,
                state: SnState,
                sid_class: Option<&str>,
                pool_id: Option<&str>,
            ) -> Result<(), RepoError>;

            async fn get_state(&self, epc: &Epc) -> Result<Option<SerialNumber>, RepoError>;

            async fn query(&self, query: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError>;

            async fn record_transition(&self, transition: &SnTransition) -> Result<(), RepoError>;

            async fn get_history(
                &self,
                epc: &Epc,
                limit: u32,
            ) -> Result<Vec<SnTransition>, RepoError>;
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
        let mock_sn = MockSnRepo::new();
        // No SN calls expected when no bizStep

        mock_event
            .expect_store_event()
            .times(1)
            .returning(|_| Ok(EventId::new()));

        let sn_service = SerialNumberService::new(mock_sn);
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
        let mut mock_sn = MockSnRepo::new();

        mock_event
            .expect_store_event()
            .times(1)
            .returning(|_| Ok(EventId::new()));

        // SN service will call get_state, upsert_state, record_transition
        mock_sn
            .expect_get_state()
            .times(1)
            .returning(|_| Ok(None));
        mock_sn
            .expect_upsert_state()
            .times(1)
            .returning(|_, _, _, _| Ok(()));
        mock_sn
            .expect_record_transition()
            .times(1)
            .returning(|_| Ok(()));

        let sn_service = SerialNumberService::new(mock_sn);
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
        let mock_sn = MockSnRepo::new();
        // packing bizStep -> no SN state change, so no SN repo calls

        mock_event
            .expect_store_event()
            .times(1)
            .returning(|_| Ok(EventId::new()));

        mock_agg
            .expect_add_child()
            .times(2)
            .returning(|_, _, _| Ok(()));

        let sn_service = SerialNumberService::new(mock_sn);
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
        let mock_sn = MockSnRepo::new();

        mock_event
            .expect_store_event()
            .times(1)
            .returning(|_| Ok(EventId::new()));

        mock_agg
            .expect_remove_child()
            .times(2)
            .returning(|_, _| Ok(()));

        let sn_service = SerialNumberService::new(mock_sn);
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
```

**Step 3: Update `crates/ephemeris-mqtt/src/lib.rs`**

No changes needed — `EventHandler` is already re-exported.

**Step 4: Update `crates/ephemeris-mqtt/src/subscriber.rs`**

The `MqttSubscriber::run` method signature needs to add the `S` generic. Check and update the signature to:

```rust
pub async fn run<E, A, S>(mut self, handler: EventHandler<E, A, S>)
where
    E: EventRepository + 'static,
    A: AggregationRepository + 'static,
    S: SerialNumberRepository + 'static,
```

**Step 5: Run tests**

Run: `cargo test -p ephemeris-mqtt --lib`
Expected: 4 tests PASS (old 3 tests updated + 1 new)

**Step 6: Commit**

```bash
git add crates/ephemeris-mqtt/
git commit -m "feat(mqtt): integrate SN state transitions into EventHandler"
```

---

## Task 9: API Routes — Serial Number Endpoints

**Files:**
- Create: `crates/ephemeris-api/src/routes/serial_numbers.rs`
- Modify: `crates/ephemeris-api/src/routes/mod.rs`
- Modify: `crates/ephemeris-api/src/state.rs`
- Modify: `crates/ephemeris-api/src/lib.rs`

**Step 1: Update AppState**

Replace `crates/ephemeris-api/src/state.rs` with:

```rust
use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};
use ephemeris_core::service::SerialNumberService;

/// Shared application state holding repository implementations and services.
pub struct AppState<E: EventRepository, A: AggregationRepository, S: SerialNumberRepository> {
    pub event_repo: E,
    pub agg_repo: A,
    pub sn_service: SerialNumberService<S>,
}
```

**Step 2: Create serial number routes**

Create `crates/ephemeris-api/src/routes/serial_numbers.rs`:

```rust
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use ephemeris_core::domain::{Epc, SerialNumberQuery, SnState};
use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};

use crate::state::AppState;

/// GET /serial-numbers/{epc} — get current SN state.
pub async fn get_sn_state<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Path(epc): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let epc = Epc::new(epc);
    match state.sn_service.get_state(&epc).await {
        Ok(Some(sn)) => Ok(Json(serde_json::to_value(sn).unwrap())),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "serial number not tracked"})),
        )),
        Err(e) => {
            tracing::error!("Failed to get SN state: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}

/// GET /serial-numbers/{epc}/history — get transition audit trail.
pub async fn get_sn_history<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Path(epc): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let epc = Epc::new(epc);
    state
        .sn_service
        .get_history(&epc, 100)
        .await
        .map(|h| Json(serde_json::to_value(h).unwrap()))
        .map_err(|e| {
            tracing::error!("Failed to get SN history: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })
}

/// GET /serial-numbers — query serial numbers by state/filters.
pub async fn query_serial_numbers<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Query(query): Query<SerialNumberQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .sn_service
        .query(&query)
        .await
        .map(|sns| Json(serde_json::to_value(sns).unwrap()))
        .map_err(|e| {
            tracing::error!("Failed to query serial numbers: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })
}

/// POST body for manual state override.
#[derive(Deserialize)]
pub struct TransitionRequest {
    pub target_state: SnState,
    #[serde(default)]
    pub reason: String,
}

/// POST /serial-numbers/{epc}/transition — manual state override.
pub async fn manual_transition<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Path(epc): Path<String>,
    Json(req): Json<TransitionRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let epc = Epc::new(epc);
    match state
        .sn_service
        .manual_override(&epc, req.target_state, &req.reason)
        .await
    {
        Ok(new_state) => Ok((
            StatusCode::OK,
            Json(json!({"epc": epc.as_str(), "state": new_state.to_string()})),
        )),
        Err(e) => {
            tracing::error!("Failed to override SN state: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}
```

**Step 3: Update routes/mod.rs**

Add to `crates/ephemeris-api/src/routes/mod.rs`:

```rust
pub mod serial_numbers;
```

**Step 4: Update the router in lib.rs**

Replace `crates/ephemeris-api/src/lib.rs` router and generics to include `S`:

```rust
pub mod routes;
pub mod state;

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};

use crate::routes::{events, health, hierarchy, serial_numbers};
pub use crate::state::AppState;

/// Build the Axum router with all API routes.
pub fn create_router<E, A, S>(state: Arc<AppState<E, A, S>>) -> Router
where
    E: EventRepository + 'static,
    A: AggregationRepository + 'static,
    S: SerialNumberRepository + 'static,
{
    Router::new()
        .route("/health", get(health::health_check))
        .route("/events", get(events::query_events::<E, A, S>))
        .route("/events", post(events::capture_event::<E, A, S>))
        .route("/events/{event_id}", get(events::get_event::<E, A, S>))
        .route(
            "/hierarchy/{epc}",
            get(hierarchy::get_full_hierarchy::<E, A, S>),
        )
        .route(
            "/hierarchy/{epc}/children",
            get(hierarchy::get_children::<E, A, S>),
        )
        .route(
            "/hierarchy/{epc}/ancestors",
            get(hierarchy::get_ancestors::<E, A, S>),
        )
        .route(
            "/serial-numbers",
            get(serial_numbers::query_serial_numbers::<E, A, S>),
        )
        .route(
            "/serial-numbers/{epc}",
            get(serial_numbers::get_sn_state::<E, A, S>),
        )
        .route(
            "/serial-numbers/{epc}/history",
            get(serial_numbers::get_sn_history::<E, A, S>),
        )
        .route(
            "/serial-numbers/{epc}/transition",
            post(serial_numbers::manual_transition::<E, A, S>),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
```

**Step 5: Update existing route handlers to include S generic**

All existing route handlers in `events.rs`, `hierarchy.rs`, and `health.rs` need the `S` generic added to their signatures. For each handler, change:

`<E: EventRepository, A: AggregationRepository>` to `<E: EventRepository, A: AggregationRepository, S: SerialNumberRepository>`

And update the `State` type from `Arc<AppState<E, A>>` to `Arc<AppState<E, A, S>>`.

Add to the imports in each file:
```rust
use ephemeris_core::repository::SerialNumberRepository;
```

**Step 6: Update tests in lib.rs**

Add a `StubSnRepo` to the existing test module and update the test to use `AppState<E, A, S>`:

```rust
struct StubSnRepo;

impl ephemeris_core::repository::SerialNumberRepository for StubSnRepo {
    async fn upsert_state(&self, _: &Epc, _: SnState, _: Option<&str>, _: Option<&str>) -> Result<(), RepoError> { Ok(()) }
    async fn get_state(&self, _: &Epc) -> Result<Option<SerialNumber>, RepoError> { Ok(None) }
    async fn query(&self, _: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError> { Ok(vec![]) }
    async fn record_transition(&self, _: &SnTransition) -> Result<(), RepoError> { Ok(()) }
    async fn get_history(&self, _: &Epc, _: u32) -> Result<Vec<SnTransition>, RepoError> { Ok(vec![]) }
}
```

Update the health test's state construction:
```rust
let state = Arc::new(AppState {
    event_repo: StubEventRepo,
    agg_repo: StubAggRepo,
    sn_service: SerialNumberService::new(StubSnRepo),
});
```

**Step 7: Run tests**

Run: `cargo test -p ephemeris-api --lib`
Expected: PASS

**Step 8: Commit**

```bash
git add crates/ephemeris-api/
git commit -m "feat(api): add serial number REST endpoints and update generics"
```

---

## Task 10: App Wiring — Connect Everything in main.rs

**Files:**
- Modify: `crates/ephemeris-app/src/main.rs`

**Step 1: Update main.rs**

Key changes:
1. Import `SerialNumberRepository` and `SerialNumberService`
2. Construct `PgSerialNumberRepository` from the same PG pool
3. Wrap in `SerialNumberService`
4. Pass to `EventHandler::new` and `AppState`
5. Update `run_app` signature to include `S` generic

Update imports at the top:
```rust
use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};
use ephemeris_core::service::SerialNumberService;
```

In the `"postgres"` branch, after creating `agg_repo`, add:
```rust
let sn_pool = build_pg_pool(&conn_str, pg_cfg.pool_size)?;
let sn_repo = ephemeris_pg::PgSerialNumberRepository::new(sn_pool);
run_app(event_repo, agg_repo, sn_repo, app_config).await
```

In the `"arango"` branch, similarly add the SN repo:
```rust
let sn_pool = build_pg_pool(&conn_str, pg_cfg.pool_size)?;
let sn_repo = ephemeris_pg::PgSerialNumberRepository::new(sn_pool);
run_app(event_repo, agg_repo, sn_repo, app_config).await
```

Update `run_app` signature:
```rust
async fn run_app<E, A, S>(
    event_repo: E,
    agg_repo: A,
    sn_repo: S,
    app_config: AppConfig,
) -> Result<(), Box<dyn std::error::Error>>
where
    E: EventRepository + Clone + 'static,
    A: AggregationRepository + Clone + 'static,
    S: SerialNumberRepository + Clone + 'static,
{
    let sn_service = SerialNumberService::new(sn_repo.clone());

    let state = Arc::new(AppState {
        event_repo: event_repo.clone(),
        agg_repo: agg_repo.clone(),
        sn_service: SerialNumberService::new(sn_repo),
    });

    let router = ephemeris_api::create_router(state);
    // ... rest unchanged ...

    let handler = EventHandler::new(event_repo, agg_repo, sn_service);
    // ... rest unchanged ...
}
```

**Step 2: Build check**

Run: `cargo build -p ephemeris-app`
Expected: PASS

Run: `cargo build -p ephemeris-app --features enterprise-arango`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/ephemeris-app/src/main.rs
git commit -m "feat(app): wire SN repository and service into app startup"
```

---

## Task 11: Full Validation

**Step 1: Format check**

Run: `cargo fmt --check`
Expected: PASS

**Step 2: Clippy (default)**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: PASS (fix any warnings)

**Step 3: Clippy (enterprise)**

Run: `cargo clippy --all-targets --features enterprise -- -D warnings`
Expected: PASS

**Step 4: License check**

Run: `cargo deny check licenses`
Expected: PASS (no new deps added)

**Step 5: All unit tests**

Run: `cargo test --workspace --lib`
Expected: All PASS (existing 32 + new ~20)

**Step 6: Build both configurations**

Run: `cargo build -p ephemeris-app`
Run: `cargo build -p ephemeris-app --features enterprise-arango`
Expected: Both PASS

**Step 7: Commit any fixes**

```bash
git add -A
git commit -m "chore: fix clippy/fmt issues from SN lifecycle feature"
```
