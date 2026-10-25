//! Purpose: Defines normalized test statuses and result data shared by runners and reports.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
    TimedOut,
    NotRun,
    Error,
    Cancelled,
}
