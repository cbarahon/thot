use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub fn handle(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    match args.first().map(String::as_str) {
        Some("report") => report(&args[1..]),
        Some("export") if args.len() == 3 => {
            thot_engine::sandbox_repository::export_run_archive(Path::new(&args[1]), Path::new(&args[2]))?;
            println!("Verified run archive created: {}", args[2]);
            Ok(())
        }
        Some("import") if args.len() == 3 => {
            let verification = thot_engine::sandbox_repository::import_run_archive(Path::new(&args[1]), Path::new(&args[2]))?;
            println!("Imported and verified run {} ({:?}) at {}", verification.run_id, verification.status, args[2]);
            Ok(())
        }
        _ => Err("usage: thot sandbox run [PROJECT_DIR] [--agent PATH] | report RUN_DIR [--json|--csv|--html FILE] | export RUN_DIR ARCHIVE.zip | import ARCHIVE.zip DEST_DIR".into()),
    }
}

fn report(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut run_dir = None;
    let mut format = "table";
    let mut output_path = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" | "--csv" => {
                if format != "table" || output_path.is_some() {
                    return Err("choose one report format".into());
                }
                format = args[index].trim_start_matches("--");
            }
            "--html" => {
                if format != "table" || output_path.is_some() {
                    return Err("choose one report format".into());
                }
                format = "html";
                index += 1;
                output_path = Some(PathBuf::from(
                    args.get(index).ok_or("--html requires an output file")?,
                ));
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown sandbox report option: {value}").into());
            }
            value if run_dir.is_none() => run_dir = Some(PathBuf::from(value)),
            _ => {
                return Err("usage: thot sandbox report RUN_DIR [--json|--csv|--html FILE]".into());
            }
        }
        index += 1;
    }
    let run_dir = run_dir.ok_or("sandbox report requires a committed run directory")?;
    let verification = thot_engine::sandbox_repository::ProjectRunRepository::verify_run(&run_dir)?;
    if verification.status != thot_core::sandbox_contracts::RunStatus::Complete {
        return Err(format!(
            "run {} is not complete ({:?})",
            verification.run_id, verification.status
        )
        .into());
    }
    let reports = load_student_reports(&run_dir)?;
    let rendered = match format {
        "json" => format!("{}\n", serde_json::to_string_pretty(&reports)?),
        "csv" => render_csv(&reports)?,
        "html" => render_html(&reports),
        _ => render_table(&reports),
    };
    if let Some(path) = output_path {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(rendered.as_bytes())?;
        file.sync_all()?;
    } else {
        io::stdout().write_all(rendered.as_bytes())?;
    }
    Ok(())
}

fn load_student_reports(run_dir: &Path) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut paths = fs::read_dir(run_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
                && path.file_name().is_some_and(|name| name != "run.json")
        })
        .collect::<Vec<_>>();
    paths.sort();
    let mut reports = Vec::with_capacity(paths.len());
    for path in paths {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!("report entry is not a regular file: {}", path.display()).into());
        }
        let report: Value = serde_json::from_slice(&fs::read(&path)?)?;
        if !report["student_id"].is_string() || !report["tests"].is_array() {
            return Err(format!("invalid student report: {}", path.display()).into());
        }
        reports.push(report);
    }
    if reports.is_empty() {
        return Err("verified run contains no student reports".into());
    }
    Ok(reports)
}

fn item_columns(reports: &[Value]) -> Vec<String> {
    reports
        .iter()
        .flat_map(|report| report["evaluation_items"].as_array().into_iter().flatten())
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn render_csv(reports: &[Value]) -> Result<String, Box<dyn std::error::Error>> {
    let items = item_columns(reports);
    let mut output = Vec::new();
    let mut headers = vec![
        "student_id".to_owned(),
        "deliverable".into(),
        "assessment_status".into(),
        "grade".into(),
        "scale".into(),
        "cleanup_status".into(),
    ];
    for item in &items {
        headers.extend([format!("{item}_grade"), format!("{item}_status")]);
    }
    write_csv_row(&mut output, &headers)?;
    for report in reports {
        let mut row = vec![
            value_string(&report["student_id"]),
            value_string(&report["deliverable"]["name"]),
            value_string(&report["assessment_status"]),
            value_string(&report["grade"]),
            value_string(&report["scale"]),
            value_string(&report["cleanup_status"]),
        ];
        for id in &items {
            let item = report["evaluation_items"].as_array().and_then(|values| {
                values
                    .iter()
                    .find(|value| value["id"].as_str() == Some(id.as_str()))
            });
            row.push(
                item.map(|value| value_string(&value["grade"]))
                    .unwrap_or_default(),
            );
            row.push(
                item.map(|value| value_string(&value["status"]))
                    .unwrap_or_default(),
            );
        }
        write_csv_row(&mut output, &row)?;
    }
    Ok(String::from_utf8(output)?)
}

fn render_table(reports: &[Value]) -> String {
    let mut output = String::from("Student\tDeliverable\tStatus\tGrade\tCleanup");
    for id in item_columns(reports) {
        output.push_str(&format!("\t{id}"));
    }
    output.push('\n');
    for report in reports {
        output.push_str(&format!(
            "{}\t{}\t{}\t{}/{}\t{}",
            value_string(&report["student_id"]),
            value_string(&report["deliverable"]["name"]),
            value_string(&report["assessment_status"]),
            value_string(&report["grade"]),
            value_string(&report["scale"]),
            value_string(&report["cleanup_status"])
        ));
        for id in item_columns(reports) {
            let item = report["evaluation_items"].as_array().and_then(|values| {
                values
                    .iter()
                    .find(|value| value["id"].as_str() == Some(id.as_str()))
            });
            output.push_str(&format!(
                "\t{} ({})",
                item.map(|value| value_string(&value["grade"]))
                    .unwrap_or_default(),
                item.map(|value| value_string(&value["status"]))
                    .unwrap_or_else(|| "n/a".into())
            ));
        }
        output.push('\n');
    }
    output
}

fn render_html(reports: &[Value]) -> String {
    let items = item_columns(reports);
    let mut output = String::from(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Thot assessment report</title><h1>Thot assessment report</h1><table><thead><tr><th>Student</th><th>Deliverable</th><th>Status</th><th>Grade</th><th>Cleanup</th>",
    );
    for id in &items {
        output.push_str(&format!("<th>{}</th>", html_escape(id)));
    }
    output.push_str("</tr></thead><tbody>");
    for report in reports {
        output.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}/{}</td><td>{}</td>",
            html_escape(&value_string(&report["student_id"])),
            html_escape(&value_string(&report["deliverable"]["name"])),
            html_escape(&value_string(&report["assessment_status"])),
            html_escape(&value_string(&report["grade"])),
            html_escape(&value_string(&report["scale"])),
            html_escape(&value_string(&report["cleanup_status"]))
        ));
        for id in &items {
            let item = report["evaluation_items"].as_array().and_then(|values| {
                values
                    .iter()
                    .find(|value| value["id"].as_str() == Some(id.as_str()))
            });
            let cell = item
                .map(|value| {
                    format!(
                        "{} ({})",
                        value_string(&value["grade"]),
                        value_string(&value["status"])
                    )
                })
                .unwrap_or_else(|| "n/a".into());
            output.push_str(&format!("<td>{}</td>", html_escape(&cell)));
        }
        output.push_str("</tr>");
    }
    output.push_str("</tbody></table><style>body{font:16px system-ui;margin:2rem}table{border-collapse:collapse}th,td{border:1px solid #bbb;padding:.5rem;text-align:left}</style></html>\n");
    output
}

fn value_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn write_csv_row(output: &mut Vec<u8>, fields: &[String]) -> io::Result<()> {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            output.write_all(b",")?;
        }
        output.write_all(b"\"")?;
        output.write_all(field.replace('"', "\"\"").as_bytes())?;
        output.write_all(b"\"")?;
    }
    output.write_all(b"\r\n")
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(crate) fn find_agent() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let adjacent = current_exe.parent()?.join("thot-alpine-agent");
    if adjacent.is_file() {
        return Some(adjacent);
    }
    std::env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(|directory| PathBuf::from(directory).join("thot-alpine-agent"))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_report_renders_multiple_evaluation_items_and_escapes_html() {
        let reports = vec![serde_json::json!({
            "student_id": "estudiante-á",
            "deliverable": {"name": "submission <one>.ova"},
            "assessment_status": "pending",
            "grade": null,
            "scale": 20,
            "cleanup_status": "clean",
            "evaluation_items": [
                {"id":"layout", "grade":8, "scale":10, "status":"complete"},
                {"id":"boot", "grade":null, "scale":10, "status":"pending"}
            ],
            "tests": []
        })];
        let csv = render_csv(&reports).unwrap();
        assert!(csv.contains("layout_grade"));
        assert!(csv.contains("boot_status"));
        assert!(csv.contains("estudiante-á"));
        let html = render_html(&reports);
        assert!(html.contains("submission &lt;one&gt;.ova"));
        assert!(html.contains("boot"));
    }
}
