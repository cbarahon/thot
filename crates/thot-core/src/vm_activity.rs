use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct VMActivityProject {
    pub project_id: String,
    pub schema_version: String,
    pub http_endpoint: String,
    pub trial_policy: TrialPolicy,
    pub ssh_username: String,
    pub ssh_secret_ref: String,
    pub limits: Limits,
    pub activity_name: String,
    pub delivery_folder: String,
    pub concurrency: u32,
    pub global_timeout: u32,
    pub tests: Vec<Test>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TrialPolicy {
    pub max_duration: u32,
    pub max_instances: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Limits {
    pub cpu: u32,
    pub memory: u32,
    pub disk_space: u32,
    pub timeout: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Test {
    pub name: String,
    pub test_ref: String,
    pub requirements: Requirements,
    #[serde(default = "default_repeat")]
    pub repeat: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
    #[serde(default)]
    pub string_params: Vec<StringParam>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Requirements {
    // Define requirements here
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StringParam {
    pub key: String,
    pub value: String,
}

fn default_repeat() -> u32 {
    1
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RunCreateRequest {
    pub project_id: String,
    pub instance_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RunStatusResponse {
    pub instance_id: String,
    pub status: String,
    pub start_time: String,
    pub end_time: Option<String>,
}
