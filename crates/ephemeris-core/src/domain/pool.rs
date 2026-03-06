use std::fmt;

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::epc::Epc;

/// Unique identifier for a serial number pool.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PoolId(pub Uuid);

impl PoolId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for PoolId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for PoolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// OPEN-SCS pool selection criterion keys.
///
/// Used to match serial numbers to pools based on product, location,
/// and order attributes. The `Custom` variant allows site-specific extensions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolCriterionKey {
    Gtin,
    SsccGcp,
    SsccExtension,
    CountryCode,
    Location,
    Sublocation,
    LotNumber,
    /// Direct pool reference. Note: shares name with the `PoolId` struct —
    /// always qualify as `PoolCriterionKey::PoolId` to avoid ambiguity.
    PoolId,
    SidClassId,
    OrderId,
    Custom(String),
}

/// A set of key-value criteria used to select a pool or filter serial numbers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolSelectionCriteria {
    pub criteria: Vec<(PoolCriterionKey, String)>,
}

/// A serial number pool — a named container for managing SN allocation lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialNumberPool {
    pub id: PoolId,
    pub name: String,
    pub sid_class: Option<String>,
    pub criteria: PoolSelectionCriteria,
    pub esm_endpoint: Option<String>,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
}

/// Request to allocate serial numbers from a pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolRequest {
    pub count: u32,
    pub criteria: PoolSelectionCriteria,
    pub output_format: Option<String>,
}

/// Response containing allocated serial numbers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolResponse {
    pub serial_numbers: Vec<Epc>,
    pub pool_id: PoolId,
    pub fulfilled: u32,
    pub requested: u32,
}

/// Request to return serial numbers back to a pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolReturnRequest {
    pub serial_numbers: Vec<Epc>,
}

/// Request to receive serial numbers into a pool (e.g., from an ESM).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolReceiveRequest {
    pub serial_numbers: Vec<Epc>,
    pub sid_class: Option<String>,
    pub initial_state: Option<String>,
}

/// Aggregated statistics for a serial number pool, broken down by SN state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStats {
    pub pool_id: PoolId,
    pub total: u64,
    pub unassigned: u64,
    pub unallocated: u64,
    pub allocated: u64,
    pub encoded: u64,
    pub commissioned: u64,
    pub other: u64,
}

/// Query parameters for searching pools.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolQuery {
    pub sid_class: Option<String>,
    pub name_contains: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_id_display_and_new() {
        let id = PoolId::new();
        let display = format!("{id}");
        // UUID v4 display is 36 chars: 8-4-4-4-12
        assert_eq!(display.len(), 36);

        let cloned = id.clone();
        assert_eq!(id, cloned);
    }

    #[test]
    fn test_pool_criterion_key_serde_roundtrip() {
        let keys = vec![
            PoolCriterionKey::Gtin,
            PoolCriterionKey::SsccGcp,
            PoolCriterionKey::SsccExtension,
            PoolCriterionKey::CountryCode,
            PoolCriterionKey::Location,
            PoolCriterionKey::Sublocation,
            PoolCriterionKey::LotNumber,
            PoolCriterionKey::PoolId,
            PoolCriterionKey::SidClassId,
            PoolCriterionKey::OrderId,
            PoolCriterionKey::Custom("my_custom_key".to_string()),
        ];

        for key in keys {
            let json = serde_json::to_string(&key).unwrap();
            let back: PoolCriterionKey = serde_json::from_str(&json).unwrap();
            assert_eq!(back, key, "roundtrip failed for {key:?}");
        }
    }

    #[test]
    fn test_pool_criterion_key_snake_case_serialization() {
        assert_eq!(
            serde_json::to_string(&PoolCriterionKey::SsccGcp).unwrap(),
            r#""sscc_gcp""#
        );
        assert_eq!(
            serde_json::to_string(&PoolCriterionKey::CountryCode).unwrap(),
            r#""country_code""#
        );
        assert_eq!(
            serde_json::to_string(&PoolCriterionKey::SidClassId).unwrap(),
            r#""sid_class_id""#
        );
    }

    #[test]
    fn test_pool_selection_criteria_default_is_empty() {
        let criteria = PoolSelectionCriteria::default();
        assert!(criteria.criteria.is_empty());
    }

    #[test]
    fn test_serial_number_pool_serde_roundtrip() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-03-06T12:00:00+00:00").unwrap();
        let pool = SerialNumberPool {
            id: PoolId::new(),
            name: "Test Pool".to_string(),
            sid_class: Some("sgtin".to_string()),
            criteria: PoolSelectionCriteria {
                criteria: vec![(PoolCriterionKey::Gtin, "09521568251204".to_string())],
            },
            esm_endpoint: Some("https://esm.example.com/api".to_string()),
            created_at: now,
            updated_at: now,
        };

        let json = serde_json::to_string(&pool).unwrap();
        let back: SerialNumberPool = serde_json::from_str(&json).unwrap();

        assert_eq!(back.id, pool.id);
        assert_eq!(back.name, "Test Pool");
        assert_eq!(back.sid_class.as_deref(), Some("sgtin"));
        assert_eq!(back.criteria.criteria.len(), 1);
        assert_eq!(
            back.esm_endpoint.as_deref(),
            Some("https://esm.example.com/api")
        );
        assert_eq!(back.created_at, now);
        assert_eq!(back.updated_at, now);
    }

    #[test]
    fn test_pool_request_serde() {
        let json = r#"{
            "count": 500,
            "criteria": {
                "criteria": [["gtin", "09521568251204"]]
            },
            "output_format": "urn"
        }"#;

        let req: PoolRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.count, 500);
        assert_eq!(req.criteria.criteria.len(), 1);
        assert_eq!(req.output_format.as_deref(), Some("urn"));
    }

    #[test]
    fn test_pool_stats_default_zeros() {
        let stats = PoolStats {
            pool_id: PoolId::new(),
            total: 0,
            unassigned: 0,
            unallocated: 0,
            allocated: 0,
            encoded: 0,
            commissioned: 0,
            other: 0,
        };

        assert_eq!(stats.total, 0);
        assert_eq!(stats.unassigned, 0);
        assert_eq!(stats.unallocated, 0);
        assert_eq!(stats.allocated, 0);
        assert_eq!(stats.encoded, 0);
        assert_eq!(stats.commissioned, 0);
        assert_eq!(stats.other, 0);
    }

    #[test]
    fn test_pool_query_default() {
        let query = PoolQuery::default();
        assert!(query.sid_class.is_none());
        assert!(query.name_contains.is_none());
        assert!(query.limit.is_none());
        assert!(query.offset.is_none());
    }
}
