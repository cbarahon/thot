//! Purpose: Builds student and professor grade reports from saved run expedients.
use crate::{Settings, api_client::ApiClient};
use reqwest::Method;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    io::{self, Write},
    path::PathBuf,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Audience {
    Students,
    Professor,
    EvaluationItems,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Pdf,
    Csv,
}

struct Options {
    audience: Audience,
    format: Format,
    run_id: Option<String>,
    output_dir: Option<PathBuf>,
}

pub async fn handle(args: &[String], settings: &Settings) -> Result<(), Box<dyn Error>> {
    let options = parse_args(args)?;
    let client = ApiClient::new(settings)?;
    let runs = client.request(Method::GET, "/api/v1/runs", None).await?;
    let runs = runs
        .as_array()
        .ok_or("thotd returned an invalid runs list")?;
    let run = match options.run_id.as_deref() {
        Some(run_id) => runs
            .iter()
            .find(|row| row["run_id"].as_str() == Some(run_id))
            .ok_or_else(|| format!("run {run_id:?} was not found"))?,
        None => runs.first().ok_or("there are no saved runs")?,
    };
    let run_id = run["run_id"]
        .as_str()
        .ok_or("selected run has no run_id")?
        .to_owned();
    let expedient_ids = run["expedient_ids"]
        .as_array()
        .ok_or("selected run has no expedient list")?;
    let mut reports = Vec::with_capacity(expedient_ids.len());
    for id in expedient_ids.iter().filter_map(Value::as_str) {
        let path_id = encode_path_segment(id);
        let report = client
            .request(Method::GET, &format!("/api/v1/expedients/{path_id}"), None)
            .await?;
        reports.push(report);
    }
    if reports.is_empty() {
        return Err(format!("run {run_id:?} has no student expedients").into());
    }
    let project_name = reports
        .first()
        .and_then(|report| report["activity_name"].as_str())
        .unwrap_or("Thot project")
        .to_owned();
    match (options.audience, options.format) {
        (Audience::EvaluationItems, Format::Csv) => {
            print_evaluation_csv(&reports)?;
        }
        (Audience::Students, Format::Pdf) => {
            let directory = output_directory(options.output_dir, &run_id, "students")?;
            let mut paths = Vec::new();
            let mut used_names = BTreeSet::new();
            for report in &reports {
                let student = &report["student"];
                let label = student["external_id"]
                    .as_str()
                    .or_else(|| student["name"].as_str())
                    .or_else(|| report["expedient_id"].as_str())
                    .unwrap_or("student");
                let mut filename = safe_filename(label);
                if filename.is_empty() {
                    filename = "student".to_owned();
                }
                let base_name = filename.clone();
                let mut suffix = student["student_id"]
                    .as_i64()
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| used_names.len().to_string());
                while !used_names.insert(filename.clone()) {
                    filename = format!("{base_name}-{suffix}");
                    suffix.push_str("-2");
                }
                let path = directory.join(format!("{filename}.pdf"));
                let mut pdf = PdfDocument::new(595.0, 842.0);
                render_student_report(&mut pdf, report, &project_name, &run_id);
                fs::write(&path, pdf.finish())?;
                paths.push(path);
            }
            println!(
                "Generated {} student PDF report(s) in {}",
                paths.len(),
                directory.display()
            );
            for path in paths {
                println!("{}", path.display());
            }
        }
        (Audience::Professor, Format::Pdf) => {
            let directory = options
                .output_dir
                .unwrap_or_else(|| PathBuf::from("grades").join(safe_filename(&run_id)));
            fs::create_dir_all(&directory)?;
            let path = directory.join("professor_grades.pdf");
            let mut pdf = PdfDocument::new(842.0, 595.0);
            render_professor_report(&mut pdf, &reports, &project_name, &run_id);
            fs::write(&path, pdf.finish())?;
            println!("Generated professor grade report: {}", path.display());
        }
        _ => unreachable!("argument parser validates report format"),
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Options, Box<dyn Error>> {
    let mut audience = None;
    let mut format = None;
    let mut run_id = None;
    let mut output_dir = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--students" => audience = select_once(audience, Audience::Students, "audience")?,
            "--professor" => audience = select_once(audience, Audience::Professor, "audience")?,
            "--evaluation-items" => {
                audience = select_once(audience, Audience::EvaluationItems, "audience")?
            }
            "--pdf" => format = select_once(format, Format::Pdf, "output format")?,
            "--csv" => format = select_once(format, Format::Csv, "output format")?,
            "--output-dir" => {
                index += 1;
                output_dir = Some(PathBuf::from(
                    args.get(index)
                        .ok_or("--output-dir needs a directory path")?,
                ));
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown grades report option {value:?}").into());
            }
            value if run_id.is_none() => run_id = Some(value.to_owned()),
            value => return Err(format!("unexpected grades report argument {value:?}").into()),
        }
        index += 1;
    }
    let audience =
        audience.ok_or("choose one of --students, --professor, or --evaluation-items")?;
    let format = format.ok_or("choose --pdf or --csv")?;
    match (audience, format) {
        (Audience::Students | Audience::Professor, Format::Pdf)
        | (Audience::EvaluationItems, Format::Csv) => {}
        (Audience::EvaluationItems, Format::Pdf) => {
            return Err("--evaluation-items currently supports --csv".into());
        }
        (_, Format::Csv) => return Err("--csv is supported with --evaluation-items".into()),
    }
    if output_dir.is_some() && format != Format::Pdf {
        return Err("--output-dir can only be used with --pdf".into());
    }
    Ok(Options {
        audience,
        format,
        run_id,
        output_dir,
    })
}

fn select_once<T: Copy>(
    current: Option<T>,
    selected: T,
    label: &str,
) -> Result<Option<T>, Box<dyn Error>> {
    if current.is_some() {
        return Err(format!("choose only one grades report {label}").into());
    }
    Ok(Some(selected))
}

fn output_directory(
    requested: Option<PathBuf>,
    run_id: &str,
    kind: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let directory = requested.unwrap_or_else(|| {
        let run_component = safe_filename(run_id);
        PathBuf::from("grades")
            .join(if run_component.is_empty() {
                "run".into()
            } else {
                run_component
            })
            .join(kind)
    });
    fs::create_dir_all(&directory)?;
    Ok(directory)
}

fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn safe_filename(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
            result.push(character);
        } else if character.is_whitespace() {
            result.push('_');
        } else if character.is_alphanumeric() {
            result.push('x');
        }
    }
    result.trim_matches('.').to_owned()
}

fn print_evaluation_csv(reports: &[Value]) -> Result<(), Box<dyn Error>> {
    let mut item_names = Vec::new();
    for report in reports {
        for item in array(&report["evaluation"]["results"]) {
            if let Some(name) = item["name"].as_str()
                && !item_names.iter().any(|existing| existing == name)
            {
                item_names.push(name.to_owned());
            }
        }
    }
    let mut output = Vec::new();
    let mut headers = vec![
        "student_name".to_owned(),
        "student_external_id".to_owned(),
        "student_email".to_owned(),
    ];
    headers.extend(item_names.iter().cloned());
    write_csv(&mut output, &headers)?;
    for report in reports {
        let student = &report["student"];
        let full_name = [
            student["name"].as_str().unwrap_or_default(),
            student["surname"].as_str().unwrap_or_default(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
        let mut row = vec![
            full_name,
            string(&student["external_id"]),
            string(&student["email"]),
        ];
        for name in &item_names {
            let result = array(&report["evaluation"]["results"])
                .iter()
                .find(|item| item["name"].as_str() == Some(name));
            row.push(
                result
                    .map(|item| display_value(&item["result"]))
                    .unwrap_or_default(),
            );
        }
        write_csv(&mut output, &row)?;
    }
    io::stdout().write_all(&output)?;
    Ok(())
}

fn write_csv(output: &mut impl Write, fields: &[String]) -> io::Result<()> {
    for (index, field) in fields.iter().enumerate() {
        if index != 0 {
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

fn render_student_report(pdf: &mut PdfDocument, report: &Value, project: &str, run_id: &str) {
    let student = &report["student"];
    let student_name = [
        student["name"].as_str().unwrap_or_default(),
        student["surname"].as_str().unwrap_or_default(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ");
    pdf.heading("Student progress report");
    pdf.text(&format!("Project: {project}"));
    pdf.text(&format!("Project ID: {}", string(&report["project_id"])));
    pdf.text(&format!(
        "Run: {run_id}    Report date: {}",
        string(&report["finished_at"])
    ));
    pdf.text(&format!(
        "Student: {}",
        if student_name.is_empty() {
            "Not recorded"
        } else {
            &student_name
        }
    ));
    pdf.text(&format!(
        "Student ID: {}    Email: {}",
        string(&student["external_id"]),
        string(&student["email"])
    ));
    pdf.text(&format!(
        "Submitted work: {}",
        string(&report["delivery_filename"])
    ));
    pdf.heading("Final grades");

    let evaluations = array(&report["evaluation"]["results"]);
    let ei_codes = codes(
        "EI",
        evaluations.iter().filter_map(|row| row["name"].as_str()),
    );
    let eval_rows = evaluations
        .iter()
        .map(|item| {
            let name = string(&item["name"]);
            vec![
                ei_codes.get(&name).cloned().unwrap_or_default(),
                name,
                string(&item["description"]),
                display_mark(&item["result"]),
            ]
        })
        .collect::<Vec<_>>();
    if eval_rows.is_empty() {
        pdf.text("No final evaluation marks are available for this report.");
    } else {
        pdf.table(
            &["Code", "Evaluation item", "Description", "Mark"],
            &eval_rows,
            &[44.0, 125.0, 300.0, 55.0],
        );
    }

    let rubric_rows = array(&report["rubrics"]);
    let check_rows = array(&report["checks"]);
    let rubric_codes = codes(
        "R",
        rubric_rows.iter().filter_map(|row| row["name"].as_str()),
    );
    let check_codes = codes(
        "C",
        check_rows.iter().filter_map(|row| row["name"].as_str()),
    );
    pdf.heading("Rubric results by evaluation item");
    let mut found_rubric = false;
    for item in evaluations {
        let item_name = string(&item["name"]);
        pdf.subheading(&format!(
            "{} — {}",
            ei_codes.get(&item_name).cloned().unwrap_or_default(),
            item_name
        ));
        let mut rows = Vec::new();
        for ref_row in array(&item["rubric_results"]) {
            let name = string(&ref_row["name"]);
            let rubric = rubric_rows
                .iter()
                .find(|row| row["name"].as_str() == Some(&name));
            rows.push(vec![
                rubric_codes.get(&name).cloned().unwrap_or_default(),
                name,
                rubric
                    .map(|row| string(&row["description"]))
                    .unwrap_or_default(),
                display_mark(&ref_row["result"]),
            ]);
        }
        if !rows.is_empty() {
            found_rubric = true;
            pdf.table(
                &["Code", "Rubric", "Description", "Mark"],
                &rows,
                &[44.0, 125.0, 300.0, 55.0],
            );
        }
    }
    if !found_rubric {
        pdf.text("No rubric marks are available.");
    }

    pdf.section_break();
    pdf.heading("How rubric marks were calculated");
    let mut matrix_rows = Vec::new();
    for item in evaluations {
        let ei_name = string(&item["name"]);
        for rubric_ref in array(&item["rubric_results"]) {
            let r_name = string(&rubric_ref["name"]);
            let rubric = rubric_rows
                .iter()
                .find(|row| row["name"].as_str() == Some(&r_name));
            let Some(rubric) = rubric else { continue };
            for check_ref in array(&rubric["check_results"]) {
                let c_name = string(&check_ref["name"]);
                let check = check_rows
                    .iter()
                    .find(|row| row["name"].as_str() == Some(&c_name));
                matrix_rows.push(vec![
                    ei_codes.get(&ei_name).cloned().unwrap_or_default(),
                    rubric_codes.get(&r_name).cloned().unwrap_or_default(),
                    check_codes.get(&c_name).cloned().unwrap_or_default(),
                    display_value(&check_ref["result"]),
                    check
                        .map(|row| string(&row["description"]))
                        .unwrap_or_default(),
                ]);
            }
        }
    }
    if matrix_rows.is_empty() {
        pdf.text("No check-level rubric details are available.");
    } else {
        pdf.table(
            &["EI", "Rubric", "Check", "Result", "Check description"],
            &matrix_rows,
            &[38.0, 54.0, 54.0, 55.0, 323.0],
        );
    }

    pdf.section_break();
    pdf.heading("Code guide");
    for item in evaluations {
        let name = string(&item["name"]);
        pdf.text(&format!(
            "{}: {} — {}",
            ei_codes.get(&name).cloned().unwrap_or_default(),
            name,
            string(&item["description"])
        ));
        for rubric_ref in array(&item["rubric_results"]) {
            let name = string(&rubric_ref["name"]);
            let rubric = rubric_rows
                .iter()
                .find(|row| row["name"].as_str() == Some(&name));
            pdf.text(&format!(
                "  {}: {} — {}",
                rubric_codes.get(&name).cloned().unwrap_or_default(),
                name,
                rubric
                    .map(|row| string(&row["description"]))
                    .unwrap_or_default()
            ));
        }
    }
    for check in check_rows {
        let name = string(&check["name"]);
        pdf.text(&format!(
            "{}: {} — {}",
            check_codes.get(&name).cloned().unwrap_or_default(),
            name,
            string(&check["description"])
        ));
    }
    pdf.text("Check result: 1 means the requirement was met; 0 means it was not met. A blank mark means no result was recorded.");

    pdf.section_break();
    pdf.heading("Test results appendix");
    pdf.text("These results show the evidence used to calculate the marks. Process output is included when available.");
    let tests = array(&report["test_results"]);
    if tests.is_empty() {
        pdf.text("No tests were recorded for this student.");
    }
    for test in tests {
        let test_id = string(&test["test_id"]);
        let description = test["test_description"]
            .as_str()
            .filter(|description| !description.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| friendly_test_name(&test_id, test["test_reference"].as_str()));
        pdf.subheading(&format!("{test_id} — {description}"));
        pdf.text(&format!(
            "Result: {}    Exit code: {}    Duration: {} ms",
            friendly_status(test["status"].as_str().unwrap_or("not_run")),
            display_value(&test["exit_code"]),
            display_value(&test["duration_ms"])
        ));
        append_output(pdf, "Output", test["stdout"].as_str().unwrap_or_default());
        append_output(
            pdf,
            "Additional output",
            test["stderr"].as_str().unwrap_or_default(),
        );
    }
}

fn render_professor_report(pdf: &mut PdfDocument, reports: &[Value], project: &str, run_id: &str) {
    let evaluation_names = union_names(reports, |report| &report["evaluation"]["results"]);
    let rubric_names = union_names(reports, |report| &report["rubrics"]);
    let check_names = union_names(reports, |report| &report["checks"]);
    let test_names = union_names(reports, |report| &report["test_results"]);
    let ei_codes = codes("EI", evaluation_names.iter().map(String::as_str));
    let rubric_codes = codes("R", rubric_names.iter().map(String::as_str));
    let check_codes = codes("C", check_names.iter().map(String::as_str));

    pdf.heading("Professor grade summary");
    pdf.text(&format!("Project: {project}    Run: {run_id}"));
    pdf.text(&format!("Students included: {}", reports.len()));
    pdf.heading("Final grades by evaluation item");
    let mut headers = vec!["Student".to_owned(), "External ID".to_owned()];
    headers.extend(evaluation_names.iter().map(|name| ei_codes[name].clone()));
    let rows = reports
        .iter()
        .map(|report| {
            let mut row = student_columns(report);
            row.extend(evaluation_names.iter().map(|name| {
                find_result(&report["evaluation"]["results"], name)
                    .map(|item| display_mark(&item["result"]))
                    .unwrap_or_default()
            }));
            row
        })
        .collect::<Vec<_>>();
    pdf.table_dynamic(&headers, &rows);

    pdf.heading("Rubric marks grouped by evaluation item");
    for evaluation_name in &evaluation_names {
        let rubric_order = reports
            .iter()
            .find_map(|report| find_result(&report["evaluation"]["results"], evaluation_name))
            .map(|evaluation| {
                array(&evaluation["rubric_results"])
                    .iter()
                    .filter_map(|row| row["name"].as_str())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        pdf.subheading(&format!(
            "{} — {evaluation_name}",
            ei_codes[evaluation_name]
        ));
        let mut headers = vec!["Student".to_owned(), "External ID".to_owned()];
        headers.extend(rubric_order.iter().map(|name| {
            rubric_codes
                .get(name)
                .cloned()
                .unwrap_or_else(|| name.clone())
        }));
        let rows = reports
            .iter()
            .map(|report| {
                let mut row = student_columns(report);
                row.extend(rubric_order.iter().map(|name| {
                    find_result(&report["rubrics"], name)
                        .map(|item| display_mark(&item["result"]))
                        .unwrap_or_default()
                }));
                row
            })
            .collect::<Vec<_>>();
        if rubric_order.is_empty() {
            pdf.text("No rubric results are configured for this evaluation item.");
        } else {
            pdf.table_dynamic(&headers, &rows);
        }
    }

    pdf.section_break();
    pdf.heading("Check results, grouped by rubric");
    for rubric_name in &rubric_names {
        let check_order = reports
            .iter()
            .flat_map(|report| array(&report["rubrics"]).iter())
            .find(|rubric| rubric["name"].as_str() == Some(rubric_name))
            .map(|rubric| {
                array(&rubric["checks"])
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| check_names.clone());
        pdf.subheading(&format!("{} — {rubric_name}", rubric_codes[rubric_name]));
        let mut headers = vec!["Student".to_owned(), "External ID".to_owned()];
        headers.extend(check_order.iter().map(|name| {
            check_codes
                .get(name)
                .cloned()
                .unwrap_or_else(|| name.clone())
        }));
        let rows = reports
            .iter()
            .map(|report| {
                let mut row = student_columns(report);
                let rubric = find_result(&report["rubrics"], rubric_name);
                row.extend(check_order.iter().map(|name| {
                    rubric
                        .and_then(|rubric| find_result(&rubric["check_results"], name))
                        .map(|check| display_value(&check["result"]))
                        .or_else(|| {
                            find_result(&report["checks"], name)
                                .map(|check| display_value(&check["result"]))
                        })
                        .unwrap_or_default()
                }));
                row
            })
            .collect::<Vec<_>>();
        pdf.table_dynamic(&headers, &rows);
    }

    pdf.section_break();
    pdf.heading("Test outcomes by student");
    let mut headers = vec!["Student".to_owned(), "External ID".to_owned()];
    headers.extend(test_names.iter().cloned());
    let rows = reports
        .iter()
        .map(|report| {
            let mut row = student_columns(report);
            row.extend(test_names.iter().map(|name| {
                find_result(&report["test_results"], name)
                    .map(|test| match test["status"].as_str().unwrap_or("") {
                        "passed" => "Pass".to_owned(),
                        "skipped" | "not_run" => "Skipped".to_owned(),
                        "running" => "Running".to_owned(),
                        _ => "No pass".to_owned(),
                    })
                    .unwrap_or_default()
            }));
            row
        })
        .collect::<Vec<_>>();
    pdf.table_dynamic(&headers, &rows);

    pdf.section_break();
    pdf.heading("Code guide");
    for name in &evaluation_names {
        let description = reports
            .iter()
            .find_map(|report| {
                find_result(&report["evaluation"]["results"], name)
                    .and_then(|item| item["description"].as_str())
            })
            .unwrap_or_default();
        pdf.text(&format!("{}: {name} — {description}", ei_codes[name]));
    }
    for name in &rubric_names {
        let description = reports
            .iter()
            .find_map(|report| {
                find_result(&report["rubrics"], name).and_then(|item| item["description"].as_str())
            })
            .unwrap_or_default();
        pdf.text(&format!("{}: {name} — {description}", rubric_codes[name]));
    }
    for name in &check_names {
        let description = reports
            .iter()
            .find_map(|report| {
                find_result(&report["checks"], name).and_then(|item| item["description"].as_str())
            })
            .unwrap_or_default();
        pdf.text(&format!("{}: {name} — {description}", check_codes[name]));
    }
    pdf.text("Check result: 1 means the requirement was met; 0 means it was not met. Pass means a test completed successfully; No pass includes test failures and errors.");
}

fn append_output(pdf: &mut PdfDocument, label: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    pdf.text(label);
    let safe = redact_sensitive_output(value);
    let truncated = if safe.chars().count() > 1400 {
        format!(
            "{}\n[Output shortened for this report]",
            safe.chars().take(1400).collect::<String>()
        )
    } else {
        safe
    };
    for line in truncated.lines() {
        let line = if line.chars().count() > 110 {
            line.chars().take(110).collect::<String>()
        } else {
            line.to_owned()
        };
        pdf.code_text(&line);
    }
}

fn redact_sensitive_output(text: &str) -> String {
    let mut result = String::new();
    for line in text.lines() {
        let lowered = line.to_ascii_lowercase();
        if lowered.contains("password")
            || lowered.contains("passwd")
            || lowered.contains("secret")
            || lowered.contains("token")
            || lowered.contains("pass=")
            || lowered.contains("pass:")
        {
            result.push_str("[Sensitive credential-like output omitted]");
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }
    result
}

fn friendly_test_name(test_id: &str, reference: Option<&str>) -> String {
    match reference.unwrap_or("") {
        "VirtualBox/import_ova" => "Virtual machine import".into(),
        "VirtualBox/start_vm" => "Virtual machine startup".into(),
        "VirtualBox/stop_vm" => "Virtual machine shutdown".into(),
        "ssh/get_host_info" => "Remote system information".into(),
        "network/nmap" => "Network service availability".into(),
        "libguestfs/linux_rootfs_info" => "Linux installation information".into(),
        "libguestfs/linux_tinycore_info" | "libguestfs/linux_livefs_info" => {
            "Live Linux system information".into()
        }
        _ => test_id.replace('_', " "),
    }
}

fn friendly_status(status: &str) -> &'static str {
    match status {
        "passed" => "Passed",
        "failed" | "error" | "timed_out" => "Did not pass",
        "skipped" | "not_run" => "Not run",
        "running" => "In progress",
        _ => "No result",
    }
}

fn student_columns(report: &Value) -> Vec<String> {
    let student = &report["student"];
    let name = [
        student["name"].as_str().unwrap_or_default(),
        student["surname"].as_str().unwrap_or_default(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ");
    vec![name, string(&student["external_id"])]
}

fn union_names(reports: &[Value], get: impl Fn(&Value) -> &Value) -> Vec<String> {
    let mut names = Vec::new();
    for report in reports {
        for row in array(get(report)) {
            if let Some(name) = row["name"].as_str().or_else(|| row["test_id"].as_str())
                && !names.iter().any(|item| item == name)
            {
                names.push(name.to_owned());
            }
        }
    }
    names
}

fn codes<'a>(prefix: &str, names: impl Iterator<Item = &'a str>) -> BTreeMap<String, String> {
    let mut codes = BTreeMap::new();
    for name in names {
        if !codes.contains_key(name) {
            let code = format!("{prefix}{:02}", codes.len() + 1);
            codes.insert(name.to_owned(), code);
        }
    }
    codes
}

fn find_result<'a>(rows: &'a Value, name: &str) -> Option<&'a Value> {
    array(rows)
        .iter()
        .find(|row| row["name"].as_str() == Some(name) || row["test_id"].as_str() == Some(name))
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

fn string(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn display_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn display_mark(value: &Value) -> String {
    let Some(number) = value.as_f64().filter(|number| number.is_finite()) else {
        return display_value(value);
    };
    let absolute = number.abs();
    let decimals = if absolute > 100.0 {
        0
    } else if absolute > 10.0 {
        1
    } else if absolute > 1.0 || absolute == 0.0 {
        2
    } else {
        (2.0 - absolute.log10().floor()).clamp(0.0, 8.0) as usize
    };
    format!("{number:.decimals$}")
}

struct PdfDocument {
    width: f32,
    height: f32,
    pages: Vec<Vec<String>>,
    y: f32,
}

impl PdfDocument {
    fn new(width: f32, height: f32) -> Self {
        let mut document = Self {
            width,
            height,
            pages: vec![Vec::new()],
            y: 44.0,
        };
        document.line(36.0, 28.0, width - 36.0, 28.0);
        document
    }

    fn page(&mut self) {
        self.pages.push(Vec::new());
        self.y = 42.0;
        self.line(36.0, 28.0, self.width - 36.0, 28.0);
    }

    fn ensure(&mut self, height: f32) {
        if self.y + height > self.height - 40.0 {
            self.page();
        }
    }

    fn section_break(&mut self) {
        if self.y > 45.0 {
            self.page();
        }
    }

    fn heading(&mut self, text: &str) {
        self.ensure(36.0);
        self.y += 4.0;
        self.draw_text(36.0, self.y, 15.0, true, text);
        self.y += 22.0;
    }

    fn subheading(&mut self, text: &str) {
        for line in wrap_text(text, 90) {
            self.ensure(15.0);
            self.draw_text(36.0, self.y, 11.0, true, &line);
            self.y += 14.0;
        }
    }

    fn text(&mut self, text: &str) {
        for line in wrap_text(text, 108) {
            self.ensure(14.0);
            self.draw_text(36.0, self.y, 9.0, false, &line);
            self.y += 12.0;
        }
    }

    fn code_text(&mut self, text: &str) {
        for line in wrap_text(text, 112) {
            self.ensure(12.0);
            self.draw_text(43.0, self.y, 7.3, false, &line);
            self.y += 9.0;
        }
    }

    fn table(&mut self, headers: &[&str], rows: &[Vec<String>], widths: &[f32]) {
        let headers = headers
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        self.table_with_widths(&headers, rows, widths);
    }

    fn table_dynamic(&mut self, headers: &[String], rows: &[Vec<String>]) {
        if headers.is_empty() {
            return;
        }
        let usable = self.width - 72.0;
        let widths = headers
            .iter()
            .enumerate()
            .map(|(column, header)| {
                let max_chars = rows
                    .iter()
                    .filter_map(|row| row.get(column))
                    .map(|value| value.chars().count())
                    .chain(std::iter::once(header.chars().count()))
                    .max()
                    .unwrap_or(0) as f32;
                let desired = max_chars * 3.8 + 10.0;
                match column {
                    0 => desired.clamp(58.0, 125.0),
                    1 => desired.clamp(48.0, 105.0),
                    _ => desired.clamp(30.0, 82.0),
                }
            })
            .collect::<Vec<_>>();
        let total = widths.iter().sum::<f32>();
        if total <= usable || headers.len() <= 2 {
            self.table_with_widths(headers, rows, &widths);
            return;
        }

        // Keep student identity columns fixed and split wider matrices into readable blocks.
        let identity_width = widths.iter().take(2).sum::<f32>().min(usable * 0.55);
        let mut chunks: Vec<Vec<usize>> = Vec::new();
        let mut current = Vec::new();
        let mut current_width = identity_width;
        for column in 2..headers.len() {
            if !current.is_empty() && current_width + widths[column] > usable {
                chunks.push(std::mem::take(&mut current));
                current_width = identity_width;
            }
            current.push(column);
            current_width += widths[column];
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        for chunk in chunks {
            let mut indexes = vec![0, 1];
            indexes.extend(chunk);
            let chunk_headers = indexes
                .iter()
                .map(|&column| headers[column].clone())
                .collect::<Vec<_>>();
            let chunk_widths = indexes
                .iter()
                .map(|&column| widths[column])
                .collect::<Vec<_>>();
            let chunk_rows = rows
                .iter()
                .map(|row| {
                    indexes
                        .iter()
                        .map(|&column| row.get(column).cloned().unwrap_or_default())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            self.table_with_widths(&chunk_headers, &chunk_rows, &chunk_widths);
        }
    }

    fn table_with_widths(&mut self, headers: &[String], rows: &[Vec<String>], widths: &[f32]) {
        let total_width = widths.iter().sum::<f32>();
        let headers = (0..headers.len())
            .map(|index| headers.get(index).cloned().unwrap_or_default())
            .collect::<Vec<_>>();
        if self.y + table_row_height(&headers, widths) > self.height - 40.0 {
            self.page();
        }
        self.draw_table_row(&headers, widths, total_width, true);
        for row in rows {
            let cells = (0..headers.len())
                .map(|index| row.get(index).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            if self.y + table_row_height(&cells, widths) > self.height - 40.0 {
                self.page();
                self.draw_table_row(&headers, widths, total_width, true);
            }
            self.draw_table_row(&cells, widths, total_width, false);
        }
        self.y += 8.0;
    }

    fn draw_table_row(
        &mut self,
        cells: &[String],
        widths: &[f32],
        total_width: f32,
        is_header: bool,
    ) {
        let lines = cells
            .iter()
            .zip(widths)
            .map(|(cell, width)| wrap_text(cell, ((*width - 8.0) / 4.2).max(5.0) as usize))
            .collect::<Vec<_>>();
        let row_height =
            (lines.iter().map(Vec::len).max().unwrap_or(1) as f32 * 9.0 + 7.0).max(18.0);
        let mut x = 40.0;
        for (index, column) in lines.iter().enumerate() {
            for (line_index, line) in column.iter().enumerate() {
                self.draw_text(x, self.y + line_index as f32 * 9.0, 7.3, is_header, line);
            }
            x += widths.get(index).copied().unwrap_or(40.0);
        }
        self.y += row_height;
        self.line(36.0, self.y - 12.0, 36.0 + total_width, self.y - 12.0);
    }

    fn draw_text(&mut self, x: f32, y_from_top: f32, size: f32, bold: bool, value: &str) {
        let font = if bold { "F2" } else { "F1" };
        let y = self.height - y_from_top;
        self.pages.last_mut().unwrap().push(format!(
            "BT /{font} {size:.1} Tf 1 0 0 1 {x:.1} {y:.1} Tm ({}) Tj ET\n",
            pdf_literal(value)
        ));
    }

    fn line(&mut self, x1: f32, y1_from_top: f32, x2: f32, y2_from_top: f32) {
        let y1 = self.height - y1_from_top;
        let y2 = self.height - y2_from_top;
        self.pages
            .last_mut()
            .unwrap()
            .push(format!("0.65 w {x1:.1} {y1:.1} m {x2:.1} {y2:.1} l S\n"));
    }

    fn finish(mut self) -> Vec<u8> {
        let pages_count = self.pages.len();
        for (index, page) in self.pages.iter_mut().enumerate() {
            let page_num = index + 1;
            page.push(format!(
                "BT /F1 7 Tf 1 0 0 1 36 18 Tm (Page {page_num} of {pages_count}) Tj ET\n"
            ));
        }
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            Vec::new(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec(),
        ];
        let mut kids = Vec::new();
        for page in self.pages {
            let page_object = objects.len() + 1;
            let stream_object = page_object + 1;
            kids.push(format!("{page_object} 0 R"));
            let content = page.concat().into_bytes();
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.1} {:.1}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {stream_object} 0 R >>", self.width, self.height).into_bytes());
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(&content);
            stream.extend_from_slice(b"endstream");
            objects.push(stream);
        }
        objects[1] = format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages_count
        )
        .into_bytes();
        let mut output = b"%PDF-1.4\n%THOT\n".to_vec();
        let mut offsets = vec![0usize];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(output.len());
            output.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            output.extend_from_slice(object);
            output.extend_from_slice(b"\nendobj\n");
        }
        let xref_offset = output.len();
        output.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets.iter().skip(1) {
            output.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        output.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        output
    }
}

fn table_row_height(cells: &[String], widths: &[f32]) -> f32 {
    let lines = cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| wrap_text(cell, ((*width - 8.0) / 4.2).max(5.0) as usize).len())
        .max()
        .unwrap_or(1);
    (lines as f32 * 9.0 + 7.0).max(18.0)
}

fn wrap_text(value: &str, limit: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in value.replace('\r', "").split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > limit {
                lines.push(std::mem::take(&mut current));
            }
            if current.is_empty() && word.chars().count() > limit {
                let mut part = String::new();
                for ch in word.chars() {
                    part.push(ch);
                    if part.chars().count() >= limit {
                        lines.push(std::mem::take(&mut part));
                    }
                }
                current = part;
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn pdf_literal(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        let byte = match character {
            '\u{20ac}' => 0x80,
            '\u{201a}' => 0x82,
            '\u{0192}' => 0x83,
            '\u{201e}' => 0x84,
            '\u{2026}' => 0x85,
            '\u{2020}' => 0x86,
            '\u{2021}' => 0x87,
            '\u{02c6}' => 0x88,
            '\u{2030}' => 0x89,
            '\u{0160}' => 0x8a,
            '\u{2039}' => 0x8b,
            '\u{0152}' => 0x8c,
            '\u{017d}' => 0x8e,
            '\u{2018}' => 0x91,
            '\u{2019}' => 0x92,
            '\u{201c}' => 0x93,
            '\u{201d}' => 0x94,
            '\u{2022}' => 0x95,
            '\u{2013}' => 0x96,
            '\u{2014}' => 0x97,
            '\u{02dc}' => 0x98,
            '\u{2122}' => 0x99,
            '\u{0161}' => 0x9a,
            '\u{203a}' => 0x9b,
            '\u{0153}' => 0x9c,
            '\u{017e}' => 0x9e,
            '\u{0178}' => 0x9f,
            '\u{00a0}'..='\u{00ff}' => character as u8,
            '\n' | '\r' => b' ',
            c if c.is_ascii() => c as u8,
            _ => b'?',
        };
        match byte {
            b'(' | b')' | b'\\' => {
                escaped.push('\\');
                escaped.push(byte as char);
            }
            0x20..=0x7e => escaped.push(byte as char),
            _ => escaped.push_str(&format!("\\{byte:03o}")),
        }
    }
    escaped
}
