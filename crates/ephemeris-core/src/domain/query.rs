use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

/// Query parameters for filtering events (subset of EPCIS 2.0 Query Interface).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ge_event_time: Option<DateTime<FixedOffset>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lt_event_time: Option<DateTime<FixedOffset>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eq_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eq_biz_step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_epc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_parent_id: Option<String>,
    /// Max results to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_page: Option<u32>,
    /// Pagination token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}
