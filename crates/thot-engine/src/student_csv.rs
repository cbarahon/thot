//! Purpose: Parses project students.csv files and maps localized headers to student fields.
use std::collections::HashMap;
use thot_core::StudentInput;

pub fn parse_students_csv(
    contents: &str,
) -> Result<(Vec<StudentInput>, Vec<(u64, String)>), String> {
    let mut records = parse_csv(contents);
    if records.is_empty() {
        return Err("students.csv is empty".into());
    }
    let headers = records
        .remove(0)
        .map_err(|error| format!("students.csv header is invalid: {error}"))?;
    let mut indexes = HashMap::new();
    for (index, header) in headers.iter().enumerate() {
        if let Some(field) = field_kind(header) {
            indexes.entry(field).or_insert(index);
        }
    }
    let name_index = *indexes
        .get("name")
        .ok_or("students.csv must have a name/Nom/Nombre column")?;
    let email_index = *indexes
        .get("email")
        .ok_or("students.csv must have an email/Email/Mail column")?;
    let mut students = Vec::new();
    let mut malformed = Vec::new();
    for (index, record) in records.into_iter().enumerate() {
        let row_number = index as u64 + 2;
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                malformed.push((row_number, error));
                continue;
            }
        };
        if record.len() != headers.len() {
            malformed.push((
                row_number,
                format!(
                    "expected {} columns but found {}",
                    headers.len(),
                    record.len()
                ),
            ));
            continue;
        }
        let value = |kind: &str| {
            indexes
                .get(kind)
                .and_then(|index| record.get(*index))
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        };
        students.push(StudentInput {
            source_row: Some(row_number),
            name: record[name_index].trim().to_owned(),
            surname: value("surname"),
            email: record[email_index].trim().to_owned(),
            external_id: value("external_id"),
            group_name: value("group"),
            center: value("center"),
            birth_date: value("birth_date"),
        });
    }
    Ok((students, malformed))
}

fn normalize_header(header: &str) -> String {
    header
        .trim()
        .to_lowercase()
        .chars()
        .map(|character| match character {
            'à' | 'á' | 'â' | 'ä' => 'a',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            other => other,
        })
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

fn field_kind(header: &str) -> Option<&'static str> {
    match normalize_header(header).as_str() {
        "nom" | "nombre" | "name" | "studentname" | "first" | "firstname" | "givenname"
        | "nombrealumno" | "nombreestudiante" => Some("name"),
        "adrecaelectronica"
        | "direcciondecorreoelectronico"
        | "correoelectronico"
        | "correo"
        | "emailaddress"
        | "email"
        | "mail" => Some("email"),
        "cognom" | "cognoms" | "apellido" | "apellidos" | "surname" | "lastname" | "familyname" => {
            Some("surname")
        }
        "id" | "identificador" | "studentid" | "idalumno" | "idestudiante" | "username"
        | "userid" | "externalid" | "externalstudentid" | "studentnumber" => Some("external_id"),
        "grup" | "grupo" | "group" | "class" => Some("group"),
        "centre" | "centro" | "center" | "school" | "institute" => Some("center"),
        "datanaixement" | "fechanacimiento" | "birthdate" | "dateofbirth" | "dob" => {
            Some("birth_date")
        }
        _ => None,
    }
}

fn parse_csv(contents: &str) -> Vec<Result<Vec<String>, String>> {
    let characters = contents.chars().collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut after_quote = false;
    let mut error: Option<String> = None;
    let mut has_content = false;
    let mut index = 0;
    while index < characters.len() {
        let ch = characters[index];
        if quoted {
            if ch == '"' {
                if characters.get(index + 1) == Some(&'"') {
                    field.push('"');
                    index += 1;
                } else {
                    quoted = false;
                    after_quote = true;
                }
            } else {
                field.push(ch);
            }
        } else if after_quote {
            match ch {
                ',' => {
                    row.push(std::mem::take(&mut field));
                    after_quote = false;
                    has_content = true;
                }
                '\n' | '\r' => {
                    row.push(std::mem::take(&mut field));
                    push_row(&mut rows, &mut row, &mut error, has_content);
                    after_quote = false;
                    has_content = false;
                    if ch == '\r' && characters.get(index + 1) == Some(&'\n') {
                        index += 1;
                    }
                }
                ch if ch.is_whitespace() => {}
                _ => {
                    error.get_or_insert_with(|| "unexpected character after closing quote".into());
                }
            };
        } else {
            match ch {
                '"' if field.is_empty() => {
                    quoted = true;
                    has_content = true;
                }
                '"' => {
                    error.get_or_insert_with(|| "quote inside an unquoted field".into());
                }
                ',' => {
                    row.push(std::mem::take(&mut field));
                    has_content = true;
                }
                '\n' | '\r' => {
                    row.push(std::mem::take(&mut field));
                    push_row(&mut rows, &mut row, &mut error, has_content);
                    has_content = false;
                    if ch == '\r' && characters.get(index + 1) == Some(&'\n') {
                        index += 1;
                    }
                }
                _ => {
                    field.push(ch);
                    has_content = true;
                }
            }
        }
        index += 1;
    }
    if quoted {
        error.get_or_insert_with(|| "unterminated quoted field".into());
    }
    if !row.is_empty() || !field.is_empty() || has_content || after_quote || error.is_some() {
        row.push(field);
        push_row(&mut rows, &mut row, &mut error, true);
    }
    rows
}

fn push_row(
    rows: &mut Vec<Result<Vec<String>, String>>,
    row: &mut Vec<String>,
    error: &mut Option<String>,
    has_content: bool,
) {
    if has_content || row.iter().any(|field| !field.is_empty()) || error.is_some() {
        if let Some(error) = error.take() {
            rows.push(Err(error));
        } else {
            rows.push(Ok(std::mem::take(row)));
        }
    } else {
        row.clear();
        error.take();
    }
}
