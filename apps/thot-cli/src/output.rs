//! Purpose: Renders JSON API values as human-readable YAML for CLI output.

use serde_json::Value;
use serde_yaml;
use std::io::{self, Write};

/// Prints a JSON value as a YAML document to stdout.
///
/// # Arguments
///
/// * `value` - A reference to the JSON value to be printed.
///
/// # Returns
///
/// * `Ok(())` if the YAML document is successfully printed.
/// * `Err(Box<dyn std::error::Error>)` if there is an error during serialization or I/O.
pub fn print_yaml(value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let yaml = serde_yaml::to_string(value)?;
    io::stdout().write_all(yaml.as_bytes())?;
    io::stdout().flush()?;
    Ok(())
}
