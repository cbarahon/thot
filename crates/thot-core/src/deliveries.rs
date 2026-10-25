//! Purpose: Defines shared data types for identifying and tracking student deliveries.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DeliveryIdentity {
    pub delivery_id: String,
    pub delivery_collection_id: String,
    pub original_filename: String,
    pub byte_size: u64,
    pub sha256: String,
    pub source_reference: String,
    pub intake_validation_status: String,
}
