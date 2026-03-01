use serde::{Deserialize, Serialize};

use super::epc::Epc;

/// A node in the aggregation tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationNode {
    pub epc: Epc,
    pub children: Vec<AggregationNode>,
}

/// A flat representation of the full hierarchy from a root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationTree {
    pub root: Epc,
    pub nodes: Vec<AggregationNode>,
}
