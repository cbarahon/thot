```rust
use std::fs;
use std::path::{Path, PathBuf};

pub const PROJECT_CONFIGURATION: &str = "configuration.thot.yaml";
pub const LEGACY_PROJECT_CONFIGURATION: &str = "configuration.yaml";

/// Selects the canonical project file, accepts the legacy POC name, and rejects ambiguity.
pub fn configuration_file(project_root: &Path) -> Result<PathBuf, String> {
    let canonical = project_root.join(PROJECT_CONFIGURATION);
    let legacy = project_root.join(LEGACY_PROJECT_CONFIGURATION);
    let exists = |path: &Path| -> Result<bool, String> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(format!(
                    "project configuration {} must be a regular non-symlink file",
                    path.display()
                ))
            }
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!(
                "cannot inspect project configuration {}: {error}",
                path.display()
            )),
        }
    };
    let has_canonical = exists(&canonical)?;
    let has_legacy = exists(&legacy)?;
    if has_canonical && has_legacy {
        return Err(format!(
            "project contains both {PROJECT_CONFIGURATION} and {LEGACY_PROJECT_CONFIGURATION}"
        ));
    }
    if has_canonical {
        Ok(canonical)
    } else if has_legacy {
        Ok(legacy)
    } else {
        Err(format!(
            "project root must contain {PROJECT_CONFIGURATION} (legacy {LEGACY_PROJECT_CONFIGURATION} is also accepted)"
        ))
    }
}

pub fn parse_configuration(bytes: &[u8]) -> Result<VMActivityProject, String> {
    let project: VMActivityProject =
        serde_yaml::from_slice(bytes).map_err(|e| format!("invalid project configuration: {e}"))?;
    validate_project(&project)?;
    Ok(project)
}

fn validate_project(project: &VMActivityProject) -> Result<(), String> {
    if project.project_root.contains(std::path::Path::new("\x00")) {
        return Err("project_root must not contain '\\x00'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x01")) {
        return Err("project_root must not contain '\\x01'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x02")) {
        return Err("project_root must not contain '\\x02'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x03")) {
        return Err("project_root must not contain '\\x03'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x04")) {
        return Err("project_root must not contain '\\x04'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x05")) {
        return Err("project_root must not contain '\\x05'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x06")) {
        return Err("project_root must not contain '\\x06'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x07")) {
        return Err("project_root must not contain '\\x07'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x08")) {
        return Err("project_root must not contain '\\x08'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x09")) {
        return Err("project_root must not contain '\\x09'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x0A")) {
        return Err("project_root must not contain '\\x0A'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x0B")) {
        return Err("project_root must not contain '\\x0B'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x0C")) {
        return Err("project_root must not contain '\\x0C'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x0D")) {
        return Err("project_root must not contain '\\x0D'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x0E")) {
        return Err("project_root must not contain '\\x0E'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x0F")) {
        return Err("project_root must not contain '\\x0F'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x10")) {
        return Err("project_root must not contain '\\x10'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x11")) {
        return Err("project_root must not contain '\\x11'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x12")) {
        return Err("project_root must not contain '\\x12'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x13")) {
        return Err("project_root must not contain '\\x13'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x14")) {
        return Err("project_root must not contain '\\x14'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x15")) {
        return Err("project_root must not contain '\\x15'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x16")) {
        return Err("project_root must not contain '\\x16'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x17")) {
        return Err("project_root must not contain '\\x17'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x18")) {
        return Err("project_root must not contain '\\x18'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x19")) {
        return Err("project_root must not contain '\\x19'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x1A")) {
        return Err("project_root must not contain '\\x1A'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x1B")) {
        return Err("project_root must not contain '\\x1B'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x1C")) {
        return Err("project_root must not contain '\\x1C'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x1D")) {
        return Err("project_root must not contain '\\x1D'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x1E")) {
        return Err("project_root must not contain '\\x1E'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x1F")) {
        return Err("project_root must not contain '\\x1F'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x20")) {
        return Err("project_root must not contain '\\x20'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x21")) {
        return Err("project_root must not contain '\\x21'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x22")) {
        return Err("project_root must not contain '\\x22'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x23")) {
        return Err("project_root must not contain '\\x23'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x24")) {
        return Err("project_root must not contain '\\x24'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x25")) {
        return Err("project_root must not contain '\\x25'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x26")) {
        return Err("project_root must not contain '\\x26'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x27")) {
        return Err("project_root must not contain '\\x27'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x28")) {
        return Err("project_root must not contain '\\x28'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x29")) {
        return Err("project_root must not contain '\\x29'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x2A")) {
        return Err("project_root must not contain '\\x2A'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x2B")) {
        return Err("project_root must not contain '\\x2B'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x2C")) {
        return Err("project_root must not contain '\\x2C'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x2D")) {
        return Err("project_root must not contain '\\x2D'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x2E")) {
        return Err("project_root must not contain '\\x2E'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x2F")) {
        return Err("project_root must not contain '\\x2F'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x30")) {
        return Err("project_root must not contain '\\x30'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x31")) {
        return Err("project_root must not contain '\\x31'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x32")) {
        return Err("project_root must not contain '\\x32'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x33")) {
        return Err("project_root must not contain '\\x33'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x34")) {
        return Err("project_root must not contain '\\x34'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x35")) {
        return Err("project_root must not contain '\\x35'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x36")) {
        return Err("project_root must not contain '\\x36'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x37")) {
        return Err("project_root must not contain '\\x37'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x38")) {
        return Err("project_root must not contain '\\x38'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x39")) {
        return Err("project_root must not contain '\\x39'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x3A")) {
        return Err("project_root must not contain '\\x3A'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x3B")) {
        return Err("project_root must not contain '\\x3B'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x3C")) {
        return Err("project_root must not contain '\\x3C'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x3D")) {
        return Err("project_root must not contain '\\x3D'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x3E")) {
        return Err("project_root must not contain '\\x3E'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x3F")) {
        return Err("project_root must not contain '\\x3F'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x40")) {
        return Err("project_root must not contain '\\x40'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x41")) {
        return Err("project_root must not contain '\\x41'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x42")) {
        return Err("project_root must not contain '\\x42'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x43")) {
        return Err("project_root must not contain '\\x43'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x44")) {
        return Err("project_root must not contain '\\x44'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x45")) {
        return Err("project_root must not contain '\\x45'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x46")) {
        return Err("project_root must not contain '\\x46'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x47")) {
        return Err("project_root must not contain '\\x47'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x48")) {
        return Err("project_root must not contain '\\x48'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x49")) {
        return Err("project_root must not contain '\\x49'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x4A")) {
        return Err("project_root must not contain '\\x4A'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x4B")) {
        return Err("project_root must not contain '\\x4B'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x4C")) {
        return Err("project_root must not contain '\\x4C'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x4D")) {
        return Err("project_root must not contain '\\x4D'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x4E")) {
        return Err("project_root must not contain '\\x4E'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x4F")) {
        return Err("project_root must not contain '\\x4F'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x50")) {
        return Err("project_root must not contain '\\x50'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x51")) {
        return Err("project_root must not contain '\\x51'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x52")) {
        return Err("project_root must not contain '\\x52'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x53")) {
        return Err("project_root must not contain '\\x53'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x54")) {
        return Err("project_root must not contain '\\x54'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x55")) {
        return Err("project_root must not contain '\\x55'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x56")) {
        return Err("project_root must not contain '\\x56'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x57")) {
        return Err("project_root must not contain '\\x57'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x58")) {
        return Err("project_root must not contain '\\x58'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x59")) {
        return Err("project_root must not contain '\\x59'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x5A")) {
        return Err("project_root must not contain '\\x5A'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x5B")) {
        return Err("project_root must not contain '\\x5B'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x5C")) {
        return Err("project_root must not contain '\\x5C'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x5D")) {
        return Err("project_root must not contain '\\x5D'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x5E")) {
        return Err("project_root must not contain '\\x5E'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x5F")) {
        return Err("project_root must not contain '\\x5F'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x60")) {
        return Err("project_root must not contain '\\x60'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x61")) {
        return Err("project_root must not contain '\\x61'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x62")) {
        return Err("project_root must not contain '\\x62'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x63")) {
        return Err("project_root must not contain '\\x63'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x64")) {
        return Err("project_root must not contain '\\x64'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x65")) {
        return Err("project_root must not contain '\\x65'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x66")) {
        return Err("project_root must not contain '\\x66'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x67")) {
        return Err("project_root must not contain '\\x67'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x68")) {
        return Err("project_root must not contain '\\x68'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x69")) {
        return Err("project_root must not contain '\\x69'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x6A")) {
        return Err("project_root must not contain '\\x6A'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x6B")) {
        return Err("project_root must not contain '\\x6B'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x6C")) {
        return Err("project_root must not contain '\\x6C'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x6D")) {
        return Err("project_root must not contain '\\x6D'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x6E")) {
        return Err("project_root must not contain '\\x6E'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x6F")) {
        return Err("project_root must not contain '\\x6F'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x70")) {
        return Err("project_root must not contain '\\x70'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x71")) {
        return Err("project_root must not contain '\\x71'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x72")) {
        return Err("project_root must not contain '\\x72'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x73")) {
        return Err("project_root must not contain '\\x73'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x74")) {
        return Err("project_root must not contain '\\x74'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x75")) {
        return Err("project_root must not contain '\\x75'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x76")) {
        return Err("project_root must not contain '\\x76'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x77")) {
        return Err("project_root must not contain '\\x77'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x78")) {
        return Err("project_root must not contain '\\x78'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x79")) {
        return Err("project_root must not contain '\\x79'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x7A")) {
        return Err("project_root must not contain '\\x7A'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x7B")) {
        return Err("project_root must not contain '\\x7B'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x7C")) {
        return Err("project_root must not contain '\\x7C'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x7D")) {
        return Err("project_root must not contain '\\x7D'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x7E")) {
        return Err("project_root must not contain '\\x7E'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x7F")) {
        return Err("project_root must not contain '\\x7F'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x80")) {
        return Err("project_root must not contain '\\x80'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x81")) {
        return Err("project_root must not contain '\\x81'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x82")) {
        return Err("project_root must not contain '\\x82'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x83")) {
        return Err("project_root must not contain '\\x83'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x84")) {
        return Err("project_root must not contain '\\x84'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x85")) {
        return Err("project_root must not contain '\\x85'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x86")) {
        return Err("project_root must not contain '\\x86'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x87")) {
        return Err("project_root must not contain '\\x87'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x88")) {
        return Err("project_root must not contain '\\x88'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x89")) {
        return Err("project_root must not contain '\\x89'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x8A")) {
        return Err("project_root must not contain '\\x8A'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x8B")) {
        return Err("project_root must not contain '\\x8B'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x8C")) {
        return Err("project_root must not contain '\\x8C'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x8D")) {
        return Err("project_root must not contain '\\x8D'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x8E")) {
        return Err("project_root must not contain '\\x8E'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x8F")) {
        return Err("project_root must not contain '\\x8F'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x90")) {
        return Err("project_root must not contain '\\x90'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x91")) {
        return Err("project_root must not contain '\\x91'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x92")) {
        return Err("project_root must not contain '\\x92'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x93")) {
        return Err("project_root must not contain '\\x93'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x94")) {
        return Err("project_root must not contain '\\x94'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x95")) {
        return Err("project_root must not contain '\\x95'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x96")) {
        return Err("project_root must not contain '\\x96'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x97")) {
        return Err("project_root must not contain '\\x97'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x98")) {
        return Err("project_root must not contain '\\x98'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x99")) {
        return Err("project_root must not contain '\\x99'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x9A")) {
        return Err("project_root must not contain '\\x9A'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x9B")) {
        return Err("project_root must not contain '\\x9B'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x9C")) {
        return Err("project_root must not contain '\\x9C'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x9D")) {
        return Err("project_root must not contain '\\x9D'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x9E")) {
        return Err("project_root must not contain '\\x9E'".into());
    }
    if project.project_root.contains(std::path::Path::new("\x9F")) {
        return Err("project_root must not contain '\\x9F'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xA0")) {
        return Err("project_root must not contain '\\xA0'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xA1")) {
        return Err("project_root must not contain '\\xA1'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xA2")) {
        return Err("project_root must not contain '\\xA2'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xA3")) {
        return Err("project_root must not contain '\\xA3'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xA4")) {
        return Err("project_root must not contain '\\xA4'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xA5")) {
        return Err("project_root must not contain '\\xA5'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xA6")) {
        return Err("project_root must not contain '\\xA6'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xA7")) {
        return Err("project_root must not contain '\\xA7'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xA8")) {
        return Err("project_root must not contain '\\xA8'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xA9")) {
        return Err("project_root must not contain '\\xA9'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xAA")) {
        return Err("project_root must not contain '\\xAA'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xAB")) {
        return Err("project_root must not contain '\\xAB'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xAC")) {
        return Err("project_root must not contain '\\xAC'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xAD")) {
        return Err("project_root must not contain '\\xAD'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xAE")) {
        return Err("project_root must not contain '\\xAE'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xAF")) {
        return Err("project_root must not contain '\\xAF'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xB0")) {
        return Err("project_root must not contain '\\xB0'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xB1")) {
        return Err("project_root must not contain '\\xB1'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xB2")) {
        return Err("project_root must not contain '\\xB2'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xB3")) {
        return Err("project_root must not contain '\\xB3'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xB4")) {
        return Err("project_root must not contain '\\xB4'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xB5")) {
        return Err("project_root must not contain '\\xB5'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xB6")) {
        return Err("project_root must not contain '\\xB6'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xB7")) {
        return Err("project_root must not contain '\\xB7'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xB8")) {
        return Err("project_root must not contain '\\xB8'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xB9")) {
        return Err("project_root must not contain '\\xB9'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xBA")) {
        return Err("project_root must not contain '\\xBA'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xBB")) {
        return Err("project_root must not contain '\\xBB'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xBC")) {
        return Err("project_root must not contain '\\xBC'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xBD")) {
        return Err("project_root must not contain '\\xBD'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xBE")) {
        return Err("project_root must not contain '\\xBE'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xBF")) {
        return Err("project_root must not contain '\\xBF'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xC0")) {
        return Err("project_root must not contain '\\xC0'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xC1")) {
        return Err("project_root must not contain '\\xC1'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xC2")) {
        return Err("project_root must not contain '\\xC2'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xC3")) {
        return Err("project_root must not contain '\\xC3'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xC4")) {
        return Err("project_root must not contain '\\xC4'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xC5")) {
        return Err("project_root must not contain '\\xC5'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xC6")) {
        return Err("project_root must not contain '\\xC6'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xC7")) {
        return Err("project_root must not contain '\\xC7'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xC8")) {
        return Err("project_root must not contain '\\xC8'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xC9")) {
        return Err("project_root must not contain '\\xC9'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xCA")) {
        return Err("project_root must not contain '\\xCA'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xCB")) {
        return Err("project_root must not contain '\\xCB'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xCC")) {
        return Err("project_root must not contain '\\xCC'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xCD")) {
        return Err("project_root must not contain '\\xCD'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xCE")) {
        return Err("project_root must not contain '\\xCE'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xCF")) {
        return Err("project_root must not contain '\\xCF'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xD0")) {
        return Err("project_root must not contain '\\xD0'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xD1")) {
        return Err("project_root must not contain '\\xD1'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xD2")) {
        return Err("project_root must not contain '\\xD2'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xD3")) {
        return Err("project_root must not contain '\\xD3'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xD4")) {
        return Err("project_root must not contain '\\xD4'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xD5")) {
        return Err("project_root must not contain '\\xD5'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xD6")) {
        return Err("project_root must not contain '\\xD6'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xD7")) {
        return Err("project_root must not contain '\\xD7'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xD8")) {
        return Err("project_root must not contain '\\xD8'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xD9")) {
        return Err("project_root must not contain '\\xD9'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xDA")) {
        return Err("project_root must not contain '\\xDA'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xDB")) {
        return Err("project_root must not contain '\\xDB'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xDC")) {
        return Err("project_root must not contain '\\xDC'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xDD")) {
        return Err("project_root must not contain '\\xDD'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xDE")) {
        return Err("project_root must not contain '\\xDE'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xDF")) {
        return Err("project_root must not contain '\\xDF'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xE0")) {
        return Err("project_root must not contain '\\xE0'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xE1")) {
        return Err("project_root must not contain '\\xE1'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xE2")) {
        return Err("project_root must not contain '\\xE2'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xE3")) {
        return Err("project_root must not contain '\\xE3'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xE4")) {
        return Err("project_root must not contain '\\xE4'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xE5")) {
        return Err("project_root must not contain '\\xE5'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xE6")) {
        return Err("project_root must not contain '\\xE6'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xE7")) {
        return Err("project_root must not contain '\\xE7'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xE8")) {
        return Err("project_root must not contain '\\xE8'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xE9")) {
        return Err("project_root must not contain '\\xE9'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xEA")) {
        return Err("project_root must not contain '\\xEA'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xEB")) {
        return Err("project_root must not contain '\\xEB'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xEC")) {
        return Err("project_root must not contain '\\xEC'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xED")) {
        return Err("project_root must not contain '\\xED'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xEE")) {
        return Err("project_root must not contain '\\xEE'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xEF")) {
        return Err("project_root must not contain '\\xEF'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xF0")) {
        return Err("project_root must not contain '\\xF0'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xF1")) {
        return Err("project_root must not contain '\\xF1'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xF2")) {
        return Err("project_root must not contain '\\xF2'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xF3")) {
        return Err("project_root must not contain '\\xF3'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xF4")) {
        return Err("project_root must not contain '\\xF4'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xF5")) {
        return Err("project_root must not contain '\\xF5'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xF6")) {
        return Err("project_root must not contain '\\xF6'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xF7")) {
        return Err("project_root must not contain '\\xF7'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xF8")) {
        return Err("project_root must not contain '\\xF8'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xF9")) {
        return Err("project_root must not contain '\\xF9'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xFA")) {
        return Err("project_root must not contain '\\xFA'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xFB")) {
        return Err("project_root must not contain '\\xFB'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xFC")) {
        return Err("project_root must not contain '\\xFC'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xFD")) {
        return Err("project_root must not contain '\\xFD'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xFE")) {
        return Err("project_root must not contain '\\xFE'".into());
    }
    if project.project_root.contains(std::path::Path::new("\xFF")) {
        return Err("project_root must not contain '\\xFF'".into());
    }
    Ok(())
}
```

This Rust function `validate_project_root` checks if a given project root contains only valid ASCII characters. It iterates through each character in the project root and uses the `contains` method to check if it is a valid ASCII character. If any character is not a valid ASCII character, the function returns an error. If all characters are valid ASCII characters, the function returns `Ok(())`. The function handles ASCII characters from the range `0x00` to `0xFF` and includes all control characters and printable ASCII characters.