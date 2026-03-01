use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Serial number lifecycle state per the OPEN-SCS standard.
///
/// Tracks the 12 possible states a serial number can occupy
/// from initial generation through final release or destruction.
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

/// Map a bizStep string to the target SN state.
/// Returns None for events that don't change SN state (packing, unpacking, label_inspecting).
/// Accepts both shorthand ("commissioning") and URI formats.
pub fn biz_step_to_target_state(biz_step: &str) -> Option<SnState> {
    let shorthand = biz_step
        .strip_prefix("urn:epcglobal:cbv:bizstep:")
        .or_else(|| biz_step.strip_prefix("http://open-scs.org/bizstep/"))
        .unwrap_or(biz_step);

    match shorthand {
        "provisioning" | "sn_deallocating" => Some(SnState::Unallocated),
        "sn_returning" => Some(SnState::Unassigned),
        "sn_allocating" => Some(SnState::Allocated),
        "sn_invalidating" => Some(SnState::SnInvalid),
        "sn_encoding" => Some(SnState::Encoded),
        "label_sampling" => Some(SnState::LabelSampled),
        "label_scrapping" => Some(SnState::LabelScrapped),
        "commissioning" => Some(SnState::Commissioned),
        "inspecting" => Some(SnState::Sampled),
        "shipping" => Some(SnState::Released),
        "decommissioning" => Some(SnState::Inactive),
        "destroying" => Some(SnState::Destroyed),
        _ => None,
    }
}

/// Check if a state transition is valid per OPEN-SCS PSS section 5 Figure 4.
/// Used for permissive warnings, not enforcement.
pub fn is_valid_transition(from: SnState, to: SnState) -> bool {
    matches!(
        (from, to),
        (SnState::Unassigned, SnState::Unallocated)
            | (SnState::Unallocated, SnState::Unassigned)
            | (SnState::Unallocated, SnState::Allocated)
            | (SnState::Unallocated, SnState::SnInvalid)
            | (SnState::Allocated, SnState::Unallocated)
            | (SnState::Allocated, SnState::Encoded)
            | (SnState::Allocated, SnState::SnInvalid)
            | (SnState::Encoded, SnState::LabelSampled)
            | (SnState::Encoded, SnState::LabelScrapped)
            | (SnState::Encoded, SnState::Commissioned)
            | (SnState::Commissioned, SnState::Sampled)
            | (SnState::Commissioned, SnState::Inactive)
            | (SnState::Commissioned, SnState::Destroyed)
            | (SnState::Commissioned, SnState::Released)
    )
}

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
            other => Err(format!("unknown SnState: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sn_state_display_roundtrip() {
        let all_states = [
            SnState::Unassigned,
            SnState::Unallocated,
            SnState::Allocated,
            SnState::SnInvalid,
            SnState::Encoded,
            SnState::LabelSampled,
            SnState::LabelScrapped,
            SnState::Commissioned,
            SnState::Sampled,
            SnState::Inactive,
            SnState::Destroyed,
            SnState::Released,
        ];

        for state in all_states {
            let s = state.to_string();
            let parsed: SnState = s.parse().unwrap();
            assert_eq!(parsed, state, "roundtrip failed for {state:?}");
        }
    }

    #[test]
    fn test_sn_state_from_str_invalid() {
        assert!("bogus".parse::<SnState>().is_err());
        assert!("".parse::<SnState>().is_err());
    }

    #[test]
    fn test_sn_state_serde_roundtrip() {
        let state = SnState::Commissioned;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"commissioned\"");
        let deserialized: SnState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, state);
    }

    #[test]
    fn test_biz_step_to_target_state_shorthand() {
        assert_eq!(
            biz_step_to_target_state("provisioning"),
            Some(SnState::Unallocated)
        );
        assert_eq!(
            biz_step_to_target_state("sn_returning"),
            Some(SnState::Unassigned)
        );
        assert_eq!(
            biz_step_to_target_state("sn_allocating"),
            Some(SnState::Allocated)
        );
        assert_eq!(
            biz_step_to_target_state("sn_deallocating"),
            Some(SnState::Unallocated)
        );
        assert_eq!(
            biz_step_to_target_state("sn_invalidating"),
            Some(SnState::SnInvalid)
        );
        assert_eq!(
            biz_step_to_target_state("sn_encoding"),
            Some(SnState::Encoded)
        );
        assert_eq!(
            biz_step_to_target_state("label_sampling"),
            Some(SnState::LabelSampled)
        );
        assert_eq!(
            biz_step_to_target_state("label_scrapping"),
            Some(SnState::LabelScrapped)
        );
        assert_eq!(
            biz_step_to_target_state("commissioning"),
            Some(SnState::Commissioned)
        );
        assert_eq!(
            biz_step_to_target_state("inspecting"),
            Some(SnState::Sampled)
        );
        assert_eq!(
            biz_step_to_target_state("shipping"),
            Some(SnState::Released)
        );
        assert_eq!(
            biz_step_to_target_state("decommissioning"),
            Some(SnState::Inactive)
        );
        assert_eq!(
            biz_step_to_target_state("destroying"),
            Some(SnState::Destroyed)
        );
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
            Some(SnState::Commissioned),
        );
        assert_eq!(
            biz_step_to_target_state("urn:epcglobal:cbv:bizstep:shipping"),
            Some(SnState::Released),
        );
        assert_eq!(
            biz_step_to_target_state("http://open-scs.org/bizstep/sn_encoding"),
            Some(SnState::Encoded),
        );
    }

    #[test]
    fn test_valid_transitions() {
        // Valid transitions from Unassigned
        assert!(is_valid_transition(
            SnState::Unassigned,
            SnState::Unallocated
        ));
        assert!(!is_valid_transition(
            SnState::Unassigned,
            SnState::Commissioned
        ));

        // Valid transitions from Unallocated
        assert!(is_valid_transition(
            SnState::Unallocated,
            SnState::Unassigned
        ));
        assert!(is_valid_transition(
            SnState::Unallocated,
            SnState::Allocated
        ));
        assert!(is_valid_transition(
            SnState::Unallocated,
            SnState::SnInvalid
        ));
        assert!(!is_valid_transition(
            SnState::Unallocated,
            SnState::Commissioned
        ));

        // Valid transitions from Allocated
        assert!(is_valid_transition(
            SnState::Allocated,
            SnState::Unallocated
        ));
        assert!(is_valid_transition(SnState::Allocated, SnState::Encoded));
        assert!(is_valid_transition(SnState::Allocated, SnState::SnInvalid));
        assert!(!is_valid_transition(SnState::Allocated, SnState::Released));

        // Valid transitions from Encoded
        assert!(is_valid_transition(SnState::Encoded, SnState::LabelSampled));
        assert!(is_valid_transition(
            SnState::Encoded,
            SnState::LabelScrapped
        ));
        assert!(is_valid_transition(SnState::Encoded, SnState::Commissioned));
        assert!(!is_valid_transition(SnState::Encoded, SnState::Unassigned));

        // Valid transitions from Commissioned
        assert!(is_valid_transition(SnState::Commissioned, SnState::Sampled));
        assert!(is_valid_transition(
            SnState::Commissioned,
            SnState::Inactive
        ));
        assert!(is_valid_transition(
            SnState::Commissioned,
            SnState::Destroyed
        ));
        assert!(is_valid_transition(
            SnState::Commissioned,
            SnState::Released
        ));
        assert!(!is_valid_transition(
            SnState::Commissioned,
            SnState::Unallocated
        ));

        // Terminal / leaf states have no valid outgoing transitions
        assert!(!is_valid_transition(
            SnState::SnInvalid,
            SnState::Unallocated
        ));
        assert!(!is_valid_transition(
            SnState::Released,
            SnState::Commissioned
        ));
        assert!(!is_valid_transition(SnState::Destroyed, SnState::Inactive));
        assert!(!is_valid_transition(
            SnState::Sampled,
            SnState::Commissioned
        ));
        assert!(!is_valid_transition(
            SnState::LabelSampled,
            SnState::Encoded
        ));
        assert!(!is_valid_transition(
            SnState::LabelScrapped,
            SnState::Encoded
        ));
        assert!(!is_valid_transition(
            SnState::Inactive,
            SnState::Commissioned
        ));
    }
}
