//! Purpose: Defines project configuration and test declarations loaded from project archives.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Project configuration stored as configuration.yaml in a .thot.zip/.thot.tgz archive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VMActivityProject {
    pub activity_name: String,
    pub delivery_folder: String,
    pub concurrency: usize,
    pub timeout: u64,
    pub tests: Vec<ProjectTest>,
    #[serde(default)]
    pub checks: Vec<ProjectCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectTest {
    pub name: String,
    #[serde(rename = "test")]
    pub test_ref: String,
    #[serde(default)]
    pub requirements: Vec<String>,
    #[serde(default = "default_repeat")]
    pub repeat: usize,
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub parameters: ParameterInput,
}

/// A configured check evaluated against a named test result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectCheck {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "check")]
    pub check_ref: String,
    pub test: String,
    #[serde(default)]
    pub parameters: ParameterInput,
}

/// The example format permits either a mapping or a sequence of one-key mappings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ParameterInput {
    Mapping(BTreeMap<String, Value>),
    Entries(Vec<BTreeMap<String, Value>>),
}

impl Default for ParameterInput {
    fn default() -> Self {
        Self::Mapping(BTreeMap::new())
    }
}

impl ParameterInput {
    pub fn into_strings(&self) -> Result<BTreeMap<String, String>, String> {
        let entries: Vec<(&String, &Value)> = match self {
            Self::Mapping(values) => values.iter().collect(),
            Self::Entries(groups) => groups.iter().flat_map(|group| group.iter()).collect(),
        };
        let mut result = BTreeMap::new();
        for (key, value) in entries {
            let text = match value {
                Value::String(value) => value.clone(),
                Value::Number(value) => value.to_string(),
                Value::Bool(value) => value.to_string(),
                Value::Null => String::new(),
                Value::Array(_) | Value::Object(_) => {
                    return Err(format!("parameter {key:?} must be a scalar"));
                }
            };
            if result.insert(key.clone(), text).is_some() {
                return Err(format!("parameter {key:?} is repeated"));
            }
        }
        Ok(result)
    }
}

fn default_repeat() -> usize {
    1
}

pub type VMActivity = VMActivityProject;
