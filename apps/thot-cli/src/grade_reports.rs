//! Purpose: Builds student and professor grade reports from saved run expedients.
use crate::{Settings, api_client::ApiClient};
use flate2::{Compression, write::ZlibEncoder};
use image::{ImageReader, Limits};
use reqwest::Method;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    io::{self, Cursor, Write},
    path::PathBuf,
};

struct StudentArtifact {
    test_id: String,
    path: String,
    relative_path: String,
    file_name: String,
    content_type: String,
    bytes: Vec<u8>,
}

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
    all_reports: bool,
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
    if options.all_reports {
        let directory = all_output_directory(options.output_dir, &run_id)?;
        let student_directory = directory.join("students");
        fs::create_dir_all(&student_directory)?;
        let student_paths = generate_student_pdfs(
            &client,
            &reports,
            &project_name,
            &run_id,
            &student_directory,
        )
        .await?;

        let professor_path = directory.join("professor_grades.pdf");
        let mut professor_pdf = PdfDocument::new(842.0, 595.0);
        render_professor_report(&mut professor_pdf, &reports, &project_name, &run_id);
        fs::write(&professor_path, professor_pdf.finish())?;

        let moodle_path = directory.join("moodle_grades.csv");
        fs::write(&moodle_path, evaluation_csv(&reports)?)?;
        println!(
            "Generated {} student PDF report(s), professor PDF, and Moodle CSV in {}",
            student_paths.len(),
            directory.display()
        );
        for path in student_paths {
            println!("{}", path.display());
        }
        println!("{}", professor_path.display());
        println!("{}", moodle_path.display());
        return Ok(());
    }
    match (options.audience, options.format) {
        (Audience::EvaluationItems, Format::Csv) => {
            print_evaluation_csv(&reports)?;
        }
        (Audience::Students, Format::Pdf) => {
            let directory = output_directory(options.output_dir, &run_id, "students")?;
            let paths =
                generate_student_pdfs(&client, &reports, &project_name, &run_id, &directory)
                    .await?;
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
    let mut all_reports = false;
    let mut run_id = None;
    let mut output_dir = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--all" => all_reports = true,
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
    if all_reports && (audience.is_some() || format.is_some()) {
        return Err("--all cannot be combined with an audience or format option".into());
    }
    let (audience, format) = if all_reports {
        (Audience::Students, Format::Pdf)
    } else {
        let audience = audience
            .ok_or("choose --all or one of --students, --professor, or --evaluation-items")?;
        let format = format.ok_or("choose --pdf or --csv")?;
        match (audience, format) {
            (Audience::Students | Audience::Professor, Format::Pdf)
            | (Audience::EvaluationItems, Format::Csv) => {}
            (Audience::EvaluationItems, Format::Pdf) => {
                return Err("--evaluation-items currently supports --csv".into());
            }
            (_, Format::Csv) => return Err("--csv is supported with --evaluation-items".into()),
        }
        (audience, format)
    };
    if output_dir.is_some() && format != Format::Pdf && !all_reports {
        return Err("--output-dir can only be used with --pdf".into());
    }
    Ok(Options {
        audience,
        format,
        all_reports,
        run_id,
        output_dir,
    })
}

fn all_output_directory(
    requested: Option<PathBuf>,
    run_id: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let directory = requested.unwrap_or_else(|| {
        let run_component = safe_filename(run_id);
        PathBuf::from("grades").join(if run_component.is_empty() {
            "run".into()
        } else {
            run_component
        })
    });
    fs::create_dir_all(&directory)?;
    Ok(directory)
}

async fn generate_student_pdfs(
    client: &ApiClient,
    reports: &[Value],
    project_name: &str,
    run_id: &str,
    directory: &std::path::Path,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    fs::create_dir_all(directory)?;
    let mut paths = Vec::new();
    let mut used_names = BTreeSet::new();
    for report in reports {
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
        let artifact_list_path = format!(
            "/api/v1/expedients/{}/artifacts",
            encode_path_segment(string(&report["expedient_id"]).as_str())
        );
        let artifacts = match client.request(Method::GET, &artifact_list_path, None).await {
            Ok(value) => fetch_student_artifacts(client, &artifact_list_path, &value).await,
            Err(error) => {
                eprintln!("Warning: could not list artifacts for {label}: {error}");
                Vec::new()
            }
        };
        let sidecar_name = format!("{filename}_artifacts");
        let sidecar_dir = directory.join(&sidecar_name);
        let mut pdf = PdfDocument::new(595.0, 842.0);
        render_student_report(
            &mut pdf,
            report,
            project_name,
            run_id,
            &artifacts,
            &sidecar_dir,
            &sidecar_name,
        )?;
        fs::write(&path, pdf.finish())?;
        paths.push(path);
    }
    Ok(paths)
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

async fn fetch_student_artifacts(
    client: &ApiClient,
    list_path: &str,
    value: &Value,
) -> Vec<StudentArtifact> {
    let Some(rows) = value.as_array() else {
        eprintln!("Warning: thotd returned an invalid expedient artifact list");
        return Vec::new();
    };
    let base = list_path.trim_end_matches("/artifacts");
    let mut artifacts = Vec::new();
    for row in rows {
        let (Some(test_id), Some(path), Some(relative_path), Some(file_name)) = (
            row["test_id"].as_str(),
            row["path"].as_str(),
            row["relative_path"].as_str(),
            row["file_name"].as_str(),
        ) else {
            continue;
        };
        let encoded_path = path
            .split('/')
            .map(encode_path_segment)
            .collect::<Vec<_>>()
            .join("/");
        let url = format!("{base}/artifacts/{encoded_path}");
        match client.request_bytes(&url).await {
            Ok(bytes) => artifacts.push(StudentArtifact {
                test_id: test_id.to_owned(),
                path: path.to_owned(),
                relative_path: relative_path.to_owned(),
                file_name: file_name.to_owned(),
                content_type: row["content_type"]
                    .as_str()
                    .unwrap_or("application/octet-stream")
                    .to_owned(),
                bytes,
            }),
            Err(error) => eprintln!("Warning: could not download test artifact {path}: {error}"),
        }
    }
    artifacts
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
    io::stdout().write_all(&evaluation_csv(reports)?)?;
    Ok(())
}

fn evaluation_csv(reports: &[Value]) -> Result<Vec<u8>, Box<dyn Error>> {
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
    Ok(output)
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

fn render_student_report(
    pdf: &mut PdfDocument,
    report: &Value,
    project: &str,
    run_id: &str,
    artifacts: &[StudentArtifact],
    sidecar_dir: &std::path::Path,
    sidecar_name: &str,
) -> Result<(), Box<dyn Error>> {
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
    let eval_rows = evaluations
        .iter()
        .map(|item| {
            let name = string(&item["name"]);
            vec![
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
            &["Evaluation item", "Description", "Mark"],
            &eval_rows,
            &[150.0, 320.0, 55.0],
        );
    }

    let rubric_rows = array(&report["rubrics"]);
    let check_rows = array(&report["checks"]);
    let check_codes = codes(
        "C",
        check_rows.iter().filter_map(|row| row["name"].as_str()),
    );
    pdf.heading("Rubric results by evaluation item");
    let mut found_rubric = false;
    for item in evaluations {
        let item_name = string(&item["name"]);
        pdf.subheading(&name_description(&item_name, &item["description"]));
        let mut rows = Vec::new();
        for ref_row in array(&item["rubric_results"]) {
            let name = string(&ref_row["name"]);
            let rubric = rubric_rows
                .iter()
                .find(|row| row["name"].as_str() == Some(&name));
            rows.push(vec![
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
                &["Rubric", "Description", "Mark"],
                &rows,
                &[150.0, 320.0, 55.0],
            );
        }
    }
    if !found_rubric {
        pdf.text("No rubric marks are available.");
    }

    pdf.section_break();
    pdf.heading("How rubric marks were calculated");
    let mut abbreviated_checks = BTreeSet::new();
    for item in evaluations {
        let ei_name = string(&item["name"]);
        let mut check_names = Vec::new();
        for rubric_ref in array(&item["rubric_results"]) {
            let r_name = string(&rubric_ref["name"]);
            if let Some(rubric) = rubric_rows
                .iter()
                .find(|row| row["name"].as_str() == Some(&r_name))
            {
                for check in array(&rubric["check_results"]) {
                    let check_name = string(&check["name"]);
                    if !check_names.contains(&check_name) {
                        check_names.push(check_name);
                    }
                }
            }
        }
        if check_names.is_empty() {
            continue;
        }
        let use_names = full_column_names_fit(&check_names, 245.0, pdf.width);
        if !use_names {
            abbreviated_checks.extend(check_names.iter().cloned());
        }
        pdf.subheading(&name_description(&ei_name, &item["description"]));
        let mut matrix_rows = Vec::new();
        for rubric_ref in array(&item["rubric_results"]) {
            let r_name = string(&rubric_ref["name"]);
            let rubric = rubric_rows
                .iter()
                .find(|row| row["name"].as_str() == Some(&r_name));
            let Some(rubric) = rubric else { continue };
            let mut row = vec![r_name, display_mark(&rubric_ref["result"])];
            row.extend(check_names.iter().map(|check_name| {
                array(&rubric["check_results"])
                    .iter()
                    .find(|check| check["name"].as_str() == Some(check_name))
                    .map(|check| check_symbol(&check["result"]))
                    .unwrap_or_default()
            }));
            matrix_rows.push(row);
        }
        let mut headers = vec!["Rubric".to_owned(), "Rubric mark".to_owned()];
        headers.extend(check_names.iter().map(|name| {
            if use_names {
                name.clone()
            } else {
                check_codes
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone())
            }
        }));
        pdf.table_dynamic(&headers, &matrix_rows);
    }

    pdf.section_break();
    pdf.heading("Code guide");
    let check_key = check_rows
        .iter()
        .filter(|check| abbreviated_checks.contains(check["name"].as_str().unwrap_or_default()))
        .map(|check| {
            let name = string(&check["name"]);
            (
                check_codes.get(&name).cloned().unwrap_or_default(),
                name_description(&name, &check["description"]),
            )
        })
        .collect::<Vec<_>>();
    write_code_key(pdf, "Check reference", &check_key, 3);
    pdf.text("v means the check passed; X means it did not pass. A blank rubric mark means no result was recorded.");

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
        for artifact in artifacts
            .iter()
            .filter(|artifact| artifact.test_id == test_id)
        {
            pdf.subheading(&format!("Test artifact: {}", artifact.file_name));
            if is_image_artifact(artifact) && pdf.add_image(&artifact.bytes) {
                continue;
            }
            if is_text_artifact(artifact) {
                let content = String::from_utf8_lossy(&artifact.bytes);
                pdf.source_code_block(&artifact.file_name, &content);
            }
            let mut destination = sidecar_dir.join(safe_filename(&artifact.test_id));
            let safe_parts = artifact
                .relative_path
                .split('/')
                .filter(|part| !part.is_empty())
                .map(safe_filename)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            for part in &safe_parts {
                destination.push(part);
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&destination, &artifact.bytes)?;
            let relative_uri = std::iter::once(sidecar_name.to_owned())
                .chain(std::iter::once(safe_filename(&artifact.test_id)))
                .chain(safe_parts)
                .map(|part| encode_path_segment(&part))
                .collect::<Vec<_>>()
                .join("/");
            pdf.link_text(&artifact.path, &relative_uri);
        }
    }
    render_student_calculation_guide(pdf, report);
    Ok(())
}

fn render_student_calculation_guide(pdf: &mut PdfDocument, report: &Value) {
    pdf.section_break();
    pdf.heading("How final marks are calculated");
    pdf.text("Tests provide evidence. Checks turn test results into pass/fail marks. Rubrics calculate marks from checks, then evaluation items combine rubric marks into the final grades.");

    let plan = &report["grading_plan"];
    let tests = array(&plan["tests"]);
    let configured_checks = array(&plan["checks"]);
    let test_results = array(&report["test_results"]);
    let check_results = array(&report["checks"]);
    let mut checks_seen = BTreeSet::new();
    let mut pipeline_rows = Vec::new();
    for check in configured_checks {
        let name = string(&check["name"]);
        checks_seen.insert(name.clone());
        let test_name = string(&check["test"]);
        let test = tests
            .iter()
            .find(|test| test["name"].as_str() == Some(test_name.as_str()));
        let test_label = if let Some(test) = test {
            let description = string(&test["description"]);
            if description.is_empty() {
                format!("{} ({})", test_name, string(&test["test"]))
            } else {
                format!("{} ({}) — {description}", test_name, string(&test["test"]))
            }
        } else if check["check"] == "import/csv" {
            "CSV input".to_owned()
        } else if check["check"] == "aggr/checks" {
            "Other check results".to_owned()
        } else {
            test_name.clone()
        };
        let matching_tests = test_results
            .iter()
            .filter(|test| {
                test["test_id"]
                    .as_str()
                    .is_some_and(|id| id == test_name || id.starts_with(&format!("{test_name}_")))
            })
            .collect::<Vec<_>>();
        let test_status = if matching_tests.is_empty() {
            "not recorded".to_owned()
        } else if test.is_some_and(|test| test["repeat"].as_u64().unwrap_or(1) > 1) {
            let passed = matching_tests
                .iter()
                .filter(|test| test["status"].as_str() == Some("passed"))
                .count();
            format!("{passed}/{} repetitions passed", matching_tests.len())
        } else {
            friendly_status(matching_tests[0]["status"].as_str().unwrap_or("not_run")).to_owned()
        };
        let result = find_result(&report["checks"], &name)
            .map(|row| check_symbol(&row["result"]))
            .unwrap_or_else(|| "—".to_owned());
        let criterion = check_criterion(check);
        pipeline_rows.push(vec![
            test_label,
            format!("{} — {}", name, string(&check["description"])),
            criterion,
            format!("Test: {test_status}; check: {result}"),
        ]);
    }
    for check in check_results {
        let name = string(&check["name"]);
        if checks_seen.insert(name.clone()) {
            pipeline_rows.push(vec![
                "Imported or generated".to_owned(),
                format!("{} — {}", name, string(&check["description"])),
                "Result was imported or combined from other checks".to_owned(),
                check_symbol(&check["result"]),
            ]);
        }
    }
    pdf.subheading("Test evidence and checks");
    if pipeline_rows.is_empty() {
        pdf.text("No check definitions are available in this saved expedient.");
    } else {
        pdf.table_dynamic(
            &[
                "Test evidence".to_owned(),
                "Check".to_owned(),
                "Rule".to_owned(),
                "Outcome".to_owned(),
            ],
            &pipeline_rows,
        );
    }

    let multiplier = plan["rubrics_multiplier"]
        .as_f64()
        .or_else(|| report["evaluation"]["rubrics_multiplier"].as_f64())
        .unwrap_or(1.0);
    let mut rubric_rows = Vec::new();
    let mut rubric_names = BTreeSet::new();
    for rubric in calculation_definitions(plan, "rubrics", &report["rubrics"]) {
        let name = string(&rubric["name"]);
        rubric_names.insert(name.clone());
        let checks = string_array(&rubric["checks"]);
        let row = find_result(&report["rubrics"], &name);
        let weights = numeric_array(row.map(|row| &row["weights"]).unwrap_or(&rubric["weights"]));
        let formula = calculation_formula(
            &string(&rubric["calculation"]),
            &checks,
            &weights,
            Some(multiplier),
            false,
        );
        rubric_rows.push(vec![
            format!("{} — {}", name, string(&rubric["description"])),
            string(&rubric["calculation"]),
            formula,
            display_optional_mark(row.map(|row| &row["result"])),
        ]);
    }
    for rubric in array(&report["rubrics"]) {
        let name = string(&rubric["name"]);
        if !rubric_names.insert(name.clone()) {
            continue;
        }
        rubric_rows.push(vec![
            format!("{} — {}", name, string(&rubric["description"])),
            string(&rubric["calculation"]),
            calculation_formula(
                &string(&rubric["calculation"]),
                &string_array(&rubric["checks"]),
                &numeric_array(&rubric["weights"]),
                Some(multiplier),
                false,
            ),
            display_optional_mark(Some(&rubric["result"])),
        ]);
    }
    pdf.subheading("Rubric marks from check outcomes");
    if rubric_rows.is_empty() {
        pdf.text("No rubric calculation definitions are available.");
    } else {
        pdf.table_dynamic(
            &[
                "Rubric".to_owned(),
                "Calculation".to_owned(),
                "Inputs and weights".to_owned(),
                "Mark".to_owned(),
            ],
            &rubric_rows,
        );
    }

    let eval_multiplier = plan["evaluation_multiplier"]
        .as_f64()
        .or_else(|| report["evaluation"]["evaluation_multiplier"].as_f64())
        .unwrap_or(1.0);
    let mut evaluation_rows = Vec::new();
    let mut evaluation_names = BTreeSet::new();
    for item in calculation_definitions(plan, "evaluation", &report["evaluation"]["results"]) {
        let name = string(&item["name"]);
        evaluation_names.insert(name.clone());
        let rubrics = string_array(&item["rubrics"]);
        let row = find_result(&report["evaluation"]["results"], &name);
        let weights = numeric_array(row.map(|row| &row["weights"]).unwrap_or(&item["weights"]));
        let formula = if row.is_some_and(|row| row["calculation"] == "not_delivered_mark") {
            format!(
                "No-delivery mark: {}",
                plan["not_delivered_mark"]
                    .as_f64()
                    .map(|mark| display_value(&serde_json::json!(mark)))
                    .unwrap_or_else(|| "not configured".to_owned())
            )
        } else {
            calculation_formula(
                &string(&item["calculation"]),
                &rubrics,
                &weights,
                Some(eval_multiplier),
                true,
            )
        };
        evaluation_rows.push(vec![
            format!("{} — {}", name, string(&item["description"])),
            string(&item["calculation"]),
            formula,
            display_optional_mark(row.map(|row| &row["result"])),
        ]);
    }
    for item in array(&report["evaluation"]["results"]) {
        let name = string(&item["name"]);
        if !evaluation_names.insert(name.clone()) {
            continue;
        }
        let formula = if item["calculation"] == "not_delivered_mark" {
            format!(
                "No-delivery mark: {}",
                plan["not_delivered_mark"]
                    .as_f64()
                    .map(|mark| display_value(&serde_json::json!(mark)))
                    .unwrap_or_else(|| "not configured".to_owned())
            )
        } else {
            calculation_formula(
                &string(&item["calculation"]),
                &string_array(&item["rubrics"]),
                &numeric_array(&item["weights"]),
                Some(eval_multiplier),
                true,
            )
        };
        evaluation_rows.push(vec![
            format!("{} — {}", name, string(&item["description"])),
            string(&item["calculation"]),
            formula,
            display_optional_mark(Some(&item["result"])),
        ]);
    }
    pdf.subheading("Final evaluation items from rubric marks");
    if evaluation_rows.is_empty() {
        pdf.text("No final evaluation definitions are available.");
    } else {
        pdf.table_dynamic(
            &[
                "Evaluation item".to_owned(),
                "Calculation".to_owned(),
                "Inputs and weights".to_owned(),
                "Final mark".to_owned(),
            ],
            &evaluation_rows,
        );
    }
    pdf.text("A v means a check passed; X means it did not pass. A blank mark means it could not be calculated or was not recorded. Weighted averages divide the weighted sum by the sum of the listed weights.");
}

fn is_image_artifact(artifact: &StudentArtifact) -> bool {
    artifact.content_type.starts_with("image/")
        || matches!(
            extension(&artifact.file_name).as_str(),
            "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "bmp"
                | "tif"
                | "tiff"
                | "ico"
                | "tga"
                | "pnm"
                | "ppm"
                | "pgm"
                | "pbm"
                | "qoi"
                | "webp"
        )
}

fn is_text_artifact(artifact: &StudentArtifact) -> bool {
    artifact.content_type.starts_with("text/")
        || matches!(
            extension(&artifact.file_name).as_str(),
            "txt"
                | "log"
                | "html"
                | "htm"
                | "py"
                | "js"
                | "ts"
                | "rs"
                | "sh"
                | "bash"
                | "css"
                | "json"
                | "yaml"
                | "yml"
                | "csv"
                | "md"
                | "xml"
                | "conf"
                | "ini"
                | "sql"
                | "java"
                | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "toml"
                | "go"
                | "rb"
                | "php"
                | "pl"
                | "lua"
                | "properties"
                | "kt"
                | "swift"
                | "cs"
                | "dockerfile"
                | "makefile"
        )
        || looks_like_text(&artifact.bytes)
}

fn looks_like_text(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(8192)];
    if sample.is_empty() || std::str::from_utf8(sample).is_err() || sample.contains(&0) {
        return false;
    }
    let non_text_controls = sample
        .iter()
        .filter(|byte| **byte < 0x20 && !matches!(**byte, b'\n' | b'\r' | b'\t' | 0x0c))
        .count();
    non_text_controls * 100 <= sample.len()
}

fn extension(file_name: &str) -> String {
    std::path::Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn render_professor_report(pdf: &mut PdfDocument, reports: &[Value], project: &str, run_id: &str) {
    let evaluation_names = union_names(reports, |report| &report["evaluation"]["results"]);
    let rubric_names = union_names(reports, |report| &report["rubrics"]);
    let check_names = union_names(reports, |report| &report["checks"]);
    let test_names = union_names(reports, |report| &report["test_results"]);
    let ei_codes = codes("EI", evaluation_names.iter().map(String::as_str));
    let rubric_codes = codes("R", rubric_names.iter().map(String::as_str));
    let check_codes = codes("C", check_names.iter().map(String::as_str));
    let mut abbreviated_evaluations = BTreeSet::new();
    let mut abbreviated_rubrics = BTreeSet::new();
    let mut abbreviated_checks = BTreeSet::new();

    pdf.heading("Professor grade summary");
    pdf.text(&format!("Project: {project}    Run: {run_id}"));
    pdf.text(&format!("Students included: {}", reports.len()));
    pdf.heading("Final grades by evaluation item");
    let mut headers = vec!["Student".to_owned(), "External ID".to_owned()];
    let use_evaluation_names = full_column_names_fit(&evaluation_names, 230.0, pdf.width);
    if !use_evaluation_names {
        abbreviated_evaluations.extend(evaluation_names.iter().cloned());
    }
    headers.extend(evaluation_names.iter().map(|name| {
        if use_evaluation_names {
            name.clone()
        } else {
            ei_codes[name].clone()
        }
    }));
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
        let evaluation_description = reports
            .iter()
            .find_map(|report| {
                find_result(&report["evaluation"]["results"], evaluation_name)
                    .and_then(|item| item["description"].as_str())
            })
            .unwrap_or_default();
        pdf.subheading(&name_description_value(
            evaluation_name,
            evaluation_description,
        ));
        let use_rubric_names = full_column_names_fit(&rubric_order, 230.0, pdf.width);
        if !use_rubric_names {
            abbreviated_rubrics.extend(rubric_order.iter().cloned());
        }
        let mut headers = vec!["Student".to_owned(), "External ID".to_owned()];
        headers.extend(rubric_order.iter().map(|name| {
            if use_rubric_names {
                name.clone()
            } else {
                rubric_codes
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone())
            }
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
        let rubric_description = reports
            .iter()
            .find_map(|report| {
                find_result(&report["rubrics"], rubric_name)
                    .and_then(|rubric| rubric["description"].as_str())
            })
            .unwrap_or_default();
        pdf.subheading(&name_description_value(rubric_name, rubric_description));
        let use_check_names = full_column_names_fit(&check_order, 230.0, pdf.width);
        if !use_check_names {
            abbreviated_checks.extend(check_order.iter().cloned());
        }
        let mut headers = vec!["Student".to_owned(), "External ID".to_owned()];
        headers.extend(check_order.iter().map(|name| {
            if use_check_names {
                name.clone()
            } else {
                check_codes
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone())
            }
        }));
        let rows = reports
            .iter()
            .map(|report| {
                let mut row = student_columns(report);
                let rubric = find_result(&report["rubrics"], rubric_name);
                row.extend(check_order.iter().map(|name| {
                    rubric
                        .and_then(|rubric| find_result(&rubric["check_results"], name))
                        .map(|check| check_symbol(&check["result"]))
                        .or_else(|| {
                            find_result(&report["checks"], name)
                                .map(|check| check_symbol(&check["result"]))
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
    write_code_key(
        pdf,
        "Evaluation item references",
        &code_key_entries(&abbreviated_evaluations, &ei_codes, reports, "evaluation"),
        4,
    );
    write_code_key(
        pdf,
        "Rubric references",
        &code_key_entries(&abbreviated_rubrics, &rubric_codes, reports, "rubrics"),
        4,
    );
    write_code_key(
        pdf,
        "Check references",
        &code_key_entries(&abbreviated_checks, &check_codes, reports, "checks"),
        4,
    );
    pdf.text("v means a check passed; X means it did not pass. Pass means a test completed successfully; No pass includes test failures and errors.");
    pdf.section_break();
    render_professor_calculation_guide(pdf, reports);
}

fn render_professor_calculation_guide(pdf: &mut PdfDocument, reports: &[Value]) {
    pdf.heading("How final marks are calculated");
    pdf.text("Tests produce evidence for checks. Check outcomes are combined into rubric marks; evaluation items combine rubric marks into final grades. These tables show the configured inputs, calculations, and weights.");
    let empty = Value::Null;
    let plan = reports
        .iter()
        .find(|report| report["grading_plan"].is_object())
        .map(|report| &report["grading_plan"])
        .unwrap_or(&empty);
    let tests = array(&plan["tests"]);
    let checks = array(&plan["checks"]);
    let mut check_rows = Vec::new();
    for check in checks {
        let test_name = string(&check["test"]);
        let test = tests
            .iter()
            .find(|test| test["name"].as_str() == Some(test_name.as_str()));
        let test_label = test
            .map(|test| {
                let description = string(&test["description"]);
                if description.is_empty() {
                    format!("{} ({})", test_name, string(&test["test"]))
                } else {
                    format!("{} ({}) — {description}", test_name, string(&test["test"]))
                }
            })
            .unwrap_or_else(|| match check["check"].as_str().unwrap_or("") {
                "import/csv" => "CSV input".to_owned(),
                "aggr/checks" => "Other check results".to_owned(),
                _ => test_name,
            });
        let check_name = string(&check["name"]);
        let evaluated_rows = reports
            .iter()
            .filter_map(|report| find_result(&report["checks"], &check_name))
            .collect::<Vec<_>>();
        let pass_count = evaluated_rows
            .iter()
            .filter(|row| row["result"].as_i64() == Some(1))
            .count();
        check_rows.push(vec![
            test_label,
            format!("{} — {}", check_name, string(&check["description"])),
            check_criterion(check),
            format!("{pass_count}/{} passed", evaluated_rows.len()),
        ]);
    }
    let mut check_names = checks
        .iter()
        .filter_map(|check| check["name"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for check in reports.iter().flat_map(|report| array(&report["checks"])) {
        let name = string(&check["name"]);
        if !check_names.insert(name.clone()) {
            continue;
        }
        let evaluated_rows = reports
            .iter()
            .filter_map(|report| find_result(&report["checks"], &name))
            .collect::<Vec<_>>();
        let pass_count = evaluated_rows
            .iter()
            .filter(|row| row["result"].as_i64() == Some(1))
            .count();
        check_rows.push(vec![
            "CSV or aggregate result".to_owned(),
            format!("{} — {}", name, string(&check["description"])),
            "Imported or combined check outcome".to_owned(),
            format!("{pass_count}/{} passed", evaluated_rows.len()),
        ]);
    }
    pdf.subheading("Test evidence feeding checks");
    if check_rows.is_empty() {
        pdf.text("This saved run does not contain definitions linking tests to checks.");
    } else {
        pdf.table_dynamic(
            &[
                "Test".into(),
                "Check".into(),
                "Rule".into(),
                "Outcomes".into(),
            ],
            &check_rows,
        );
    }

    let rubric_multiplier = plan["rubrics_multiplier"].as_f64().unwrap_or(1.0);
    let rubric_fallback = reports
        .first()
        .map(|report| &report["rubrics"])
        .unwrap_or(&empty);
    let mut rubric_rows = Vec::new();
    let mut rubric_names = BTreeSet::new();
    for rubric in calculation_definitions(plan, "rubrics", rubric_fallback) {
        let name = string(&rubric["name"]);
        rubric_names.insert(name.clone());
        let inputs = string_array(&rubric["checks"]);
        let formula = calculation_formula(
            &string(&rubric["calculation"]),
            &inputs,
            &numeric_array(&rubric["weights"]),
            Some(rubric_multiplier),
            false,
        );
        rubric_rows.push(vec![
            format!("{} — {}", name, string(&rubric["description"])),
            formula,
        ]);
    }
    for rubric in reports.iter().flat_map(|report| array(&report["rubrics"])) {
        let name = string(&rubric["name"]);
        if !rubric_names.insert(name.clone()) {
            continue;
        }
        rubric_rows.push(vec![
            format!("{} — {}", name, string(&rubric["description"])),
            calculation_formula(
                &string(&rubric["calculation"]),
                &string_array(&rubric["checks"]),
                &numeric_array(&rubric["weights"]),
                Some(rubric_multiplier),
                false,
            ),
        ]);
    }
    pdf.subheading("Rubric formulas");
    if rubric_rows.is_empty() {
        pdf.text("No rubric calculation definitions are available in this run.");
    } else {
        pdf.table_dynamic(
            &["Rubric".into(), "Calculation, inputs, and weights".into()],
            &rubric_rows,
        );
        pdf.text(&format!(
            "Rubric multiplier: {}. It scales each check result before calculation.",
            display_value(&plan["rubrics_multiplier"])
        ));
    }

    let evaluation_multiplier = plan["evaluation_multiplier"].as_f64().unwrap_or(1.0);
    let evaluation_fallback = reports
        .first()
        .map(|report| &report["evaluation"]["results"])
        .unwrap_or(&empty);
    let mut evaluation_rows = Vec::new();
    let mut evaluation_names = BTreeSet::new();
    for item in calculation_definitions(plan, "evaluation", evaluation_fallback) {
        let name = string(&item["name"]);
        evaluation_names.insert(name.clone());
        let formula = calculation_formula(
            &string(&item["calculation"]),
            &string_array(&item["rubrics"]),
            &numeric_array(&item["weights"]),
            Some(evaluation_multiplier),
            true,
        );
        evaluation_rows.push(vec![
            format!("{} — {}", name, string(&item["description"])),
            formula,
        ]);
    }
    for item in reports
        .iter()
        .flat_map(|report| array(&report["evaluation"]["results"]))
    {
        let name = string(&item["name"]);
        if !evaluation_names.insert(name.clone()) {
            continue;
        }
        let formula = if item["calculation"] == "not_delivered_mark" {
            format!(
                "No-delivery mark: {}",
                plan["not_delivered_mark"]
                    .as_f64()
                    .map(|mark| display_value(&serde_json::json!(mark)))
                    .unwrap_or_else(|| "not configured".to_owned())
            )
        } else {
            calculation_formula(
                &string(&item["calculation"]),
                &string_array(&item["rubrics"]),
                &numeric_array(&item["weights"]),
                Some(evaluation_multiplier),
                true,
            )
        };
        evaluation_rows.push(vec![
            format!("{} — {}", name, string(&item["description"])),
            formula,
        ]);
    }
    pdf.subheading("Final evaluation formulas");
    if evaluation_rows.is_empty() {
        pdf.text("No evaluation calculation definitions are available in this run.");
    } else {
        pdf.table_dynamic(
            &[
                "Evaluation item".into(),
                "Calculation, inputs, and weights".into(),
            ],
            &evaluation_rows,
        );
        pdf.text(&format!(
            "Evaluation multiplier: {}. Weighted averages divide the weighted sum by the sum of the listed weights.",
            display_value(&plan["evaluation_multiplier"])
        ));
        pdf.text(&format!(
            "Students without a delivery receive the configured no-delivery mark ({}) instead of calculated marks; their tests, checks, and rubrics remain blank.",
            plan["not_delivered_mark"]
                .as_f64()
                .map(|mark| display_value(&serde_json::json!(mark)))
                .unwrap_or_else(|| "none".to_owned())
        ));
    }
}

fn calculation_definitions(plan: &Value, key: &str, fallback: &Value) -> Vec<Value> {
    let configured = array(&plan[key]);
    if configured.is_empty() {
        array(fallback).to_vec()
    } else {
        configured.to_vec()
    }
}

fn string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn numeric_array(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .collect()
}

fn display_optional_mark(value: Option<&Value>) -> String {
    value
        .filter(|value| !value.is_null())
        .map(display_mark)
        .unwrap_or_else(|| "—".to_owned())
}

fn calculation_formula(
    calculation: &str,
    inputs: &[String],
    weights: &[f64],
    multiplier: Option<f64>,
    multiplier_after: bool,
) -> String {
    if calculation == "import/csv" {
        return "Imported from CSV".to_owned();
    }
    if inputs.is_empty() {
        return "No inputs are configured".to_owned();
    }
    let arguments = inputs
        .iter()
        .map(|input| {
            if let Some(multiplier) = multiplier.filter(|_| !multiplier_after) {
                format!(
                    "({input} × {})",
                    display_value(&serde_json::json!(multiplier))
                )
            } else {
                input.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let weights = if calculation == "weighted_avg" && !weights.is_empty() {
        format!(
            "; weights=[{}]",
            weights
                .iter()
                .map(|weight| display_value(&serde_json::json!(weight)))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        String::new()
    };
    let mut formula = format!("{calculation}({arguments}{weights})");
    if let Some(multiplier) = multiplier.filter(|_| multiplier_after) {
        formula.push_str(&format!(
            " × {}",
            display_value(&serde_json::json!(multiplier))
        ));
    }
    formula
}

fn check_criterion(check: &Value) -> String {
    let check_type = string(&check["check"]);
    let property = check["property"].as_str().unwrap_or_default();
    let details = check["parameters"]
        .as_object()
        .into_iter()
        .flat_map(|parameters| parameters.iter())
        .map(|(key, value)| {
            let value = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            format!("{key}={value}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    let mut summary = check_type;
    if !property.is_empty() {
        summary.push_str(&format!(" on {property}"));
    }
    if !details.is_empty() {
        summary.push_str(&format!(": {details}"));
    }
    summary
}

fn full_column_names_fit(names: &[String], reserved_width: f32, page_width: f32) -> bool {
    let names_width = names
        .iter()
        .map(|name| name.chars().count() as f32 * 3.8 + 10.0)
        .sum::<f32>();
    reserved_width + names_width <= page_width - 72.0
}

fn name_description(name: &str, description: &Value) -> String {
    name_description_value(name, description.as_str().unwrap_or_default())
}

fn name_description_value(name: &str, description: &str) -> String {
    if description.trim().is_empty() {
        name.to_owned()
    } else {
        format!("{name} — {}", description.trim())
    }
}

fn check_symbol(result: &Value) -> &'static str {
    if result.as_i64() == Some(1) || result.as_f64() == Some(1.0) {
        "✓" // Ya no necesitas .to_owned()
    } else {
        "X"
    }
}

fn code_key_entries(
    names: &BTreeSet<String>,
    codes: &BTreeMap<String, String>,
    reports: &[Value],
    category: &str,
) -> Vec<(String, String)> {
    names
        .iter()
        .filter_map(|name| {
            let code = codes.get(name)?.clone();
            let description = reports.iter().find_map(|report| {
                let rows = match category {
                    "evaluation" => &report["evaluation"]["results"],
                    "rubrics" => &report["rubrics"],
                    "checks" => &report["checks"],
                    _ => return None,
                };
                find_result(rows, name)
                    .and_then(|row| row["description"].as_str())
                    .map(str::to_owned)
            });
            Some((
                code,
                name_description_value(name, description.as_deref().unwrap_or_default()),
            ))
        })
        .collect()
}

fn write_code_key(
    pdf: &mut PdfDocument,
    title: &str,
    entries: &[(String, String)],
    columns: usize,
) {
    if entries.is_empty() {
        return;
    }
    pdf.subheading(title);
    for chunk in entries.chunks(columns.max(1)) {
        let headers = chunk
            .iter()
            .map(|(code, _)| code.clone())
            .collect::<Vec<_>>();
        let descriptions = chunk
            .iter()
            .map(|(_, description)| description.clone())
            .collect::<Vec<_>>();
        let widths = vec![(pdf.width - 72.0) / chunk.len() as f32; chunk.len()];
        pdf.table_with_widths(&headers, &[descriptions], &widths);
    }
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

struct PdfLink {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    uri: String,
}

struct PdfImage {
    name: String,
    width: u32,
    height: u32,
    data: Vec<u8>,
}

struct PdfDocument {
    width: f32,
    height: f32,
    pages: Vec<Vec<String>>,
    page_links: Vec<Vec<PdfLink>>,
    images: Vec<PdfImage>,
    y: f32,
}

impl PdfDocument {
    fn new(width: f32, height: f32) -> Self {
        let mut document = Self {
            width,
            height,
            pages: vec![Vec::new()],
            page_links: vec![Vec::new()],
            images: Vec::new(),
            y: 44.0,
        };
        document.line(36.0, 28.0, width - 36.0, 28.0);
        document
    }

    fn page(&mut self) {
        self.pages.push(Vec::new());
        self.page_links.push(Vec::new());
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
            self.draw_text_font(43.0, self.y, 7.3, "F3", &line);
            self.y += 9.0;
        }
    }

    fn source_code_block(&mut self, file_name: &str, source: &str) {
        self.text(&format!("File: {file_name}"));
        const DISPLAY_LIMIT: usize = 48 * 1024;
        let mut source = source.to_owned();
        let was_truncated = source.len() > DISPLAY_LIMIT;
        if was_truncated {
            let mut boundary = DISPLAY_LIMIT;
            while !source.is_char_boundary(boundary) {
                boundary -= 1;
            }
            source.truncate(boundary);
        }
        let lines = source
            .lines()
            .flat_map(|line| wrap_text(line, 112))
            .collect::<Vec<_>>();
        let was_line_truncated = lines.len() > 1400;
        for line in lines.into_iter().take(1400) {
            self.ensure(11.0);
            self.pages.last_mut().unwrap().push(format!(
                "q 0.94 g 40 {:.1} {:.1} 10 re f Q\n",
                self.height - self.y - 2.0,
                self.width - 80.0
            ));
            self.draw_text_font(44.0, self.y + 5.5, 7.0, "F3", &line);
            self.y += 10.0;
        }
        if was_truncated || was_line_truncated {
            self.text("[File shortened to fit the student report]");
        }
    }

    fn add_image(&mut self, encoded: &[u8]) -> bool {
        if encoded.len() > 32 * 1024 * 1024 {
            return false;
        }
        let mut limits = Limits::default();
        limits.max_image_width = Some(6000);
        limits.max_image_height = Some(6000);
        limits.max_alloc = Some(64 * 1024 * 1024);
        let mut reader = match ImageReader::new(Cursor::new(encoded)).with_guessed_format() {
            Ok(reader) => reader,
            Err(_) => return false,
        };
        reader.limits(limits);
        let image = match reader.decode() {
            Ok(image) => image.to_rgb8(),
            Err(_) => return false,
        };
        let width = image.width();
        let height = image.height();
        if width == 0 || height == 0 {
            return false;
        }
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        if encoder.write_all(image.as_raw()).is_err() {
            return false;
        }
        let Ok(data) = encoder.finish() else {
            return false;
        };
        let max_width = self.width - 72.0;
        let max_height = (self.height - 140.0).min(380.0);
        let scale = (max_width / width as f32)
            .min(max_height / height as f32)
            .min(1.0);
        let draw_width = width as f32 * scale;
        let draw_height = height as f32 * scale;
        self.ensure(draw_height + 18.0);
        let name = format!("Im{}", self.images.len());
        let x = (self.width - draw_width) / 2.0;
        let y = self.height - self.y - draw_height;
        self.pages.last_mut().unwrap().push(format!(
            "q {draw_width:.2} 0 0 {draw_height:.2} {x:.2} {y:.2} cm /{name} Do Q\n"
        ));
        self.images.push(PdfImage {
            name,
            width,
            height,
            data,
        });
        self.y += draw_height + 12.0;
        true
    }

    fn link_text(&mut self, text: &str, uri: &str) {
        for line in wrap_text(text, 105) {
            self.ensure(15.0);
            let y_top = self.y;
            let baseline = self.height - y_top;
            let x = 40.0;
            self.pages.last_mut().unwrap().push(format!(
                "0.05 0.25 0.65 rg BT /F1 8.5 Tf 1 0 0 1 {x:.1} {baseline:.1} Tm ({}) Tj ET 0 0 0 rg\n",
                pdf_literal(&line)
            ));
            let width = (line.chars().count() as f32 * 4.6).min(self.width - 80.0);
            self.page_links.last_mut().unwrap().push(PdfLink {
                x1: x,
                y1: self.height - y_top - 3.0,
                x2: x + width,
                y2: self.height - y_top + 10.0,
                uri: uri.to_owned(),
            });
            self.y += 13.0;
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
        self.draw_text_font(x, y_from_top, size, font, value);
    }

    fn draw_text_font(&mut self, x: f32, y_from_top: f32, size: f32, font: &str, value: &str) {
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
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".to_vec(),
        ];
        let mut image_resources = Vec::new();
        for image in &self.images {
            let object_id = objects.len() + 1;
            image_resources.push(format!("/{} {object_id} 0 R", image.name));
            let mut object = format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
                image.width,
                image.height,
                image.data.len()
            )
            .into_bytes();
            object.extend_from_slice(&image.data);
            object.extend_from_slice(b"\nendstream");
            objects.push(object);
        }
        let mut kids = Vec::new();
        for (index, page) in self.pages.into_iter().enumerate() {
            let page_object = objects.len() + 1;
            let stream_object = page_object + 1;
            kids.push(format!("{page_object} 0 R"));
            let content = page.concat().into_bytes();
            let annotation_start = stream_object + 1;
            let annotation_ids = self.page_links[index]
                .iter()
                .enumerate()
                .map(|(offset, _)| format!("{} 0 R", annotation_start + offset))
                .collect::<Vec<_>>();
            let annotations = if annotation_ids.is_empty() {
                String::new()
            } else {
                format!(" /Annots [{}]", annotation_ids.join(" "))
            };
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.1} {:.1}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> /XObject << {} >> >> /Contents {stream_object} 0 R{annotations} >>", self.width, self.height, image_resources.join(" ")).into_bytes());
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(&content);
            stream.extend_from_slice(b"endstream");
            objects.push(stream);
            for link in &self.page_links[index] {
                objects.push(format!(
                    "<< /Type /Annot /Subtype /Link /Rect [{:.1} {:.1} {:.1} {:.1}] /Border [0 0 0] /A << /S /URI /URI ({}) >> >>",
                    link.x1,
                    link.y1,
                    link.x2,
                    link.y2,
                    pdf_literal(&link.uri)
                ).into_bytes());
            }
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
