use std::path::PathBuf;

pub fn handle(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        _ => Err("usage: thot sandbox run [PROJECT_DIR] [--agent PATH]".into()),
    }
}

fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut project = None;
    let mut agent = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--agent" => {
                index += 1;
                agent = Some(PathBuf::from(
                    args.get(index).ok_or("--agent requires PATH")?,
                ));
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown sandbox option: {value}").into());
            }
            value if project.is_none() => project = Some(PathBuf::from(value)),
            _ => return Err("usage: thot sandbox run [PROJECT_DIR] [--agent PATH]".into()),
        }
        index += 1;
    }
    let agent = agent
        .or_else(find_agent)
        .ok_or("thot-alpine-agent not found; install it beside thot or pass --agent PATH")?;
    let project = project.unwrap_or_else(|| PathBuf::from("."));
    thot_kvm_alpine::run_project(&project, &agent).map_err(Into::into)
}

fn find_agent() -> Option<PathBuf> {
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
