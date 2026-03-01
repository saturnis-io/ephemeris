use chrono::{FixedOffset, Utc};
use ephemeris_core::domain::{
    Action, AggregationEventData, CommonEventFields, EpcisEvent, LocationRef, ObjectEventData,
};
use rand::Rng;
use uuid::Uuid;

const BIZ_STEPS: &[&str] = &[
    "urn:epcglobal:cbv:bizstep:commissioning",
    "urn:epcglobal:cbv:bizstep:packing",
    "urn:epcglobal:cbv:bizstep:shipping",
    "urn:epcglobal:cbv:bizstep:receiving",
    "urn:epcglobal:cbv:bizstep:storing",
    "urn:epcglobal:cbv:bizstep:picking",
];

const DISPOSITIONS: &[&str] = &[
    "urn:epcglobal:cbv:disp:active",
    "urn:epcglobal:cbv:disp:in_transit",
    "urn:epcglobal:cbv:disp:in_progress",
    "urn:epcglobal:cbv:disp:encoded",
];

const COMPANY_PREFIXES: &[&str] = &["0614141", "4012345", "0313131", "0711711"];

const LOCATION_IDS: &[&str] = &[
    "urn:epc:id:sgln:0614141.07346.1234",
    "urn:epc:id:sgln:0614141.07346.5678",
    "urn:epc:id:sgln:4012345.00001.0",
    "urn:epc:id:sgln:0313131.12345.0",
];

/// Generates random but structurally valid EPCIS events for testing.
pub struct EventGenerator {
    rng: rand::rngs::ThreadRng,
}

impl Default for EventGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl EventGenerator {
    pub fn new() -> Self {
        Self { rng: rand::rng() }
    }

    /// Generate a random ObjectEvent with realistic field values.
    pub fn object_event(&mut self) -> EpcisEvent {
        let epc_count = self.rng.random_range(1..=5);
        let prefix = self.pick(COMPANY_PREFIXES);
        let item_ref = format!("{:06}", self.rng.random_range(100000..999999u32));

        let epc_list: Vec<String> = (0..epc_count)
            .map(|_| {
                let serial = self.rng.random_range(1000..9999u32);
                format!("urn:epc:id:sgtin:{prefix}.{item_ref}.{serial}")
            })
            .collect();

        let actions = [Action::Observe, Action::Add];
        let action = actions[self.rng.random_range(0..actions.len())].clone();

        EpcisEvent::ObjectEvent(ObjectEventData {
            common: self.make_common(),
            action,
            epc_list,
            quantity_list: vec![],
        })
    }

    /// Generate a random AggregationEvent with a parent and child EPCs.
    pub fn aggregation_event(&mut self) -> EpcisEvent {
        let prefix = self.pick(COMPANY_PREFIXES);
        let sscc_serial = format!(
            "{:010}",
            self.rng.random_range(1000000000u64..9999999999u64)
        );
        let parent_id = format!("urn:epc:id:sscc:{prefix}.{sscc_serial}");

        let child_count = self.rng.random_range(2..=6);
        let item_ref = format!("{:06}", self.rng.random_range(100000..999999u32));
        let child_epcs: Vec<String> = (0..child_count)
            .map(|_| {
                let serial = self.rng.random_range(1000..9999u32);
                format!("urn:epc:id:sgtin:{prefix}.{item_ref}.{serial}")
            })
            .collect();

        let actions = [Action::Add, Action::Delete, Action::Observe];
        let action = actions[self.rng.random_range(0..actions.len())].clone();

        EpcisEvent::AggregationEvent(AggregationEventData {
            common: self.make_common(),
            action,
            parent_id: Some(parent_id),
            child_epcs,
            child_quantity_list: vec![],
        })
    }

    /// Generate a random event of any type.
    pub fn random_event(&mut self) -> EpcisEvent {
        if self.rng.random_bool(0.5) {
            self.object_event()
        } else {
            self.aggregation_event()
        }
    }

    /// Generate a batch of random events.
    pub fn batch(&mut self, count: usize) -> Vec<EpcisEvent> {
        (0..count).map(|_| self.random_event()).collect()
    }

    fn make_common(&mut self) -> CommonEventFields {
        let offset = FixedOffset::east_opt(0).expect("valid offset");
        let now = Utc::now().with_timezone(&offset);

        CommonEventFields {
            event_id: Some(Uuid::new_v4().to_string()),
            event_time: now,
            event_time_zone_offset: "+00:00".to_string(),
            record_time: None,
            biz_step: Some(self.pick(BIZ_STEPS).to_string()),
            disposition: Some(self.pick(DISPOSITIONS).to_string()),
            read_point: Some(LocationRef {
                id: self.pick(LOCATION_IDS).to_string(),
            }),
            biz_location: Some(LocationRef {
                id: self.pick(LOCATION_IDS).to_string(),
            }),
            biz_transaction_list: vec![],
            source_list: vec![],
            destination_list: vec![],
        }
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.rng.random_range(0..items.len())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_object_event() {
        let mut generator = EventGenerator::new();
        let event = generator.object_event();
        let json = serde_json::to_string(&event).unwrap();
        let roundtrip: EpcisEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(roundtrip, EpcisEvent::ObjectEvent(_)));
    }

    #[test]
    fn generates_valid_aggregation_event() {
        let mut generator = EventGenerator::new();
        let event = generator.aggregation_event();
        let json = serde_json::to_string(&event).unwrap();
        let roundtrip: EpcisEvent = serde_json::from_str(&json).unwrap();
        match roundtrip {
            EpcisEvent::AggregationEvent(data) => {
                assert!(data.parent_id.is_some());
                assert!(!data.child_epcs.is_empty());
            }
            _ => panic!("Expected AggregationEvent"),
        }
    }

    #[test]
    fn batch_generates_correct_count() {
        let mut generator = EventGenerator::new();
        let events = generator.batch(10);
        assert_eq!(events.len(), 10);
    }

    #[test]
    fn fixture_object_event_deserializes() {
        let json = include_str!("../../../tests/fixtures/object_event.json");
        let event: EpcisEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, EpcisEvent::ObjectEvent(_)));
    }

    #[test]
    fn fixture_aggregation_event_deserializes() {
        let json = include_str!("../../../tests/fixtures/aggregation_event.json");
        let event: EpcisEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, EpcisEvent::AggregationEvent(_)));
    }
}
