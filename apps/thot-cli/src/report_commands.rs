//! Purpose: Fetches report summaries and renders them as terminal tables, CSV, YAML, or JSON.
use crate::{Settings, api_client::ApiClient, output::print_yaml};
use reqwest::Method;
use serde_json::Value;
use std::{
    error::Error,
    io::{self, Write},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Table,
    Csv,
    Yaml,
    Json,
}

#[derive(Clone, Copy)]
enum Report {
    Runs,
    DeliveriesSummary,
    DeliveryTests,
    DeliveryChecks,
}

pub async fn handle(args: &[String], settings: &Settings) -> Result<(), Box<dyn Error>> {
    let (report, run_id, format) = parse_args(args)?;
    let client = ApiClient::new(settings)?;
    let response = match report {
        Report::Runs => {
            client
                .request(Method::GET, "/api/v1/reports/runs", None)
                .await?
        }
        Report::DeliveriesSummary | Report::DeliveryTests | Report::DeliveryChecks => {
            let view = match report {
                Report::DeliveriesSummary => "summary",
                Report::DeliveryTests => "tests",
                Report::DeliveryChecks => "checks",
                Report::Runs => unreachable!(),
            };
            let mut query = vec![format!("view={view}")];
            if let Some(run_id) = run_id.as_deref() {
                query.push(format!("run_id={}", encode_query(run_id)));
            }
            client
                .request(
                    Method::GET,
                    &format!("/api/v1/reports/deliveries?{}", query.join("&")),
                    None,
                )
                .await?
        }
    };
    match format {
        Format::Json => println!("{}", serde_json::to_string_pretty(&response)?),
        Format::Yaml => print_yaml(&response)?,
        Format::Csv => print_csv(report, &response)?,
        Format::Table => print_table(report, &response)?,
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(Report, Option<String>, Format), Box<dyn Error>> {
    let mut positionals = Vec::new();
    let mut format = None;
    for argument in args {
        let selected = match argument.as_str() {
            "--csv" => Some(Format::Csv),
            "--yaml" => Some(Format::Yaml),
            "--json" => Some(Format::Json),
            _ => None,
        };
        if let Some(selected) = selected {
            if format.replace(selected).is_some() {
                return Err(
                    "choose only one report output format: --csv, --yaml, or --json".into(),
                );
            }
        } else {
            positionals.push(argument.as_str());
        }
    }
    let parsed = match positionals.as_slice() {
        ["runs"] => (Report::Runs, None),
        ["deliveries", "tests"] => (Report::DeliveryTests, None),
        ["deliveries", "tests", run_id] => (Report::DeliveryTests, Some((*run_id).to_owned())),
        ["deliveries", "checks"] => (Report::DeliveryChecks, None),
        ["deliveries", "checks", run_id] => (Report::DeliveryChecks, Some((*run_id).to_owned())),
        ["deliveries"] => (Report::DeliveriesSummary, None),
        ["deliveries", run_id] => (Report::DeliveriesSummary, Some((*run_id).to_owned())),
        _ => {
            return Err("usage: thot report runs | deliveries [RUN_ID] | deliveries tests [RUN_ID] | deliveries checks [RUN_ID] [--csv|--yaml|--json]".into());
        }
    };
    Ok((parsed.0, parsed.1, format.unwrap_or(Format::Table)))
}

fn print_csv(report: Report, response: &Value) -> Result<(), Box<dyn Error>> {
    let (headers, rows) = tabular_data(report, response);
    let mut output = Vec::new();
    write_csv_record(&mut output, &headers)?;
    for row in rows {
        write_csv_record(&mut output, &row)?;
    }
    io::stdout().write_all(&output)?;
    Ok(())
}

fn print_table(report: Report, response: &Value) -> Result<(), Box<dyn Error>> {
    let (headers, rows) = tabular_data(report, response);
    let terminal_width = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width >= 40)
        .unwrap_or(120);
    let mut widths = headers
        .iter()
        .enumerate()
        .map(|(column, header)| {
            let content = rows
                .iter()
                .filter_map(|row| row.get(column))
                .map(|value| display_width(value))
                .max()
                .unwrap_or(0);
            let cap = if column == 0 {
                32
            } else if header.len() > 16 {
                18
            } else {
                16
            };
            display_width(header).max(content).min(cap)
        })
        .collect::<Vec<_>>();
    let mut total = table_width(&widths);
    while total > terminal_width {
        let candidate = widths
            .iter()
            .enumerate()
            .filter(|(_, width)| **width > 6)
            .max_by_key(|(_, width)| **width)
            .map(|(index, _)| index);
        let Some(index) = candidate else { break };
        widths[index] -= 1;
        total = table_width(&widths);
    }

    let mut output = io::BufWriter::new(io::stdout().lock());
    write_border(&mut output, &widths)?;
    write_table_row(&mut output, &headers, &widths)?;
    write_border(&mut output, &widths)?;
    for row in &rows {
        write_table_row(&mut output, row, &widths)?;
    }
    write_border(&mut output, &widths)?;
    output.flush()?;
    Ok(())
}

fn tabular_data(report: Report, response: &Value) -> (Vec<String>, Vec<Vec<String>>) {
    match report {
        Report::Runs => {
            let headers = [
                "Run",
                "Status",
                "Started",
                "Finished",
                "Duration",
                "Tests pending",
                "Tests running",
                "Tests finished",
                "Finished OK",
                "Finished errors",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            let rows = response["runs"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|run| {
                    vec![
                        string_or_empty(&run["run_id"]),
                        string_or_empty(&run["status"]),
                        string_or_empty(&run["started_at"]),
                        string_or_empty(&run["finished_at"]),
                        human_duration(&run["duration_ms"]),
                        string_or_empty(&run["tests_pending"]),
                        string_or_empty(&run["tests_running"]),
                        string_or_empty(&run["tests_finished"]),
                        string_or_empty(&run["tests_finished_without_errors"]),
                        string_or_empty(&run["tests_finished_with_errors"]),
                    ]
                })
                .collect();
            (headers, rows)
        }
        Report::DeliveriesSummary => {
            let headers = [
                "Delivery",
                "Students",
                "Tests pass",
                "Tests failed",
                "Checks pass",
                "Checks failed",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            let rows = delivery_rows(response)
                .map(|delivery| {
                    vec![
                        string_or_empty(&delivery["filename"]),
                        string_or_empty(&delivery["student_count"]),
                        string_or_empty(&delivery["tests_passed"]),
                        string_or_empty(&delivery["tests_failed"]),
                        string_or_empty(&delivery["checks_passed"]),
                        string_or_empty(&delivery["checks_failed"]),
                    ]
                })
                .collect();
            (headers, rows)
        }
        Report::DeliveryTests | Report::DeliveryChecks => {
            let (key, columns_key) = if matches!(report, Report::DeliveryTests) {
                ("tests", "test_names")
            } else {
                ("checks", "check_names")
            };
            let columns = response[columns_key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut headers = vec!["Delivery".into(), "Students".into()];
            headers.extend(columns.iter().cloned());
            let rows = delivery_rows(response)
                .map(|delivery| {
                    let mut row = vec![
                        string_or_empty(&delivery["filename"]),
                        string_or_empty(&delivery["student_count"]),
                    ];
                    for column in &columns {
                        row.push(string_or_empty(&delivery[key][column]));
                    }
                    row
                })
                .collect();
            (headers, rows)
        }
    }
}

fn delivery_rows(response: &Value) -> impl Iterator<Item = &Value> {
    response["deliveries"].as_array().into_iter().flatten()
}

fn string_or_empty(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

fn human_duration(value: &Value) -> String {
    let Some(milliseconds) = value.as_u64() else {
        return String::new();
    };
    let seconds = milliseconds / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60)
    }
}

fn write_csv_record(output: &mut impl Write, fields: &[String]) -> io::Result<()> {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            output.write_all(b",")?;
        }
        if field.contains([',', '"', '\n', '\r']) {
            output.write_all(b"\"")?;
            output.write_all(field.replace('"', "\"\"").as_bytes())?;
            output.write_all(b"\"")?;
        } else {
            output.write_all(field.as_bytes())?;
        }
    }
    output.write_all(b"\r\n")
}

fn table_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + widths.len() * 3 + 1
}

fn write_border(output: &mut impl Write, widths: &[usize]) -> io::Result<()> {
    output.write_all(b"+")?;
    for width in widths {
        for _ in 0..width + 2 {
            output.write_all(b"-")?;
        }
        output.write_all(b"+")?;
    }
    output.write_all(b"\n")
}

fn write_table_row(output: &mut impl Write, row: &[String], widths: &[usize]) -> io::Result<()> {
    output.write_all(b"|")?;
    for (index, width) in widths.iter().enumerate() {
        let value = row.get(index).map(String::as_str).unwrap_or("");
        let value = value.replace(['\n', '\r', '\t'], " ");
        let value = truncate(&value, *width);
        write!(
            output,
            " {value}{} |",
            " ".repeat(width.saturating_sub(display_width(&value)))
        )?;
    }
    output.write_all(b"\n")
}

fn truncate(value: &str, width: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(width).collect::<String>();
    if chars.next().is_some() && width > 0 {
        let mut truncated = prefix
            .chars()
            .take(width.saturating_sub(1))
            .collect::<String>();
        truncated.push('…');
        truncated
    } else {
        prefix
    }
}

fn display_width(value: &str) -> usize {
    value.chars().count()
}

fn encode_query(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}
