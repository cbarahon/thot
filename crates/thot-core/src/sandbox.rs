use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub project_name: String,
    pub platform: Platform,
    pub limits: Limits,
    pub network_policy: NetworkPolicy,
    pub inputs: Inputs,
    pub tests: Vec<Test>,
    pub evaluation_criteria: Vec<EvaluationCriterion>,
    pub roster: Roster,
    pub assignments: Vec<Assignment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Platform {
    KvmAlpine,
    // Add other platform variants as needed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Limits {
    pub cpu: Option<u32>,
    pub memory: Option<u64>,
    // Add other resource limits as needed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkPolicy {
    pub inbound: Vec<String>,
    pub outbound: Vec<String>,
    // Add other network policies as needed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Inputs {
    pub declarations: Vec<InputDeclaration>,
    // Add other input-related fields as needed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub requires: Vec<String>,
    pub parameters: Vec<Parameter>,
    pub timeout: Option<u64>,
    pub expected_exit_code: Option<u32>,
    pub expected_stdout: Option<String>,
    pub read_only: bool,
    // Add other input declaration fields as needed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Test {
    pub name: String,
    pub test_ref: String,
    pub description: Option<String>,
    pub requirements: Vec<String>,
    pub repeat: usize,
    pub timeout: Option<u64>,
    pub parameters: HashMap<String, String>,
    // Add other test-related fields as needed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvaluationCriterion {
    pub name: String,
    pub description: Option<String>,
    pub checks: Vec<String>,
    pub calculation: String,
    pub parameters: HashMap<String, String>,
    // Add other evaluation criterion fields as needed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Roster {
    pub students: Vec<String>,
    // Add other roster-related fields as needed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Assignment {
    pub student: String,
    pub tasks: Vec<String>,
    // Add other assignment-related fields as needed
}