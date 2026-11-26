use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
#[test]
fn sandbox_launch_returns_an_id_and_retains_preflight_failure_after_worker_exit() {
    let project = std::env::temp_dir().join(format!("thot-cli-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&project).unwrap();
    fs::write(
        project.join("configuration.thot.yaml"),
        r#"schema_version: thot.dev/v2
project_id: cli-test
name: CLI test
delivery_folder: missing-submissions
sandbox_platforms:
  - tests:
      - id: check
        argv: [sh, -c, "exit 0"]
    evaluation:
      sample: {tests: [check]}
      rubric: {calculation: weighted_sum, scale: 1, criteria: [{check: sample, points: 1}]}
"#,
    )
    .unwrap();
    let invoke = |cwd: &Path, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_thot"))
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap()
    };
    let launch = invoke(&project, &["run", "--agent", "/bin/true"]);
    assert!(launch.status.success(), "{:?}", launch.stderr);
    let stdout = String::from_utf8(launch.stdout).unwrap();
    let id = stdout
        .lines()
        .find_map(|line| line.strip_prefix("Run "))
        .unwrap();
    uuid::Uuid::parse_str(id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = invoke(&project, &["status"]);
        let msg = String::from_utf8(status.stdout).unwrap();
        if msg.contains("failed:") {
            assert!(msg.contains(id));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "worker did not report failure: {msg}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    let log = invoke(&project, &["log", id]);
    assert!(log.status.success());
    assert!(String::from_utf8(log.stdout).unwrap().contains("inventory"));
    fs::remove_dir_all(project).unwrap();
}
