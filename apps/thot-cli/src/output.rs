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
    let mut blocks = Vec::new();
    let mut next_block = 0;
    let ordered = yaml_value(value, None, &mut blocks, &mut next_block)?;
    let yaml = render_output_blocks(serde_yaml::to_string(&ordered)?, &blocks);
    io::stdout().write_all(yaml.as_bytes())?;
    io::stdout().flush()?;
    Ok(())
}

fn yaml_value(
    value: &Value,
    key: Option<&str>,
    blocks: &mut Vec<(String, String)>,
    next_block: &mut usize,
) -> Result<serde_yaml::Value, serde_yaml::Error> {
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            let order = result_field_order(object);
            entries.sort_by_key(|(name, _)| {
                order
                    .iter()
                    .position(|preferred| preferred == name)
                    .unwrap_or(order.len())
            });
            let mut mapping = serde_yaml::Mapping::new();
            for (name, child) in entries {
                mapping.insert(
                    serde_yaml::Value::String(name.clone()),
                    yaml_value(child, Some(name), blocks, next_block)?,
                );
            }
            Ok(serde_yaml::Value::Mapping(mapping))
        }
        Value::Array(values) => values
            .iter()
            .map(|child| yaml_value(child, key, blocks, next_block))
            .collect::<Result<Vec<_>, _>>()
            .map(serde_yaml::Value::Sequence),
        Value::String(text) if matches!(key, Some("stdout" | "stderr")) => {
            let text = display_output_text(text);
            if text
                .chars()
                .any(|character| matches!(character, '\n' | '\t' | '\r'))
            {
                let marker = format!("THOT_OUTPUT_BLOCK_{}", *next_block);
                *next_block += 1;
                blocks.push((marker.clone(), text));
                Ok(serde_yaml::Value::String(marker))
            } else {
                Ok(serde_yaml::Value::String(text))
            }
        }
        _ => serde_yaml::to_value(value),
    }
}

fn result_field_order(object: &serde_json::Map<String, Value>) -> &'static [&'static str] {
    if object.contains_key("test_result_id") || object.contains_key("test_reference") {
        &[
            "test_id",
            "test_reference",
            "test_result_id",
            "status",
            "exit_code",
            "start_time",
            "end_time",
            "duration_ms",
            "stdout",
            "stderr",
            "diagnostics",
            "artifacts_dir",
            "record_dir",
            "evidence",
        ]
    } else if object.contains_key("calculation") && object.contains_key("check_results") {
        &[
            "name",
            "description",
            "calculation",
            "checks",
            "result",
            "err",
            "date_time",
            "duration_ms",
            "weights",
            "check_results",
        ]
    } else if object.contains_key("date_time") && object.contains_key("result") {
        &[
            "name",
            "description",
            "result",
            "err",
            "date_time",
            "duration_ms",
            "statistic",
            "sample_count",
            "student_value",
        ]
    } else if object.contains_key("test_id") && object.contains_key("status") {
        &["test_id", "status", "start_time", "end_time", "duration_ms"]
    } else if object.contains_key("name") && object.contains_key("result") {
        &["name", "result", "err"]
    } else {
        &[]
    }
}

fn display_output_text(text: &str) -> String {
    let mut expanded = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('n') => expanded.push('\n'),
                Some('t') => expanded.push('\t'),
                Some('r') => expanded.push('\r'),
                Some(other) => {
                    expanded.push('\\');
                    expanded.push(other);
                }
                None => expanded.push('\\'),
            }
        } else {
            expanded.push(character);
        }
    }
    expanded.replace("\r\n", "\n").replace('\r', "\n")
}

fn render_output_blocks(mut yaml: String, blocks: &[(String, String)]) -> String {
    for (marker, text) in blocks {
        let mut rendered = String::with_capacity(yaml.len() + text.len());
        for line in yaml.split_inclusive('\n') {
            let content = line.strip_suffix('\n').unwrap_or(line);
            let trimmed = content.trim_start_matches(' ');
            let indent = content.len() - trimmed.len();
            let output_key = ["stdout", "stderr"].into_iter().find(|key| {
                trimmed
                    .strip_prefix(&format!("{key}: "))
                    .is_some_and(|value| value == marker)
            });
            if let Some(key) = output_key {
                rendered.push_str(&render_block_scalar(indent, key, text));
            } else {
                rendered.push_str(line);
            }
        }
        yaml = rendered;
    }
    yaml
}

fn render_block_scalar(indent: usize, key: &str, text: &str) -> String {
    let trailing_newlines = text
        .chars()
        .rev()
        .take_while(|character| *character == '\n')
        .count();
    let chomping = match trailing_newlines {
        0 => "-",
        1 => "",
        _ => "+",
    };
    let mut block = format!("{}{key}: |2{chomping}\n", " ".repeat(indent));
    let body = text.trim_end_matches('\n');
    for line in body.split('\n') {
        block.push_str(&" ".repeat(indent + 2));
        block.push_str(line);
        block.push('\n');
    }
    for _ in 1..trailing_newlines {
        block.push('\n');
    }
    block
}
