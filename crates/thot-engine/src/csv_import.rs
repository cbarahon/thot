//! Purpose: Loads safe project-relative CSV data used by imported tests and marks.
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Component, Path},
};
use thot_core::{ProjectCheck, ProjectEvaluation, ProjectRubric, ProjectTest, VMActivityProject};

#[derive(Debug, Clone)]
pub(crate) struct CsvTable {
    headers: Vec<String>,
    indexes: HashMap<String, usize>,
    rows: Vec<CsvRow>,
}

#[derive(Debug, Clone)]
pub(crate) struct CsvRow {
    pub external_id: String,
    pub values: Vec<String>,
}

impl CsvTable {
    pub(crate) fn index(&self, name: &str) -> Option<usize> {
        self.indexes.get(&normalize_header(name)).copied()
    }

    pub(crate) fn row_for_external_id(&self, external_id: &str) -> Option<&CsvRow> {
        self.rows.iter().find(|row| row.external_id == external_id)
    }
}

pub(crate) fn read_project_csv(root: &Path, filepath: &str) -> Result<CsvTable, String> {
    validate_relative_filepath(filepath)?;
    let root = root
        .canonicalize()
        .map_err(|error| format!("project files are unavailable: {error}"))?;
    let requested = root.join(filepath);
    let metadata = fs::symlink_metadata(&requested)
        .map_err(|error| format!("CSV file {} is unavailable: {error}", requested.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "CSV file must be a regular non-symlink file: {}",
            requested.display()
        ));
    }
    let path = requested
        .canonicalize()
        .map_err(|error| format!("cannot resolve CSV file {}: {error}", requested.display()))?;
    if !path.starts_with(&root) {
        return Err("CSV file resolves outside the project directory".into());
    }
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read CSV file {}: {error}", path.display()))?;
    parse_table(&contents, &path)
}

pub(crate) fn validate_project_files(
    project: &VMActivityProject,
    project_root: &Path,
) -> Result<(), String> {
    for test in &project.tests {
        if test.test_ref == "import/csv" {
            let parameters = test.parameters.into_strings()?;
            let filepath = required_filepath(&parameters, "CSV test")?;
            let table = read_project_csv(project_root, filepath)?;
            require_columns(&table, &["external_id", "stdout", "exit_code"], "CSV test")?;
            for (row_number, row) in table.rows.iter().enumerate() {
                let index = table.index("exit_code").expect("validated required column");
                row.values[index].trim().parse::<i32>().map_err(|_| {
                    format!(
                        "CSV test {:?} row {} has an invalid exit_code {:?}; use a signed integer",
                        test.name,
                        row_number + 2,
                        row.values[index]
                    )
                })?;
            }
        }
    }
    for check in &project.checks {
        if check.check_ref == "import/csv" {
            validate_mark_table(check.parameters.into_strings()?, project_root, "CSV check")?;
        }
    }
    for rubric in &project.rubrics {
        if rubric.calculation == "import/csv" {
            validate_mark_table(
                rubric.parameters.into_strings()?,
                project_root,
                "CSV rubric",
            )?;
        }
    }
    for item in &project.evaluation {
        if item.calculation == "import/csv" {
            validate_mark_table(
                item.parameters.into_strings()?,
                project_root,
                "CSV evaluation",
            )?;
        }
    }

    let mut available_checks = project
        .checks
        .iter()
        .filter(|check| check.check_ref != "import/csv")
        .map(|check| check.name.clone())
        .collect::<std::collections::HashSet<_>>();
    for check in project
        .checks
        .iter()
        .filter(|check| check.check_ref == "import/csv")
    {
        available_checks.extend(mark_table_columns(
            check.parameters.into_strings()?,
            project_root,
            "CSV check",
        )?);
    }
    for rubric in project
        .rubrics
        .iter()
        .filter(|rubric| rubric.calculation != "import/csv")
    {
        for check_name in &rubric.checks {
            if !available_checks.contains(check_name) {
                return Err(format!(
                    "rubric {:?} references check {:?}, which is not defined by the project or a check CSV header",
                    rubric.name, check_name
                ));
            }
        }
    }

    let mut available_rubrics = project
        .rubrics
        .iter()
        .filter(|rubric| rubric.calculation != "import/csv")
        .map(|rubric| rubric.name.clone())
        .collect::<std::collections::HashSet<_>>();
    for rubric in project
        .rubrics
        .iter()
        .filter(|rubric| rubric.calculation == "import/csv")
    {
        available_rubrics.extend(mark_table_columns(
            rubric.parameters.into_strings()?,
            project_root,
            "CSV rubric",
        )?);
    }
    for item in project
        .evaluation
        .iter()
        .filter(|item| item.calculation != "import/csv")
    {
        for rubric_name in &item.rubrics {
            if !available_rubrics.contains(rubric_name) {
                return Err(format!(
                    "evaluation item {:?} references rubric {:?}, which is not defined by the project or a rubric CSV header",
                    item.name, rubric_name
                ));
            }
        }
    }
    Ok(())
}

fn validate_mark_table(
    parameters: std::collections::BTreeMap<String, String>,
    project_root: &Path,
    label: &str,
) -> Result<(), String> {
    mark_table_columns(parameters, project_root, label).map(|_| ())
}

fn mark_table_columns(
    parameters: std::collections::BTreeMap<String, String>,
    project_root: &Path,
    label: &str,
) -> Result<Vec<String>, String> {
    let filepath = required_filepath(&parameters, label)?;
    let table = read_project_csv(project_root, filepath)?;
    require_external_id(&table, label)?;
    let result_columns = table
        .headers
        .iter()
        .filter(|header| normalize_header(header) != "external_id")
        .collect::<Vec<_>>();
    if result_columns.is_empty() {
        return Err(format!(
            "{label} CSV must contain at least one result column"
        ));
    }
    for name in &result_columns {
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(format!(
                "{label} result column {name:?} must be a check, rubric, or evaluation name using only letters, digits, underscores, or hyphens"
            ));
        }
    }
    Ok(result_columns.into_iter().cloned().collect())
}

pub(crate) fn imported_test_result(
    project_root: &Path,
    test: &ProjectTest,
    external_id: &str,
) -> Result<Option<ImportedProcessResult>, String> {
    let parameters = test.parameters.into_strings()?;
    let filepath = required_filepath(&parameters, "CSV test")?;
    let table = read_project_csv(project_root, filepath)?;
    require_columns(&table, &["external_id", "stdout", "exit_code"], "CSV test")?;
    let Some(row) = table.row_for_external_id(external_id) else {
        return Ok(None);
    };
    let stdout = row.values[table.index("stdout").expect("validated column")].clone();
    let stderr = table
        .index("stderr")
        .map(|index| row.values[index].clone())
        .unwrap_or_default();
    let exit_code = row.values[table.index("exit_code").expect("validated column")]
        .trim()
        .parse::<i32>()
        .map_err(|_| {
            format!("CSV test row for external_id {external_id:?} has invalid exit_code")
        })?;
    Ok(Some(ImportedProcessResult {
        stdout,
        stderr,
        exit_code,
    }))
}

#[derive(Debug, Clone)]
pub(crate) struct ImportedProcessResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub(crate) fn external_ids_covered_by_import(
    project_root: &Path,
    test: &ProjectTest,
    external_ids: &[String],
) -> Result<bool, String> {
    let parameters = test.parameters.into_strings()?;
    let filepath = required_filepath(&parameters, "CSV test")?;
    let table = read_project_csv(project_root, filepath)?;
    require_columns(&table, &["external_id", "stdout", "exit_code"], "CSV test")?;
    Ok(!external_ids.is_empty()
        && external_ids
            .iter()
            .all(|external_id| table.row_for_external_id(external_id).is_some()))
}

pub(crate) fn tests_to_skip_for_students(
    project: &VMActivityProject,
    project_root: &Path,
    external_ids: &[String],
) -> Result<std::collections::HashSet<String>, String> {
    let mut skipped = std::collections::HashSet::new();
    for (index, importer) in project.tests.iter().enumerate() {
        if importer.test_ref != "import/csv" {
            continue;
        }
        let parameters = importer.parameters.into_strings()?;
        let Some(target_name) = parameters.get("override_test") else {
            continue;
        };
        let target_index = project
            .tests
            .iter()
            .position(|test| test.name == *target_name)
            .unwrap_or(usize::MAX);
        if index < target_index
            && external_ids_covered_by_import(project_root, importer, external_ids)?
        {
            skipped.insert(target_name.clone());
        }
    }
    Ok(skipped)
}

pub(crate) fn apply_test_imports(
    project: &VMActivityProject,
    project_root: &Path,
    external_id: &str,
    report: &mut Value,
) -> Result<(), String> {
    let already_has_errors = report["overall_status"] == "completed_with_errors"
        || !report["orchestration_error"].is_null()
        || report["cleanup"]["status"] != "passed";
    let Some(results) = report["test_results"].as_array_mut() else {
        return Err("expedient has no test_results array".into());
    };
    for importer in project
        .tests
        .iter()
        .filter(|test| test.test_ref == "import/csv")
    {
        let imported = imported_test_result(project_root, importer, external_id)?;
        let Some(imported) = imported else {
            if let Some(row) = results
                .iter_mut()
                .find(|row| row["test_id"].as_str() == Some(importer.name.as_str()))
            {
                row["status"] = json!("skipped");
                row["exit_code"] = Value::Null;
                row["stdout"] = json!("");
                row["stderr"] = json!("");
                row["diagnostics"] = json!(format!(
                    "CSV file has no row for student external_id {external_id:?}"
                ));
            }
            continue;
        };
        replace_process_result(results, &importer.name, &imported, true, &importer.name)?;
        if let Some(target) = importer
            .parameters
            .into_strings()?
            .get("override_test")
            .cloned()
        {
            replace_process_result(results, &target, &imported, false, &importer.name)?;
        }
    }
    let has_failed_test = results.iter().any(|row| {
        matches!(
            row["status"].as_str(),
            Some("failed" | "error" | "timed_out")
        )
    });
    report["overall_status"] = json!(if has_failed_test || already_has_errors {
        "completed_with_errors"
    } else {
        "completed"
    });
    Ok(())
}

fn replace_process_result(
    results: &mut [Value],
    test_id: &str,
    imported: &ImportedProcessResult,
    imported_test: bool,
    importer_name: &str,
) -> Result<(), String> {
    let row = results
        .iter_mut()
        .find(|row| row["test_id"].as_str() == Some(test_id))
        .ok_or_else(|| format!("CSV test cannot find result row for test {test_id:?}"))?;
    row["test_result_id"] = json!(format!("result-{}", uuid::Uuid::new_v4().simple()));
    if imported_test {
        row["test_reference"] = json!("import/csv");
    }
    row["status"] = json!(if imported.exit_code == 0 {
        "passed"
    } else {
        "failed"
    });
    row["stdout"] = json!(imported.stdout);
    row["stderr"] = json!(imported.stderr);
    row["exit_code"] = json!(imported.exit_code);
    row["diagnostics"] = json!(format!("result imported from CSV test {importer_name:?}"));
    row["imported_from"] = json!(importer_name);
    Ok(())
}

pub(crate) fn check_rows(
    declarations: &[ProjectCheck],
    project_root: &Path,
    report: &Value,
    existing_rows: &[Value],
) -> Result<Vec<Value>, String> {
    let external_id = report["student"]["external_id"]
        .as_str()
        .unwrap_or_default();
    let mut output = existing_rows.to_vec();
    for declaration in declarations
        .iter()
        .filter(|item| item.check_ref == "import/csv")
    {
        let parameters = declaration.parameters.into_strings()?;
        let filepath = required_filepath(&parameters, "CSV check")?;
        let table = read_project_csv(project_root, filepath)?;
        require_external_id(&table, "CSV check")?;
        let Some(row) = table.row_for_external_id(external_id) else {
            continue;
        };
        let descriptions = descriptions(&parameters)?;
        for (index, name) in table.headers.iter().enumerate() {
            if normalize_header(name) == "external_id" {
                continue;
            }
            let cell = row.values[index].trim();
            let (result, err) = match cell {
                "0" => (json!(0), Value::Null),
                "1" => (json!(1), Value::Null),
                _ => (
                    json!(0),
                    json!(format!(
                        "CSV value {cell:?} for check {name:?} must be 0 or 1"
                    )),
                ),
            };
            let description = descriptions.get(name).cloned().or_else(|| {
                declarations
                    .iter()
                    .find(|candidate| {
                        candidate.name == *name && candidate.check_ref != "import/csv"
                    })
                    .and_then(|candidate| candidate.description.clone())
            });
            let imported = json!({
                "name": name,
                "description": description,
                "result": result,
                "err": err,
                "date_time": chrono::Utc::now().to_rfc3339(),
                "duration_ms": 0,
                "source": "import/csv"
            });
            replace_named_row(&mut output, name, imported);
        }
    }
    Ok(output)
}

pub(crate) fn rubric_rows(
    declarations: &[ProjectRubric],
    project_root: &Path,
    report: &Value,
    existing_rows: &[Value],
    multiplier: f64,
) -> Result<Vec<Value>, String> {
    let external_id = report["student"]["external_id"]
        .as_str()
        .unwrap_or_default();
    let mut output = existing_rows.to_vec();
    for declaration in declarations
        .iter()
        .filter(|item| item.calculation == "import/csv")
    {
        let parameters = declaration.parameters.into_strings()?;
        let filepath = required_filepath(&parameters, "CSV rubric")?;
        let table = read_project_csv(project_root, filepath)?;
        require_external_id(&table, "CSV rubric")?;
        let Some(row) = table.row_for_external_id(external_id) else {
            continue;
        };
        let descriptions = descriptions(&parameters)?;
        for (index, name) in table.headers.iter().enumerate() {
            if normalize_header(name) == "external_id" {
                continue;
            }
            let (value, error) = parse_mark(&row.values[index], name, "rubric");
            let imported = json!({
                "name": name,
                "description": descriptions.get(name).cloned().or_else(|| {
                    existing_rows
                        .iter()
                        .find(|candidate| candidate["name"].as_str() == Some(name))
                        .and_then(|candidate| candidate["description"].as_str().map(str::to_owned))
                }),
                "calculation": "import/csv",
                "checks": [],
                "rubrics_multiplier": multiplier,
                "check_results": [],
                "result": value,
                "err": error,
                "date_time": chrono::Utc::now().to_rfc3339(),
                "duration_ms": 0,
                "source": "import/csv"
            });
            replace_named_row(&mut output, name, imported);
        }
    }
    Ok(output)
}

pub(crate) fn evaluation_rows(
    declarations: &[ProjectEvaluation],
    project_root: &Path,
    report: &Value,
    existing_rows: &[Value],
    multiplier: f64,
) -> Result<Vec<Value>, String> {
    let external_id = report["student"]["external_id"]
        .as_str()
        .unwrap_or_default();
    let mut output = existing_rows.to_vec();
    for declaration in declarations
        .iter()
        .filter(|item| item.calculation == "import/csv")
    {
        let parameters = declaration.parameters.into_strings()?;
        let filepath = required_filepath(&parameters, "CSV evaluation")?;
        let table = read_project_csv(project_root, filepath)?;
        require_external_id(&table, "CSV evaluation")?;
        let Some(row) = table.row_for_external_id(external_id) else {
            continue;
        };
        let descriptions = descriptions(&parameters)?;
        for (index, name) in table.headers.iter().enumerate() {
            if normalize_header(name) == "external_id" {
                continue;
            }
            let (value, error) = parse_mark(&row.values[index], name, "evaluation");
            let imported = json!({
                "name": name,
                "description": descriptions.get(name).cloned().or_else(|| {
                    existing_rows
                        .iter()
                        .find(|candidate| candidate["name"].as_str() == Some(name))
                        .and_then(|candidate| candidate["description"].as_str().map(str::to_owned))
                }),
                "calculation": "import/csv",
                "rubrics": [],
                "rubric_results": [],
                "evaluation_multiplier": multiplier,
                "result": value,
                "err": error,
                "date_time": chrono::Utc::now().to_rfc3339(),
                "duration_ms": 0,
                "source": "import/csv"
            });
            replace_named_row(&mut output, name, imported);
        }
    }
    Ok(output)
}

pub(crate) fn not_delivered_evaluation_rows(
    items: &[ProjectEvaluation],
    project_root: &Path,
    mark: Option<f64>,
    multiplier: f64,
) -> Result<Vec<Value>, String> {
    let mut rows = items
        .iter()
        .filter(|item| item.calculation != "import/csv")
        .map(|item| {
            json!({
                "name": item.name,
                "description": item.description,
                "calculation": "not_delivered_mark",
                "rubrics": [],
                "rubric_results": [],
                "evaluation_multiplier": multiplier,
                "result": mark,
                "err": null,
                "date_time": chrono::Utc::now().to_rfc3339(),
                "duration_ms": 0,
                "source": "not_delivered_mark"
            })
        })
        .collect::<Vec<_>>();
    for declaration in items.iter().filter(|item| item.calculation == "import/csv") {
        let parameters = declaration.parameters.into_strings()?;
        let filepath = required_filepath(&parameters, "CSV evaluation")?;
        let table = read_project_csv(project_root, filepath)?;
        require_external_id(&table, "CSV evaluation")?;
        let descriptions = descriptions(&parameters)?;
        for name in table
            .headers
            .iter()
            .filter(|header| normalize_header(header) != "external_id")
        {
            replace_named_row(
                &mut rows,
                name,
                json!({
                    "name": name,
                    "description": descriptions.get(name),
                    "calculation": "not_delivered_mark",
                    "rubrics": [],
                    "rubric_results": [],
                    "evaluation_multiplier": multiplier,
                    "result": mark,
                    "err": null,
                    "date_time": chrono::Utc::now().to_rfc3339(),
                    "duration_ms": 0,
                    "source": "not_delivered_mark"
                }),
            );
        }
    }
    Ok(rows)
}

fn parse_mark(cell: &str, name: &str, kind: &str) -> (Value, Value) {
    match cell.trim().parse::<f64>() {
        Ok(value) if value.is_finite() => (json!(value), Value::Null),
        _ => (
            Value::Null,
            json!(format!(
                "CSV value {cell:?} for {kind} {name:?} must be a finite number"
            )),
        ),
    }
}

fn replace_named_row(rows: &mut Vec<Value>, name: &str, replacement: Value) {
    if let Some(index) = rows
        .iter()
        .position(|row| row["name"].as_str() == Some(name))
    {
        rows[index] = replacement;
    } else {
        rows.push(replacement);
    }
}

fn descriptions(
    parameters: &std::collections::BTreeMap<String, String>,
) -> Result<HashMap<String, String>, String> {
    let Some(encoded) = parameters.get("descriptions") else {
        return Ok(HashMap::new());
    };
    serde_json::from_str(encoded)
        .map_err(|error| format!("descriptions parameter is invalid: {error}"))
}

fn require_columns(table: &CsvTable, columns: &[&str], label: &str) -> Result<(), String> {
    for column in columns {
        if table.index(column).is_none() {
            return Err(format!("{label} CSV must have an {column} column"));
        }
    }
    Ok(())
}

fn require_external_id(table: &CsvTable, label: &str) -> Result<(), String> {
    require_columns(table, &["external_id"], label)
}

fn required_filepath<'a>(
    parameters: &'a std::collections::BTreeMap<String, String>,
    label: &str,
) -> Result<&'a str, String> {
    parameters
        .get("filepath")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{label} requires parameter filepath"))
}

fn validate_relative_filepath(filepath: &str) -> Result<(), String> {
    let path = Path::new(filepath);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("CSV filepath must be project-relative and must not contain '..'".into());
    }
    Ok(())
}

fn parse_table(contents: &str, path: &Path) -> Result<CsvTable, String> {
    let mut records = crate::student_csv::parse_csv(contents);
    if records.is_empty() {
        return Err(format!("CSV file {} is empty", path.display()));
    }
    let headers = records
        .remove(0)
        .map_err(|error| format!("CSV file {} has an invalid header: {error}", path.display()))?
        .into_iter()
        .map(|header| header.trim().to_owned())
        .collect::<Vec<_>>();
    if headers.iter().any(String::is_empty) {
        return Err(format!(
            "CSV file {} has an empty column name",
            path.display()
        ));
    }
    let mut indexes = HashMap::new();
    for (index, header) in headers.iter().enumerate() {
        if indexes.insert(normalize_header(header), index).is_some() {
            return Err(format!(
                "CSV file {} repeats column {header:?}",
                path.display()
            ));
        }
    }
    let external_index = indexes
        .get(&normalize_header("external_id"))
        .copied()
        .ok_or_else(|| {
            format!(
                "CSV file {} must have an external_id column",
                path.display()
            )
        })?;
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (offset, record) in records.into_iter().enumerate() {
        let row_number = offset + 2;
        let values = record.map_err(|error| {
            format!(
                "CSV file {} row {row_number} is invalid: {error}",
                path.display()
            )
        })?;
        if values.len() != headers.len() {
            return Err(format!(
                "CSV file {} row {row_number} has {} fields; expected {}",
                path.display(),
                values.len(),
                headers.len()
            ));
        }
        let external_id = values[external_index].trim().to_owned();
        if external_id.is_empty() {
            return Err(format!(
                "CSV file {} row {row_number} has an empty external_id",
                path.display()
            ));
        }
        if !seen.insert(external_id.clone()) {
            return Err(format!(
                "CSV file {} repeats external_id {external_id:?}",
                path.display()
            ));
        }
        rows.push(CsvRow {
            external_id,
            values,
        });
    }
    Ok(CsvTable {
        headers,
        indexes,
        rows,
    })
}

fn normalize_header(header: &str) -> String {
    header.trim().to_ascii_lowercase()
}
