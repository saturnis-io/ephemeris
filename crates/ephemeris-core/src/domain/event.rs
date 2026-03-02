use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::epc::Epc;

/// Unique identifier for a stored event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

impl EventId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

/// EPCIS 2.0 action types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Action {
    Observe,
    Add,
    Delete,
}

/// Quantity with optional unit of measure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuantityElement {
    pub epc_class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uom: Option<String>,
}

/// Business transaction reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BizTransaction {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub tx_type: Option<String>,
    pub biz_transaction: String,
}

/// Location reference (readPoint or bizLocation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationRef {
    pub id: String,
}

/// Source or destination party/location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDest {
    #[serde(rename = "type")]
    pub sd_type: String,
    #[serde(alias = "source", alias = "destination")]
    pub identifier: String,
}

/// Common fields shared by all event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonEventFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    pub event_time: DateTime<FixedOffset>,
    pub event_time_zone_offset: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_time: Option<DateTime<FixedOffset>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub biz_step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disposition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_point: Option<LocationRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub biz_location: Option<LocationRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub biz_transaction_list: Vec<BizTransaction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_list: Vec<SourceDest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destination_list: Vec<SourceDest>,
}

/// The top-level EPCIS event enum.
/// Each variant holds the event-type-specific fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EpcisEvent {
    ObjectEvent(ObjectEventData),
    AggregationEvent(AggregationEventData),
    TransformationEvent(TransformationEventData),
}

impl EpcisEvent {
    /// Access the common fields shared by all event types.
    pub fn common(&self) -> &CommonEventFields {
        match self {
            Self::ObjectEvent(data) => &data.common,
            Self::AggregationEvent(data) => &data.common,
            Self::TransformationEvent(data) => &data.common,
        }
    }
}

/// Object event data — tracks EPCs with an action at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectEventData {
    #[serde(flatten)]
    pub common: CommonEventFields,
    pub action: Action,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub epc_list: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quantity_list: Vec<QuantityElement>,
}

/// Aggregation event data — tracks parent-child relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregationEventData {
    #[serde(flatten)]
    pub common: CommonEventFields,
    pub action: Action,
    #[serde(rename = "parentID", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, rename = "childEPCs", skip_serializing_if = "Vec::is_empty")]
    pub child_epcs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_quantity_list: Vec<QuantityElement>,
}

/// Transformation event data — tracks input-to-output transformations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformationEventData {
    #[serde(flatten)]
    pub common: CommonEventFields,
    #[serde(
        default,
        rename = "inputEPCList",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub input_epc_list: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_quantity_list: Vec<QuantityElement>,
    #[serde(
        default,
        rename = "outputEPCList",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub output_epc_list: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_quantity_list: Vec<QuantityElement>,
    #[serde(rename = "transformationID", skip_serializing_if = "Option::is_none")]
    pub transformation_id: Option<String>,
}

// Re-export Epc so event module users don't need a separate import
#[allow(unused_imports)]
use Epc as _;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_event_roundtrip() {
        let json = r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2005-04-03T20:33:31.116-06:00",
            "eventTimeZoneOffset": "-06:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2017"],
            "bizStep": "shipping",
            "readPoint": {"id": "urn:epc:id:sgln:0614141.07346.1234"}
        }"#;

        let event: EpcisEvent = serde_json::from_str(json).unwrap();
        match &event {
            EpcisEvent::ObjectEvent(data) => {
                assert_eq!(data.action, Action::Observe);
                assert_eq!(data.epc_list.len(), 1);
                assert_eq!(data.common.biz_step.as_deref(), Some("shipping"));
            }
            _ => panic!("Expected ObjectEvent"),
        }

        // Roundtrip
        let serialized = serde_json::to_string(&event).unwrap();
        let _: EpcisEvent = serde_json::from_str(&serialized).unwrap();
    }

    #[test]
    fn test_aggregation_event_roundtrip() {
        let json = r#"{
            "type": "AggregationEvent",
            "action": "ADD",
            "eventTime": "2013-06-08T14:58:56.591+02:00",
            "eventTimeZoneOffset": "+02:00",
            "parentID": "urn:epc:id:sscc:0614141.1234567890",
            "childEPCs": [
                "urn:epc:id:sgtin:0614141.107346.2017",
                "urn:epc:id:sgtin:0614141.107346.2018"
            ],
            "bizStep": "packing"
        }"#;

        let event: EpcisEvent = serde_json::from_str(json).unwrap();
        match &event {
            EpcisEvent::AggregationEvent(data) => {
                assert_eq!(data.action, Action::Add);
                assert_eq!(
                    data.parent_id.as_deref(),
                    Some("urn:epc:id:sscc:0614141.1234567890")
                );
                assert_eq!(data.child_epcs.len(), 2);
            }
            _ => panic!("Expected AggregationEvent"),
        }

        // Roundtrip
        let serialized = serde_json::to_string(&event).unwrap();
        let _: EpcisEvent = serde_json::from_str(&serialized).unwrap();
    }

    #[test]
    fn test_transformation_event_roundtrip() {
        let json = r#"{
            "type": "TransformationEvent",
            "eventTime": "2020-01-15T10:00:00.000+00:00",
            "eventTimeZoneOffset": "+00:00",
            "inputEPCList": ["urn:epc:id:sgtin:4012345.011111.987"],
            "outputEPCList": ["urn:epc:id:sgtin:4012345.022222.123"],
            "transformationID": "urn:epc:id:gdti:4012345.55555.1234"
        }"#;

        let event: EpcisEvent = serde_json::from_str(json).unwrap();
        match &event {
            EpcisEvent::TransformationEvent(data) => {
                assert_eq!(data.input_epc_list.len(), 1);
                assert_eq!(data.output_epc_list.len(), 1);
                assert_eq!(
                    data.transformation_id.as_deref(),
                    Some("urn:epc:id:gdti:4012345.55555.1234")
                );
            }
            _ => panic!("Expected TransformationEvent"),
        }

        // Roundtrip
        let serialized = serde_json::to_string(&event).unwrap();
        let _: EpcisEvent = serde_json::from_str(&serialized).unwrap();
    }

    #[test]
    fn test_action_serialization() {
        assert_eq!(
            serde_json::to_string(&Action::Observe).unwrap(),
            "\"OBSERVE\""
        );
        assert_eq!(serde_json::to_string(&Action::Add).unwrap(), "\"ADD\"");
        assert_eq!(
            serde_json::to_string(&Action::Delete).unwrap(),
            "\"DELETE\""
        );
    }

    #[test]
    fn test_event_id_generates_unique() {
        let a = EventId::new();
        let b = EventId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn test_object_event_with_quantity() {
        let json = r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2020-03-15T00:00:00.000+00:00",
            "eventTimeZoneOffset": "+00:00",
            "quantityList": [
                {
                    "epcClass": "urn:epc:class:lgtin:4012345.012345.998877",
                    "quantity": 200.5,
                    "uom": "KGM"
                }
            ]
        }"#;

        let event: EpcisEvent = serde_json::from_str(json).unwrap();
        match &event {
            EpcisEvent::ObjectEvent(data) => {
                assert_eq!(data.quantity_list.len(), 1);
                assert_eq!(
                    data.quantity_list[0].epc_class,
                    "urn:epc:class:lgtin:4012345.012345.998877"
                );
                assert_eq!(data.quantity_list[0].quantity, Some(200.5));
                assert_eq!(data.quantity_list[0].uom.as_deref(), Some("KGM"));
            }
            _ => panic!("Expected ObjectEvent"),
        }
    }

    #[test]
    fn test_object_event_with_biz_transactions() {
        let json = r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2020-03-15T00:00:00.000+00:00",
            "eventTimeZoneOffset": "+00:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2017"],
            "bizTransactionList": [
                {
                    "type": "urn:epcglobal:cbv:btt:po",
                    "bizTransaction": "http://transaction.example.com/po/12345678"
                }
            ]
        }"#;

        let event: EpcisEvent = serde_json::from_str(json).unwrap();
        match &event {
            EpcisEvent::ObjectEvent(data) => {
                assert_eq!(data.common.biz_transaction_list.len(), 1);
                assert_eq!(
                    data.common.biz_transaction_list[0].tx_type.as_deref(),
                    Some("urn:epcglobal:cbv:btt:po")
                );
            }
            _ => panic!("Expected ObjectEvent"),
        }
    }
}
