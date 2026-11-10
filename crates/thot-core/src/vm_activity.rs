//! Purpose: Defines project configuration and test declarations loaded from project archives.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Internal marker used to pass labeled JSONPath filters through scalar test parameters.
pub const LABELED_JSONPATH_FILTER_PREFIX: &str = "\0THOT_JSONPATH_LABELS_V1\0";
/// Internal marker used to pass labeled grep filters through scalar test parameters.
pub const LABELED_GREP_FILTER_PREFIX: &str = "\0THOT_GREP_LABELS_V1\0";

/// Project configuration stored as configuration.yaml in a .thot.zip/.thot.tgz archive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VMActivityProject {
    pub activity_name: String,
    pub delivery_folder: String,
    pub concurrency: usize,
    pub timeout: u64,
    pub tests: Vec<ProjectTest>,
    #[serde(default)]
    pub checks: Vec<ProjectCheck>,
    #[serde(default)]
    pub rubrics: Vec<ProjectRubric>,
    #[serde(default)]
    pub evaluation: Vec<ProjectEvaluation>,
    #[serde(default = "default_multiplier")]
    pub rubrics_multiplier: f64,
    #[serde(default = "default_multiplier")]
    pub evaluation_multiplier: f64,
    #[serde(default)]
    pub not_delivered_mark: Option<f64>,
}

fn default_multiplier() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectTest {
    pub name: String,
    #[serde(rename = "test")]
    pub test_ref: String,
    #[serde(default)]
    pub description: Option<String>,
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
    #[serde(default)]
    pub test: String,
    #[serde(default)]
    pub property: Option<String>,
    #[serde(default)]
    pub parameters: ParameterInput,
}

/// A project rubric calculates one numeric result from named check outcomes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectRubric {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "checks", alias = "checks_list")]
    #[serde(default)]
    pub checks: Vec<String>,
    pub calculation: String,
    #[serde(default)]
    pub parameters: ParameterInput,
}

/// A configured evaluation item aggregates named rubric outcomes into a final mark.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectEvaluation {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "rubrics", alias = "rubrics_list")]
    #[serde(default)]
    pub rubrics: Vec<String>,
    pub calculation: String,
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
                Value::String(value)
                    if matches!(key.as_str(), "filter_grep" | "filter_jsonpath")
                        && value.contains('\0') =>
                {
                    return Err(format!("parameter {key:?} must not contain NUL characters"));
                }
                Value::String(value) => value.clone(),
                Value::Number(value) => value.to_string(),
                Value::Bool(value) => value.to_string(),
                Value::Null => String::new(),
                Value::Array(values) if key == "tests" => {
                    let names = values
                        .iter()
                        .map(|value| match value {
                            Value::String(name) if !name.trim().is_empty() => Ok(name.clone()),
                            _ => Err(format!(
                                "parameter {key:?} list entries must be non-empty strings"
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    names.join(",")
                }
                Value::Array(values) if key == "checks" => {
                    let names = values
                        .iter()
                        .map(|value| match value {
                            Value::String(name)
                                if !name.trim().is_empty()
                                    && !name.contains(',')
                                    && !name.contains('\0') =>
                            {
                                Ok(name.clone())
                            }
                            _ => Err(format!(
                                "parameter {key:?} list entries must be non-empty check names without commas"
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    if names.is_empty() {
                        return Err(format!("parameter {key:?} list must not be empty"));
                    }
                    names.join(",")
                }
                Value::Array(values) if key == "weights" => {
                    let weights = values
                        .iter()
                        .map(|value| {
                            value
                                .as_number()
                                .and_then(serde_json::Number::as_f64)
                                .filter(|weight| weight.is_finite())
                                .map(|weight| weight.to_string())
                                .ok_or_else(|| {
                                    "parameter \"weights\" must contain only finite numbers"
                                        .to_owned()
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    weights.join(",")
                }
                Value::Array(values)
                    if matches!(key.as_str(), "filter_grep" | "filter_jsonpath") =>
                {
                    if values.is_empty() {
                        return Err(format!("parameter {key:?} list must not be empty"));
                    }
                    let mut labels = BTreeMap::<String, String>::new();
                    let mut sequence = Vec::with_capacity(values.len());
                    let mut has_raw = false;
                    let mut has_labeled = false;
                    for value in values {
                        match value {
                            Value::String(expression)
                                if !expression.trim().is_empty() && !expression.contains('\0') =>
                            {
                                has_raw = true;
                                sequence.push(Value::String(expression.clone()));
                            }
                            Value::Object(entry) if entry.len() == 1 => {
                                let (label, expression) = entry.iter().next().expect("one entry");
                                if label.trim().is_empty() || label.contains('\0') {
                                    return Err(format!(
                                        "{key} labels must be non-empty strings without NUL characters"
                                    ));
                                }
                                let Value::String(expression) = expression else {
                                    return Err(format!(
                                        "{key} label {label:?} must map to a non-empty expression string"
                                    ));
                                };
                                if expression.trim().is_empty() || expression.contains('\0') {
                                    return Err(format!(
                                        "{key} label {label:?} must map to a non-empty expression string without NUL characters"
                                    ));
                                }
                                if labels.insert(label.clone(), expression.clone()).is_some() {
                                    return Err(format!("{key} label {label:?} is repeated"));
                                }
                                has_labeled = true;
                                sequence.push(value.clone());
                            }
                            Value::Object(_) => {
                                return Err(format!(
                                    "parameter {key:?} labeled list entries must contain exactly one label and expression"
                                ));
                            }
                            _ => {
                                return Err(format!(
                                    "parameter {key:?} list entries must be expressions or one-label mappings"
                                ));
                            }
                        }
                    }
                    if has_labeled && !has_raw {
                        let encoded = serde_json::to_string(&labels)
                            .map_err(|error| format!("cannot encode {key} mapping: {error}"))?;
                        let prefix = if key == "filter_grep" {
                            LABELED_GREP_FILTER_PREFIX
                        } else {
                            LABELED_JSONPATH_FILTER_PREFIX
                        };
                        format!("{prefix}{encoded}")
                    } else if has_labeled {
                        let encoded = serde_json::to_string(&sequence)
                            .map_err(|error| format!("cannot encode {key} filter list: {error}"))?;
                        let prefix = if key == "filter_grep" {
                            LABELED_GREP_FILTER_PREFIX
                        } else {
                            LABELED_JSONPATH_FILTER_PREFIX
                        };
                        format!("{prefix}{encoded}")
                    } else {
                        values
                            .iter()
                            .map(Value::as_str)
                            .collect::<Option<Vec<_>>>()
                            .expect("validated string filters")
                            .join("\0")
                    }
                }
                Value::Object(values)
                    if matches!(key.as_str(), "filter_grep" | "filter_jsonpath") =>
                {
                    if values.is_empty() {
                        return Err(format!("parameter {key:?} mapping must not be empty"));
                    }
                    for (label, value) in values {
                        if label.trim().is_empty() || label.contains('\0') {
                            return Err(format!(
                                "{key} labels must be non-empty strings without NUL characters"
                            ));
                        }
                        match value {
                            Value::String(expression)
                                if !expression.trim().is_empty() && !expression.contains('\0') => {}
                            _ => {
                                return Err(format!(
                                    "{key} label {label:?} must map to a non-empty expression string"
                                ));
                            }
                        }
                    }
                    let encoded = serde_json::to_string(values)
                        .map_err(|error| format!("cannot encode {key} mapping: {error}"))?;
                    let prefix = if key == "filter_grep" {
                        LABELED_GREP_FILTER_PREFIX
                    } else {
                        LABELED_JSONPATH_FILTER_PREFIX
                    };
                    format!("{prefix}{encoded}")
                }
                Value::Object(values) if key == "descriptions" => {
                    if values
                        .iter()
                        .any(|(label, value)| label.trim().is_empty() || !value.is_string())
                    {
                        return Err(
                            "parameter \"descriptions\" must map non-empty column names to strings"
                                .into(),
                        );
                    }
                    serde_json::to_string(values)
                        .map_err(|error| format!("cannot encode descriptions mapping: {error}"))?
                }
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
