//! Purpose: CLI entry point for student import/export and delivery-run commands.
mod api_client;
mod assignment_commands;
mod delivery_commands;
mod maintenance_commands;
mod output;

use std::{
    collections::HashMap,
    error::Error,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use thot_core::{GetStudentsResponse, ImportStudentsResponse, Student, StudentInput};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

const DEFAULT_HOST: &str = "localhost";
const DEFAULT_PORT: u16 = 8081;

#[derive(Debug, Default)]
struct CliOptions {
    host: Option<String>,
    port: Option<u16>,
    command: Vec<String>,
    help: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) host: String,
    pub(crate) port: u16,
}

#[derive(Debug, Default)]
struct FileConfig {
    host: Option<String>,
    port: Option<u16>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    run().await
}

async fn run() -> Result<(), Box<dyn Error>> {
    let options = parse_args(std::env::args().skip(1))?;
    if options.help {
        print_usage();
        return Ok(());
    }

    let cwd = std::env::current_dir()?;
    let settings = resolve_settings(&options, &cwd, home_dir().as_deref())?;
    match options.command.as_slice() {
        [command] if matches!(command.as_str(), "clean" | "prune" | "reset") => {
            maintenance_commands::handle(command, &settings).await?;
        }
        [command, ..] if command == "assignments" => {
            assignment_commands::handle(&options.command[1..], &settings).await?;
        }
        [run, ..] if run == "run" => {
            delivery_commands::handle(&options.command[1..], &settings).await?;
        }
        [import, students, path] if import == "import" && students == "students" => {
            let (records, malformed) = read_students_csv(Path::new(path))?;
            let payload = serde_json::to_vec(&serde_json::json!({
                "project_path": current_project_path()?,
                "students": records
            }))?;
            let body = http_request(&settings, "POST", "/import/students", Some(&payload)).await?;
            let response: ImportStudentsResponse = serde_json::from_slice(&body)?;
            output::print_yaml(&serde_json::to_value(response)?)?;
            if !malformed.is_empty() {
                eprintln!(
                    "Se omitieron {} filas CSV malformadas antes de enviar los datos:",
                    malformed.len()
                );
                for (row, reason) in malformed {
                    eprintln!("  fila {row}: {reason}");
                }
            }
        }
        [get, students] if get == "get" && students == "students" => {
            let endpoint = with_project_query("/get/students", current_project_path()?.as_deref());
            let body = http_request(&settings, "GET", &endpoint, None).await?;
            let response: GetStudentsResponse = serde_json::from_slice(&body)?;
            output::print_yaml(&serde_json::to_value(response)?)?;
        }
        [get, students, format] if get == "get" && students == "students" && format == "csv" => {
            let endpoint = with_project_query("/get/students", current_project_path()?.as_deref());
            let body = http_request(&settings, "GET", &endpoint, None).await?;
            let response: GetStudentsResponse = serde_json::from_slice(&body)?;
            eprintln!(
                "Project: {} ({}) [{}]",
                response.project_name, response.project_id, response.project_path
            );
            print!("{}", format_students_csv(&response.students)?);
        }
        _ => {
            print_usage();
            return Err("comando no válido".into());
        }
    }

    Ok(())
}

pub(crate) fn current_project_path() -> Result<Option<String>, Box<dyn Error>> {
    let current = std::env::current_dir()?;
    let configuration = current.join("configuration.yaml");
    match fs::symlink_metadata(&configuration) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("current project configuration.yaml must not be a symlink".into())
        }
        Ok(metadata) if metadata.is_file() => {
            Ok(Some(current.canonicalize()?.to_string_lossy().into_owned()))
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn with_project_query(endpoint: &str, project_path: Option<&str>) -> String {
    let Some(project_path) = project_path else {
        return endpoint.to_owned();
    };
    format!(
        "{endpoint}?project_path={}",
        encode_query_component(project_path)
    )
}

fn encode_query_component(value: &str) -> String {
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

fn parse_args(args: impl Iterator<Item = String>) -> Result<CliOptions, Box<dyn Error>> {
    let mut options = CliOptions::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => options.help = true,
            "-h" | "--host" | "--server" => {
                options.host = Some(args.next().ok_or("falta el servidor después de -h")?);
            }
            "-p" | "--port" => {
                let value = args.next().ok_or("falta el puerto después de -p")?;
                options.port = Some(value.parse()?);
            }
            option if option.starts_with('-') => {
                return Err(format!("opción no reconocida: {option}").into());
            }
            command => {
                options.command.push(command.to_owned());
                options.command.extend(args);
                break;
            }
        }
    }
    Ok(options)
}

fn resolve_settings(
    options: &CliOptions,
    cwd: &Path,
    home: Option<&Path>,
) -> Result<Settings, Box<dyn Error>> {
    let config = config_candidates(cwd, home)
        .into_iter()
        .find(|path| path.is_file())
        .map(read_config)
        .transpose()?
        .unwrap_or_default();
    Ok(Settings {
        host: options
            .host
            .clone()
            .or(config.host)
            .unwrap_or_else(|| DEFAULT_HOST.to_owned()),
        port: options.port.or(config.port).unwrap_or(DEFAULT_PORT),
    })
}

fn config_candidates(cwd: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = vec![cwd.join("thot-config.yaml"), cwd.join(".thot-config.yaml")];
    if let Some(home) = home {
        candidates.push(home.join(".config/thot/thot-config.yaml"));
    }
    candidates
}

fn read_config(path: PathBuf) -> Result<FileConfig, Box<dyn Error>> {
    let contents = fs::read_to_string(&path)?;
    let mut config = FileConfig::default();
    for (line_number, raw_line) in contents.lines().enumerate() {
        let line = strip_yaml_comment(raw_line).trim();
        if line.is_empty() || line == "---" || line == "..." {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or_else(|| {
            format!(
                "configuración no válida en {}:{}",
                path.display(),
                line_number + 1
            )
        })?;
        let key = key.trim();
        let value = unquote_yaml_scalar(value.trim());
        match key {
            "host" | "server" => config.host = Some(value),
            "port" => {
                config.port = Some(value.parse().map_err(|_| {
                    format!("puerto no válido en {}:{}", path.display(), line_number + 1)
                })?)
            }
            _ => {} // Other application settings do not affect this CLI functionality.
        }
    }
    Ok(config)
}

fn strip_yaml_comment(line: &str) -> &str {
    let mut quote = None;
    for (index, character) in line.char_indices() {
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
        } else if character == '#' && quote.is_none() {
            let previous_is_space = index == 0 || line[..index].ends_with(char::is_whitespace);
            if previous_is_space {
                return &line[..index];
            }
        }
    }
    line
}

fn unquote_yaml_scalar(value: &str) -> String {
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = value.as_bytes()[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_owned();
        }
    }
    value.to_owned()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
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
        .filter(|character| character.is_ascii_alphanumeric())
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

fn read_students_csv(
    path: &Path,
) -> Result<(Vec<StudentInput>, Vec<(u64, String)>), Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;
    let mut parsed = parse_csv(&contents);
    if parsed.is_empty() {
        return Err("el fichero CSV está vacío".into());
    }
    let headers = parsed
        .remove(0)
        .map_err(|error| format!("cabecera CSV no válida: {error}"))?;
    let mut indexes = HashMap::new();
    for (index, header) in headers.iter().enumerate() {
        if let Some(kind) = field_kind(header) {
            indexes.entry(kind).or_insert(index);
        }
    }
    let name_index = *indexes
        .get("name")
        .ok_or("el CSV debe incluir una columna de nombre: Nom, Nombre o Name")?;
    let email_index = *indexes
        .get("email")
        .ok_or("el CSV debe incluir una columna de correo: Adreça electrònica, Email o Mail")?;

    let mut students = Vec::new();
    let mut malformed = Vec::new();
    for (record_index, result) in parsed.into_iter().enumerate() {
        let row = record_index as u64 + 2;
        let record = match result {
            Ok(record) => record,
            Err(error) => {
                malformed.push((row, error));
                continue;
            }
        };
        if record.len() != headers.len() {
            malformed.push((
                row,
                format!(
                    "se esperaban {} columnas y se encontraron {}",
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
                .map(str::to_owned)
        };
        students.push(StudentInput {
            source_row: Some(row),
            name: record[name_index].trim().to_owned(),
            email: record[email_index].trim().to_owned(),
            surname: value("surname"),
            external_id: value("external_id"),
            group_name: value("group"),
            center: value("center"),
            birth_date: value("birth_date"),
        });
    }
    Ok((students, malformed))
}

fn parse_csv(contents: &str) -> Vec<Result<Vec<String>, String>> {
    let characters = contents.chars().collect::<Vec<_>>();
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut after_quote = false;
    let mut record_error = None;
    let mut record_has_content = false;
    let mut index = 0;

    while index < characters.len() {
        let character = characters[index];
        if quoted {
            if character == '"' {
                if characters.get(index + 1) == Some(&'"') {
                    field.push('"');
                    index += 1;
                } else {
                    quoted = false;
                    after_quote = true;
                }
            } else {
                field.push(character);
            }
        } else if character == '\n' || character == '\r' {
            if character == '\r' && characters.get(index + 1) == Some(&'\n') {
                index += 1;
            }
            if record_has_content
                || !fields.is_empty()
                || !field.is_empty()
                || record_error.is_some()
            {
                fields.push(std::mem::take(&mut field));
                records.push(match record_error.take() {
                    Some(error) => Err(error),
                    None => Ok(std::mem::take(&mut fields)),
                });
            }
            fields.clear();
            field.clear();
            after_quote = false;
            record_has_content = false;
        } else if character == ',' {
            fields.push(std::mem::take(&mut field));
            after_quote = false;
            record_has_content = true;
        } else if character == '"' {
            if field.is_empty() && !after_quote {
                quoted = true;
                record_has_content = true;
            } else {
                record_error.get_or_insert_with(|| "comilla en un campo sin escapar".to_owned());
                field.push(character);
                after_quote = false;
                record_has_content = true;
            }
        } else if after_quote {
            if !character.is_whitespace() {
                record_error
                    .get_or_insert_with(|| "contenido después del cierre de comillas".to_owned());
            }
        } else {
            field.push(character);
            if !character.is_whitespace() {
                record_has_content = true;
            }
        }
        index += 1;
    }

    if quoted {
        records.push(Err("campo entrecomillado sin cerrar".into()));
    } else if record_has_content
        || !fields.is_empty()
        || !field.is_empty()
        || record_error.is_some()
    {
        fields.push(field);
        records.push(match record_error {
            Some(error) => Err(error),
            None => Ok(fields),
        });
    }
    records
}

async fn http_request(
    settings: &Settings,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let host = settings
        .host
        .strip_prefix("http://")
        .unwrap_or(&settings.host)
        .trim_end_matches('/');
    if host.is_empty() || host.contains('/') || host.contains("https://") {
        return Err("servidor no válido; usa un nombre de host o una dirección HTTP".into());
    }
    let mut stream = TcpStream::connect((host, settings.port)).await?;
    let host_header = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{}", settings.port)
    } else {
        format!("{host}:{}", settings.port)
    };
    let payload = body.unwrap_or_default();
    let headers = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_header}\r\nAccept: application/json\r\nConnection: close\r\nContent-Length: {}\r\n{}\r\n",
        payload.len(),
        if body.is_some() {
            "Content-Type: application/json\r\n"
        } else {
            ""
        }
    );
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(payload).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    parse_http_response(&response)
}

fn parse_http_response(response: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("respuesta HTTP no válida")?;
    let header_text = std::str::from_utf8(&response[..split])?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines.next().ok_or("respuesta HTTP sin estado")?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or("respuesta HTTP sin código de estado")?
        .parse::<u16>()?;
    let chunked = lines.any(|line| {
        line.split_once(':').is_some_and(|(key, value)| {
            key.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
        })
    });
    let raw_body = &response[split + 4..];
    let response_body = if chunked {
        decode_chunked(raw_body)?
    } else {
        raw_body.to_vec()
    };
    if !(200..300).contains(&status) {
        return Err(format!(
            "thotd respondió HTTP {status}: {}",
            String::from_utf8_lossy(&response_body)
        )
        .into());
    }
    Ok(response_body)
}

fn decode_chunked(mut body: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut decoded = Vec::new();
    loop {
        let line_end = body
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or("respuesta chunked no válida")?;
        let length_text = std::str::from_utf8(&body[..line_end])?
            .split(';')
            .next()
            .unwrap_or_default();
        let length = usize::from_str_radix(length_text.trim(), 16)?;
        body = &body[line_end + 2..];
        if length == 0 {
            break;
        }
        if body.len() < length + 2 || &body[length..length + 2] != b"\r\n" {
            return Err("respuesta chunked truncada".into());
        }
        decoded.extend_from_slice(&body[..length]);
        body = &body[length + 2..];
    }
    Ok(decoded)
}

fn format_students_csv(students: &[Student]) -> Result<String, Box<dyn Error>> {
    let mut output = Vec::new();
    write_csv_record(
        &mut output,
        &[
            "id",
            "nom",
            "apellidos",
            "email",
            "id_externo",
            "grupo",
            "centro",
            "fecha nacimiento",
        ],
    )?;
    for student in students {
        write_csv_record(
            &mut output,
            &[
                &student.id.to_string(),
                &student.name,
                student.surname.as_deref().unwrap_or_default(),
                &student.email,
                student.external_id.as_deref().unwrap_or_default(),
                student.group_name.as_deref().unwrap_or_default(),
                student.center.as_deref().unwrap_or_default(),
                student.birth_date.as_deref().unwrap_or_default(),
            ],
        )?;
    }
    Ok(String::from_utf8(output)?)
}

fn write_csv_record(output: &mut impl Write, fields: &[&str]) -> io::Result<()> {
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

fn print_usage() {
    println!(
        "Uso:\n  thot [-h servidor] [-p puerto] import students <fichero.csv>\n  thot [-h servidor] [-p puerto] get students [csv]\n  thot assignments guess [--json]\n  thot assignments get [--json]\n  thot assignments import <fichero.yaml|json> [--json]\n  thot clean\n  thot prune\n  thot reset\n  thot run [list]\n  thot run create <ruta-proyecto> [--deliveries DIR]\n  thot run create --project <ruta-proyecto> [--deliveries DIR]\n  thot run status [RUN_ID]\n  thot run checks [RUN_ID]\n  thot run delete RUN_ID\n  thot run expedient EXPEDIENT_ID\n  thot run cancel RUN_ID\n  thot run cleanup EXPEDIENT_ID\n\nLas asignaciones se muestran como YAML por defecto; use --json para salida JSON.\n\nOpciones:\n  -h, --host <servidor>  Servidor thotd (por defecto: 127.0.0.1)\n  -p, --port <puerto>    Puerto thotd (por defecto: 8081)\n      --help             Muestra esta ayuda\n\nConfiguración YAML: ./thot-config.yaml, ./.thot-config.yaml o ~/.config/thot/thot-config.yaml"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use thot_engine::{SqliteStudentRepository, StudentService};
    use tower::ServiceExt;

    #[test]
    fn recognizes_header_variants_and_aliases() {
        assert_eq!(field_kind("Adreça electrònica"), Some("email"));
        assert_eq!(field_kind("e-mail"), Some("email"));
        assert_eq!(field_kind("Email address"), Some("email"));
        assert_eq!(field_kind("Nombre"), Some("name"));
        assert_eq!(field_kind("Birth date"), Some("birth_date"));
    }

    #[test]
    fn local_config_precedes_home_and_cli_options_override_config() {
        let root = std::env::temp_dir().join(format!(
            "thot-cli-config-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let cwd = root.join("work");
        let home = root.join("home");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(home.join(".config/thot")).unwrap();
        fs::write(
            cwd.join(".thot-config.yaml"),
            "host: local-server\nport: 8000\n",
        )
        .unwrap();
        fs::write(
            home.join(".config/thot/thot-config.yaml"),
            "host: home-server\nport: 9000\n",
        )
        .unwrap();
        fs::write(
            cwd.join("thot-config.yaml"),
            "host: primary-local\nport: 8100\n",
        )
        .unwrap();

        let no_overrides = CliOptions::default();
        assert_eq!(
            resolve_settings(&no_overrides, &cwd, Some(&home)).unwrap(),
            Settings {
                host: "primary-local".into(),
                port: 8100,
            }
        );
        fs::remove_file(cwd.join("thot-config.yaml")).unwrap();
        fs::remove_file(cwd.join(".thot-config.yaml")).unwrap();
        assert_eq!(
            resolve_settings(&no_overrides, &cwd, Some(&home)).unwrap(),
            Settings {
                host: "home-server".into(),
                port: 9000,
            }
        );
        let overrides = CliOptions {
            host: Some("cli-server".into()),
            port: Some(8765),
            ..CliOptions::default()
        };
        assert_eq!(
            resolve_settings(&overrides, &cwd, Some(&home)).unwrap(),
            Settings {
                host: "cli-server".into(),
                port: 8765,
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_quoted_csv_fields_and_skips_malformed_records() {
        let records = parse_csv("Nom,Email\n\"Anna, Maria\",a@example.org\nBad\n");
        assert_eq!(records[1].as_ref().unwrap()[0], "Anna, Maria");
        assert_eq!(records[2].as_ref().unwrap().len(), 1);
    }

    #[test]
    fn parses_plain_and_chunked_http_responses() {
        let plain = parse_http_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        )
        .unwrap();
        assert_eq!(plain, b"{}");

        let chunked = parse_http_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(chunked, b"{}");
    }

    #[tokio::test]
    async fn imports_two_generated_csv_files_and_continues_after_a_malformed_row() {
        let root = std::env::temp_dir().join(format!(
            "thot-cli-csv-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(&root).unwrap();
        let database_url = format!("sqlite:{}", root.join("thot.sqlite3").display());
        let repository = SqliteStudentRepository::connect(&database_url)
            .await
            .unwrap();
        let app = thot_api::router(StudentService::new(repository));

        let valid_path = root.join("students.csv");
        fs::write(
            &valid_path,
            "Nom,Adreça electrònica,Apellidos,id,grupo,centro,fecha nacimiento,otra\n\
             Anna Garcia,anna@example.org,Garcia,A001,ASIX2,Institut,2004-01-02,ignorada\n",
        )
        .unwrap();
        let (valid_students, valid_malformed) = read_students_csv(&valid_path).unwrap();
        assert!(valid_malformed.is_empty());
        assert_eq!(valid_students.len(), 1);
        assert_eq!(valid_students[0].name, "Anna Garcia");
        assert_eq!(valid_students[0].email, "anna@example.org");
        assert_eq!(valid_students[0].group_name.as_deref(), Some("ASIX2"));
        let valid_response = import_through_router(&app, valid_students).await;
        assert_eq!(valid_response["imported"], 1);
        assert_eq!(valid_response["discarded"], 0);

        let malformed_path = root.join("students-malformed.csv");
        fs::write(
            &malformed_path,
            "Nom,Adreça electrònica,Apellidos,id,grupo,centro,fecha nacimiento,otra\n\
             Marc,marc@example.org,Serra,A002,ASIX2,Institut,2004-02-03,ignorada\n\
             Sense correu,,Garcia,A003,ASIX2,Institut,2004-03-04,ignorada\n\
             Fila incompleta,broken@example.org,Garcia,A004\n\
             Nuria,nuria@example.org,Puig,A005,ASIX2,Institut,2004-05-06,ignorada\n",
        )
        .unwrap();
        let (malformed_students, malformed_rows) = read_students_csv(&malformed_path).unwrap();
        assert_eq!(malformed_rows.len(), 1);
        assert_eq!(malformed_rows[0].0, 4);
        assert_eq!(malformed_students.len(), 3);
        let malformed_response = import_through_router(&app, malformed_students).await;
        assert_eq!(malformed_response["imported"], 2);
        assert_eq!(malformed_response["discarded"], 1);
        assert_eq!(malformed_response["rejected"][0]["row"], 3);

        let list_request = Request::builder()
            .method("GET")
            .uri("/get/students")
            .body(Body::empty())
            .unwrap();
        let list_response = app.clone().oneshot(list_request).await.unwrap();
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body = to_bytes(list_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let list_json: serde_json::Value = serde_json::from_slice(&list_body).unwrap();
        assert_eq!(list_json["total"], 3);
        let listed: GetStudentsResponse = serde_json::from_value(list_json).unwrap();
        let yaml = serde_yaml::to_string(&listed).unwrap();
        assert!(yaml.contains("Anna Garcia"));
        assert!(yaml.contains("Nuria"));
        let exported_csv = format_students_csv(&listed.students).unwrap();
        assert!(exported_csv.starts_with("id,nom,apellidos,email"));
        assert!(exported_csv.contains("anna@example.org"));
        assert!(exported_csv.contains("marc@example.org"));
        assert!(exported_csv.contains("nuria@example.org"));

        drop(app);
        fs::remove_dir_all(root).unwrap();
    }

    async fn import_through_router(
        app: &axum::Router,
        students: Vec<StudentInput>,
    ) -> serde_json::Value {
        let request = Request::builder()
            .method("POST")
            .uri("/import/students")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({ "students": students })).unwrap(),
            ))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }
}
