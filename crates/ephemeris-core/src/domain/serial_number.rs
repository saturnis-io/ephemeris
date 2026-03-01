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
}
