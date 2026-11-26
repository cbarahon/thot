//! A local run owns its worker and operational log. Graded evidence stays in results/ID.
use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn root(project: &Path) -> PathBuf {
    project.join("results/.jobs")
}
pub fn validate_project(project: &Path, show_effective: bool) -> Result<()> {
    let project = project.canonicalize()?;
    let (config, runtime) = thot_kvm_alpine::settings::load(&project)?;
    if show_effective {
        println!("project:\n{}", serde_yaml::to_string(&config)?);
        println!("runtime defaults:\n{}", serde_yaml::to_string(&runtime)?);
    } else {
        println!(
            "Project `{}` ({}) is valid: {} sandbox platform(s), {} evaluation test(s).",
            config.name,
            config.project_id,
            config.sandbox_platforms.len(),
            config
                .sandbox_platforms
                .iter()
                .map(|platform| platform.tests.len())
                .sum::<usize>()
        );
    }
    Ok(())
}
pub fn is_sandbox(project: &Path) -> Result<bool> {
    let path = project.join("configuration.thot.yaml");
    if !path.is_file() {
        return Ok(false);
    }
    let raw: serde_yaml::Value = serde_yaml::from_reader(File::open(path)?)?;
    Ok(raw.get("sandbox_platforms").is_some()
        || matches!(
            raw["schema_version"].as_str(),
            Some("thot.dev/v2" | "thot.dev/v3")
        ))
}
pub fn launch(project: &Path, agent: Option<PathBuf>, delivery: Option<&str>) -> Result<()> {
    let project = project.canonicalize()?;
    let (config, _) = thot_kvm_alpine::settings::load(&project)?;
    let agent = agent
        .or_else(crate::sandbox_commands::find_agent)
        .ok_or("install thot-alpine-agent beside thot or specify --agent PATH")?
        .canonicalize()?;
    let id = Uuid::new_v4();
    let jobs = root(&project);
    fs::create_dir_all(&jobs)?;
    let guard = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(jobs.join("launch.lock"))?;
    guard.lock()?;
    if let Ok(previous) = selected(&project, None)
        && running(&jobs.join(previous.to_string()))?
    {
        return Err(format!("evaluation {previous} is already running; use thot status").into());
    }
    let dir = jobs.join(id.to_string());
    fs::create_dir(&dir)?;
    // fd 0 carries the lock across exec. A dead worker releases it even after SIGKILL.
    let lock = File::create_new(dir.join("worker.lock"))?;
    lock.lock()?;
    let log = File::create_new(dir.join("worker.log"))?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("__sandbox-worker")
        .arg(&project)
        .arg(id.to_string())
        .arg(agent)
        .stdin(lock)
        .stdout(log.try_clone()?)
        .stderr(log);
    if let Some(path) = delivery {
        command.arg(std::path::absolute(path)?);
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    command.spawn()?;
    let tmp = jobs.join(id.to_string() + ".latest");
    fs::write(&tmp, id.to_string())?;
    fs::rename(tmp, jobs.join("latest"))?;
    println!(
        "Started {} in the background\nRun {id}\nProject {}\nFrom the project folder: thot status   |   thot log   |   thot report",
        config.name,
        project.display()
    );
    Ok(())
}
pub fn worker(project: &Path, id: Uuid, agent: &Path, delivery: Option<&str>) -> Result<()> {
    let dir = root(project).join(id.to_string());
    let heartbeat = File::create_new(dir.join("worker.heartbeat"))?;
    let (done, stop) = std::sync::mpsc::channel::<()>();
    let result = std::thread::scope(move |scope| {
        scope.spawn(move || {
            while matches!(
                stop.recv_timeout(std::time::Duration::from_secs(10)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                if heartbeat
                    .set_modified(std::time::SystemTime::now())
                    .is_err()
                {
                    break;
                }
            }
        });
        let result = thot_kvm_alpine::run_with_id(project, agent, id, delivery);
        drop(done);
        result
    });
    let state = match &result {
        Ok(()) => "finished".to_owned(),
        Err(error) => format!("failed: {error}"),
    };
    fs::write(dir.join("outcome.tmp"), &state)?;
    fs::rename(dir.join("outcome.tmp"), dir.join("outcome"))?;
    result.map_err(Into::into)
}
pub fn selected(project: &Path, id: Option<&str>) -> Result<Uuid> {
    let id = match id {
        Some(id) => id.to_owned(),
        None => fs::read_to_string(root(project).join("latest")).map_err(
            |_| "no local evaluation selected; start one with thot run or specify a run ID",
        )?,
    };
    Ok(id.trim().parse()?)
}
fn running(dir: &Path) -> Result<bool> {
    let lock = File::open(dir.join("worker.lock"))?;
    match lock.try_lock_shared() {
        Ok(()) => Ok(false),
        Err(fs::TryLockError::WouldBlock) => Ok(true),
        Err(fs::TryLockError::Error(e)) => Err(e.into()),
    }
}
pub fn inspect(project: &Path, id: Option<&str>, log: bool, follow: bool) -> Result<()> {
    let id = selected(project, id)?;
    let dir = root(project).join(id.to_string());
    let mut file =
        File::open(dir.join("worker.log")).map_err(|e| format!("cannot read run {id}: {e}"))?;
    if log {
        let mut output = io::stdout().lock();
        let mut offset = 0u64;
        loop {
            file.seek(SeekFrom::Start(offset))?;
            let copied = io::copy(&mut file, &mut output)?;
            offset = offset.saturating_add(copied);
            output.flush()?;
            if !follow || !running(&dir)? {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
            file = File::open(dir.join("worker.log"))?;
        }
        return Ok(());
    }
    let active = running(&dir)?;
    let outcome = match fs::read_to_string(dir.join("outcome")) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if active { "running" } else { "interrupted" }.into()
        }
        Err(e) => return Err(e.into()),
    };
    let size = file.metadata()?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(8192)))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    println!(
        "Run {id}   {outcome}\nLast update: {}",
        String::from_utf8_lossy(&tail)
            .lines()
            .rev()
            .find(|line| !line.starts_with("Error:") && !line.starts_with("  diagnostic:"))
            .unwrap_or("Starting worker")
    );
    if active
        && let Ok(age) = fs::metadata(dir.join("worker.log"))
            .and_then(|m| m.modified())
            .and_then(|t| t.elapsed().map_err(io::Error::other))
    {
        println!("Operation log updated {}s ago", age.as_secs());
        if age >= std::time::Duration::from_secs(30) {
            println!(
                "The worker is alive but the current operation has not reported progress recently."
            );
        }
    }
    if active
        && let Ok(age) = fs::metadata(dir.join("worker.heartbeat"))
            .and_then(|m| m.modified())
            .and_then(|t| t.elapsed().map_err(io::Error::other))
    {
        println!("Worker heartbeat {}s ago", age.as_secs());
    }
    Ok(())
}

pub fn watch(project: &Path, id: Option<&str>) -> Result<()> {
    let selected_id = selected(project, id)?;
    loop {
        inspect(project, Some(&selected_id.to_string()), false, false)?;
        if !running(&root(project).join(selected_id.to_string()))? {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        println!();
    }
    Ok(())
}

pub async fn run(args: &[String], settings: &crate::Settings) -> Result<()> {
    if matches!(
        args.first().map(String::as_str),
        Some("list" | "status" | "checks" | "delete" | "expedient" | "cancel" | "cleanup")
    ) {
        if args == ["list"] && is_sandbox(Path::new("."))? {
            let jobs = root(Path::new("."));
            if jobs.is_dir() {
                let mut entries =
                    fs::read_dir(jobs)?.collect::<std::result::Result<Vec<_>, _>>()?;
                entries.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
                for entry in entries {
                    let name = entry.file_name();
                    if name
                        .to_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .is_some()
                    {
                        inspect(Path::new("."), name.to_str(), false, false)?;
                    }
                }
            }
            return Ok(());
        }
        if args.first().is_some_and(|s| s == "status")
            && is_sandbox(Path::new("."))?
            && args.len() <= 2
        {
            return inspect(
                Path::new("."),
                args.get(1).map(String::as_str),
                false,
                false,
            );
        }
        return crate::delivery_commands::handle(args, settings).await;
    }
    let args = args.strip_prefix(&["create".into()]).unwrap_or(args);
    let mut args = args.iter();
    let (mut project, mut agent, mut delivery) = (None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--agent" => agent = Some(PathBuf::from(args.next().ok_or("--agent needs a path")?)),
            "--deliveries" => {
                delivery = Some(
                    args.next()
                        .ok_or("--deliveries needs a directory")?
                        .as_str(),
                )
            }
            "--project" if project.is_none() => {
                project = Some(args.next().ok_or("--project needs a path")?.clone())
            }
            path if !path.starts_with('-') && project.is_none() => project = Some(path.into()),
            _ => return Err("usage: thot run [PROJECT] [--deliveries DIR] [--agent PATH]".into()),
        }
    }
    let project = match project {
        Some(p) => p,
        None => crate::delivery_commands::discover_project()?,
    };
    let path = Path::new(&project);
    let path = if path
        .file_name()
        .is_some_and(|s| s == "configuration.thot.yaml" || s == "configuration.yaml")
    {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    } else {
        path
    };
    if is_sandbox(path)? {
        return launch(path, agent, delivery);
    }
    if agent.is_some() {
        return Err("--agent selects a KVM guest agent and requires a sandbox project".into());
    }
    let mut submit = vec!["create".into(), path.to_string_lossy().into_owned()];
    if let Some(dir) = delivery {
        submit.extend(["--deliveries".into(), dir.into()]);
    }
    crate::delivery_commands::handle(&submit, settings).await?;
    println!("Follow this evaluation with thot status or thot log.");
    Ok(())
}
