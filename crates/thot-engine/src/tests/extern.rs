//! Purpose: Runs trusted project-supplied test programs with bounded time and captured output.
use std::path::{Path, PathBuf};
use thot_core::test_results::TestStatus;
use thot_runner::external::run_external_process;

use crate::test_execution::{TestInvocation, TestOutcome};

pub async fn run(invocation: &TestInvocation) -> TestOutcome {
    let root = match invocation.project_root.canonicalize() {
        Ok(root) => root,
        Err(error) => return TestOutcome::error(format!("project files are unavailable: {error}")),
    };
    let script = match resolve_external_script_path(&root, &invocation.test_reference) {
        Ok(path) => path,
        Err(error) => return TestOutcome::error(error),
    };
    let metadata = match std::fs::symlink_metadata(&script) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        Ok(_) => return TestOutcome::error("external test must be a regular file"),
        Err(error) => return TestOutcome::error(format!("external test is unavailable: {error}")),
    };
    let _ = metadata;
    let canonical_script = match script.canonicalize() {
        Ok(path) if path.starts_with(&root) => path,
        Ok(_) => return TestOutcome::error("external test path escapes project root"),
        Err(error) => return TestOutcome::error(format!("cannot resolve external test: {error}")),
    };

    let mut args = Vec::new();
    let executable: PathBuf;
    match canonical_script.extension().and_then(|e| e.to_str()) {
        Some("py") => {
            executable = PathBuf::from("python3");
            args.push(canonical_script.to_string_lossy().into_owned());
        }
        Some("sh") => {
            executable = PathBuf::from("sh");
            args.push(canonical_script.to_string_lossy().into_owned());
        }
        _ => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    return TestOutcome::error("external test is not executable");
                }
            }
            executable = canonical_script.clone();
        }
    }
    for (key, value) in &invocation.parameters {
        if key.contains('=') || key.contains('\0') || value.contains('\0') {
            return TestOutcome::error("external test parameter contains an invalid character");
        }
        args.push(format!("{key}={value}"));
    }
    if let Err(error) = tokio::fs::create_dir_all(&invocation.artifacts_dir).await {
        return TestOutcome::error(format!(
            "cannot create external test artifact directory: {error}"
        ));
    }
    let env = vec![
        (
            "THOT_ARTIFACT_DIR".to_owned(),
            invocation.artifacts_dir.to_string_lossy().into_owned(),
        ),
        (
            "VBOX_USER_HOME".to_owned(),
            invocation
                .runtime
                .vbox_user_home
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "VBOX_MANAGE_PATH".to_owned(),
            invocation
                .runtime
                .vbox_manage
                .to_string_lossy()
                .into_owned(),
        ),
    ];
    match run_external_process(
        &executable,
        &args,
        &root,
        &env,
        invocation.timeout,
        invocation.runtime.output_limit,
    )
    .await
    {
        Ok(output) => TestOutcome {
            status: if output.timed_out {
                TestStatus::TimedOut
            } else if output.exit_code == Some(0) {
                TestStatus::Passed
            } else {
                TestStatus::Failed
            },
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.exit_code,
            diagnostics: if output.stdout_truncated || output.stderr_truncated {
                "process output was truncated at the configured limit".into()
            } else {
                String::new()
            },
            observation: serde_json::json!({"duration_ms": output.elapsed.as_millis()}),
            evidence: Vec::new(),
        },
        Err(error) => TestOutcome::error(error),
    }
}

fn resolve_external_script_path(root: &Path, test_reference: &str) -> Result<PathBuf, String> {
    let Some(relative) = test_reference.strip_prefix("extern/") else {
        return Err("external test reference must begin extern/".into());
    };
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err("external test path is unsafe".into());
    }
    Ok(root.join("extern").join(relative_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_external_reference_under_the_extern_directory() {
        let root = std::env::temp_dir().join(format!("thot-extern-{}", uuid::Uuid::new_v4()));
        let script = root.join("extern/tests/vm_start_time.py");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, "print('test')").unwrap();

        assert_eq!(
            resolve_external_script_path(&root, "extern/tests/vm_start_time.py").unwrap(),
            script
        );
        assert!(resolve_external_script_path(&root, "extern/../vm_start_time.py").is_err());

        std::fs::remove_dir_all(root).unwrap();
    }
}
