use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct TestResult {
    pub test_result_id: String,
    pub test_id: String,
    pub run_id: String,
    pub delivery_id: String,
    pub expedient_id: String,
    pub status: TestStatus,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub duration: f64,
    pub diagnostics: String,
    pub observation: HashMap<String, serde_json::Value>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
    TimedOut,
    NotRun,
    Error,
    Cancelled,
}
