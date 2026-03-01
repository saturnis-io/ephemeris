use serde::{Deserialize, Serialize};

/// Electronic Product Code — a URI identifier for a physical object or class.
/// Supports both URN format (urn:epc:id:sgtin:...) and GS1 Digital Link (https://id.gs1.org/...).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Epc(String);

impl Epc {
    pub fn new(uri: impl Into<String>) -> Self {
        Self(uri.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Epc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epc_from_urn() {
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        assert_eq!(epc.as_str(), "urn:epc:id:sgtin:0614141.107346.2017");
    }

    #[test]
    fn test_epc_from_digital_link() {
        let epc = Epc::new("https://id.gs1.org/01/09521568251204/21/10");
        assert_eq!(epc.as_str(), "https://id.gs1.org/01/09521568251204/21/10");
    }

    #[test]
    fn test_epc_equality() {
        let a = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        let b = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        assert_eq!(a, b);
    }

    #[test]
    fn test_epc_display() {
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        assert_eq!(format!("{epc}"), "urn:epc:id:sgtin:0614141.107346.2017");
    }

    #[test]
    fn test_epc_serde_roundtrip() {
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        let json = serde_json::to_string(&epc).unwrap();
        let deserialized: Epc = serde_json::from_str(&json).unwrap();
        assert_eq!(epc, deserialized);
    }
}
