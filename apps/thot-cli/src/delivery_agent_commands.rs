use std::{error::Error, fs, path::PathBuf};
use thot_core::sandbox_project::SandboxProject;
use thot_delivery::{build_agent, default_student_input_root};

pub fn handle(args: &[String]) -> Result<(), Box<dyn Error>> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err(
            "usage: thot deliveries list [PROJECT_DIR] | get <NAME> [PROJECT_DIR] [--output PATH]"
                .into(),
        );
    };
    let (project_dir, name, output) = match action {
        "list" => {
            if args.len() > 2 {
                return Err("usage: thot deliveries list [PROJECT_DIR]".into());
            }
            (
                args.get(1)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(".")),
                None,
                None,
            )
        }
        "get" => {
            let name = args
                .get(1)
                .ok_or("deliveries get requires a deliverable name")?
                .clone();
            let mut project = None;
            let mut output = None;
            let mut index = 2;
            while index < args.len() {
                if args[index] == "--output" {
                    index += 1;
                    output = Some(PathBuf::from(
                        args.get(index).ok_or("--output requires a path")?,
                    ));
                } else if project.is_none() {
                    project = Some(PathBuf::from(&args[index]));
                } else {
                    return Err(
                        "usage: thot deliveries get <NAME> [PROJECT_DIR] [--output PATH]".into(),
                    );
                }
                index += 1;
            }
            (
                project.unwrap_or_else(|| PathBuf::from(".")),
                Some(name),
                output,
            )
        }
        _ => return Err(
            "usage: thot deliveries list [PROJECT_DIR] | get <NAME> [PROJECT_DIR] [--output PATH]"
                .into(),
        ),
    };
    let project_dir = project_dir.canonicalize()?;
    let bytes = fs::read(project_dir.join("configuration.thot.yaml"))?;
    if bytes.len() > 1024 * 1024 {
        return Err("configuration.thot.yaml exceeds the 1 MiB limit".into());
    }
    let config: SandboxProject = serde_yaml::from_slice(&bytes)?;
    config.validate()?;
    let input_root = default_student_input_root(&config);
    let mut agent = build_agent(
        config.delivery_agent.as_ref(),
        &project_dir,
        config.delivery_folder.as_deref(),
        input_root.as_deref(),
        &config.delivery_limits,
    )?;
    match name {
        None => {
            println!("NAME\tID\tREVISION\tSIZE\tCREATED\tMODIFIED");
            for delivery in agent.list()? {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    delivery
                        .name
                        .replace('\t', "\\t")
                        .replace('\r', "\\r")
                        .replace('\n', "\\n"),
                    delivery.provider_id.as_deref().unwrap_or("—"),
                    delivery.revision.as_deref().unwrap_or("—"),
                    delivery.size_bytes,
                    delivery.created_at.as_deref().unwrap_or("—"),
                    delivery.modified_at.as_deref().unwrap_or("—")
                );
            }
        }
        Some(name) => {
            let destination = match output {
                Some(path) => path,
                None if !name.is_empty()
                    && name != "."
                    && name != ".."
                    && !name.contains('/')
                    && !name.contains('\\') =>
                {
                    PathBuf::from(&name)
                }
                None => {
                    return Err("deliverable names containing path separators require an explicit --output path".into());
                }
            };
            if destination.exists() {
                return Err(
                    format!("output path already exists: {}", destination.display()).into(),
                );
            }
            let item = agent.get(&name, &destination)?;
            println!(
                "Retrieved {} ({} bytes, sha256 {}) to {}",
                item.info.name,
                item.info.size_bytes,
                item.sha256,
                item.path.display()
            );
        }
    }
    Ok(())
}
