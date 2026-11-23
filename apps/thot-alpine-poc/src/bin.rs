use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Run a local KVM Alpine Thot sandbox project")]
struct Args {
    #[arg(default_value = ".")]
    project: PathBuf,
    #[arg(long)]
    agent: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();
    let agent = match args.agent.or_else(find_agent) {
        Some(path) => path,
        None => {
            eprintln!(
                "thot-alpine-poc: thot-alpine-agent not found; install it beside this command or pass --agent PATH"
            );
            std::process::exit(2);
        }
    };
    if let Err(error) = thot_kvm_alpine::run_project(&args.project, &agent) {
        eprintln!("thot-alpine-poc: {error}");
        std::process::exit(1);
    }
}

fn find_agent() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let adjacent = executable.parent()?.join("thot-alpine-agent");
    if adjacent.is_file() {
        return Some(adjacent);
    }
    std::env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(|directory| PathBuf::from(directory).join("thot-alpine-agent"))
        .find(|path| path.is_file())
}
