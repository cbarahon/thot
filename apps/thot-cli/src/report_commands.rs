//! Purpose: Fetches report summaries and renders them as terminal tables, CSV, YAML, or JSON.
use crate::{Settings, api_client::ApiClient, output::print_yaml};
use reqwest::Method;
use serde_json::Value;
use std::{
    error::Error,
    io::{self, IsTerminal, Write},
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
    let (headers, mut rows) = tabular_data(report, response);
    compact_table_rows(report, &mut rows);
    let headers = console_headers(report, &headers);
    let widths = allocate_widths(report, &headers, &rows, terminal_width());
    let wrapped_headers = headers
        .iter()
        .zip(&widths)
        .map(|(header, width)| wrap_header(header, *width))
        .collect::<Vec<_>>();
    let header_height = wrapped_headers.iter().map(Vec::len).max().unwrap_or(1);

    let mut output = io::BufWriter::new(io::stdout().lock());
    write_border(&mut output, &widths)?;
    for line in 0..header_height {
        let header_row = wrapped_headers
            .iter()
            .map(|parts| parts.get(line).cloned().unwrap_or_default())
            .collect::<Vec<_>>();
        write_table_row(&mut output, &header_row, &widths)?;
    }
    write_border(&mut output, &widths)?;
    for row in &rows {
        write_table_row(&mut output, row, &widths)?;
    }
    write_border(&mut output, &widths)?;
    output.flush()?;
    Ok(())
}

fn terminal_width() -> usize {
    #[cfg(target_os = "linux")]
    if io::stdout().is_terminal() {
        let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
        // SAFETY: `size` points to writable storage for a winsize struct, and ioctl only
        // fills that struct when the file descriptor refers to a terminal.
        let result =
            unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) };
        if result == 0 {
            // SAFETY: ioctl returned success and initialized the winsize structure.
            let columns = unsafe { size.assume_init() }.ws_col as usize;
            if columns >= 40 {
                return columns;
            }
        }
    }
    if let Some(width) = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width >= 40)
    {
        return width;
    }
    120
}

fn allocate_widths(
    report: Report,
    headers: &[String],
    rows: &[Vec<String>],
    terminal_width: usize,
) -> Vec<usize> {
    let desired = headers
        .iter()
        .enumerate()
        .map(|(column, header)| {
            let content_width = rows
                .iter()
                .filter_map(|row| row.get(column))
                .map(|value| display_width(value))
                .max()
                .unwrap_or(0);
            let header_width = if matches!(report, Report::DeliveryTests | Report::DeliveryChecks)
                && column >= 2
            {
                two_line_header_width(header).min(12)
            } else {
                header
                    .split(['\n', '_', '-', ' '])
                    .map(display_width)
                    .max()
                    .unwrap_or(1)
            };
            let default_cap = if column == 0 { 48 } else { 24 };
            content_width.max(header_width).min(default_cap).max(1)
        })
        .collect::<Vec<_>>();
    let mut widths = desired.clone();
    let mut available = terminal_width.saturating_sub(widths.len() * 3 + 1);
    if widths.iter().sum::<usize>() <= available {
        return widths;
    }

    let minimums = headers
        .iter()
        .enumerate()
        .map(|(column, _)| match report {
            Report::Runs => match column {
                0 => 10,
                1 => 8,
                2 | 3 => 12,
                4 => 6,
                _ => 5,
            },
            Report::DeliveriesSummary => match column {
                0 => 10,
                1 => 3,
                _ => 5,
            },
            Report::DeliveryTests | Report::DeliveryChecks => match column {
                0 => 10,
                1 => 3,
                _ => 5,
            },
        })
        .collect::<Vec<_>>();
    widths.clone_from(&minimums);
    let minimum_total = widths.iter().sum::<usize>();
    if minimum_total > available {
        // A report with more test columns than fit at their minimum still keeps one column
        // per test; its table may exceed the terminal width instead of hiding results.
        return widths;
    }
    available -= minimum_total;
    while available > 0 {
        let candidate = widths
            .iter()
            .enumerate()
            .filter(|(index, width)| **width < desired[*index])
            .max_by(|(left_index, left_width), (right_index, right_width)| {
                let left_need = (desired[*left_index] - **left_width) as f64;
                let right_need = (desired[*right_index] - **right_width) as f64;
                let left_weight = if *left_index == 0 { 2.5 } else { 1.0 };
                let right_weight = if *right_index == 0 { 2.5 } else { 1.0 };
                (left_need * left_weight / desired[*left_index] as f64)
                    .total_cmp(&(right_need * right_weight / desired[*right_index] as f64))
            })
            .map(|(index, _)| index);
        let Some(index) = candidate else { break };
        widths[index] += 1;
        available -= 1;
    }
    widths
}

fn console_headers(report: Report, headers: &[String]) -> Vec<String> {
    match report {
        Report::Runs => [
            "Run",
            "Status",
            "Start\nUTC",
            "Finish\nUTC",
            "Duration",
            "Tests\npending",
            "Tests\nrunning",
            "Tests\nfinished",
            "Finished\nOK",
            "Finished\nerrors",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        Report::DeliveriesSummary => [
            "Delivery",
            "Students",
            "Tests\npass",
            "Tests\nfailed",
            "Checks\npass",
            "Checks\nfailed",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        Report::DeliveryTests | Report::DeliveryChecks => headers.to_vec(),
    }
}

fn compact_table_rows(report: Report, rows: &mut [Vec<String>]) {
    match report {
        Report::Runs => {
            for row in rows {
                for index in [2, 3] {
                    if let Some(value) = row.get_mut(index) {
                        *value = compact_timestamp(value);
                    }
                }
            }
        }
        Report::DeliveryTests | Report::DeliveryChecks => {
            for row in rows {
                for value in row.iter_mut().skip(2) {
                    *value = match value.as_str() {
                        "pending" => "pend".into(),
                        "running" => "run".into(),
                        _ => value.clone(),
                    };
                }
            }
        }
        Report::DeliveriesSummary => {}
    }
}

fn compact_timestamp(value: &str) -> String {
    if value.len() >= 16 && value.as_bytes().get(10) == Some(&b'T') {
        format!("{} {}Z", &value[..10], &value[11..16])
    } else {
        value.to_owned()
    }
}

fn wrap_header(header: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    if header.contains('\n') {
        return header
            .split('\n')
            .take(2)
            .map(|part| truncate(part, width))
            .collect();
    }
    if display_width(header) <= width {
        return vec![header.to_owned()];
    }
    let words = header
        .split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if words.len() > 1 {
        let mut first = String::new();
        let mut split_at = 0;
        for (index, word) in words.iter().enumerate() {
            let candidate = if first.is_empty() {
                (*word).to_owned()
            } else {
                format!("{first}_{word}")
            };
            if display_width(&candidate) > width {
                break;
            }
            first = candidate;
            split_at = index + 1;
        }
        if split_at > 0 && split_at < words.len() {
            let second = words[split_at..].join("_");
            return vec![first, truncate(&second, width)];
        }
    }
    let chars = header.chars().collect::<Vec<_>>();
    if chars.len() > width && chars.len() <= width * 2 {
        return vec![
            chars[..width].iter().collect(),
            chars[width..].iter().collect(),
        ];
    }
    vec![truncate(header, width)]
}

fn two_line_header_width(header: &str) -> usize {
    let Some(width) = header
        .char_indices()
        .filter(|(_, character)| matches!(character, '_' | '-' | ' '))
        .map(|(index, character)| {
            let split = index + character.len_utf8();
            let first = header[..index].trim_end_matches(['_', '-', ' ']);
            let second = header[split..].trim_start_matches(['_', '-', ' ']);
            display_width(first).max(display_width(second))
        })
        .min_by_key(|width| *width)
    else {
        return display_width(header).div_ceil(2);
    };
    width
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
