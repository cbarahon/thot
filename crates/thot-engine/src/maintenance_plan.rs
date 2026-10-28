//! Purpose: Finds filesystem entries that are no longer owned by saved Thot runs.
use serde_json::Value;
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct CleanupManifest {
    pub manifest_path: PathBuf,
    pub report_dir: PathBuf,
    pub run_id: String,
    pub delivery_id: String,
    pub vm_uuid: Option<String>,
    pub storage_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct PlannedPath {
    pub path: PathBuf,
    pub manifest_path: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct CleanPlan {
    pub saved_run_ids: HashSet<String>,
    pub saved_vm_uuids: HashSet<String>,
    pub orphan_manifests: Vec<CleanupManifest>,
    pub orphan_paths: Vec<PlannedPath>,
}

pub async fn build_clean_plan(
    pool: &SqlitePool,
    artifact_root: &Path,
    storage_root: &Path,
) -> Result<CleanPlan, String> {
    let runs =
        sqlx::query_as::<_, (String, String)>("SELECT run_id, project_id FROM vm_activity_runs")
            .fetch_all(pool)
            .await
            .map_err(|e| format!("cannot read saved runs: {e}"))?;
    let expedients = sqlx::query_as::<_, (String, String)>(
        "SELECT run_id, delivery_id FROM vm_activity_expedients",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("cannot read saved expedients: {e}"))?;
    let deliveries =
        sqlx::query_as::<_, (String,)>("SELECT delivery_id FROM vm_activity_deliveries")
            .fetch_all(pool)
            .await
            .map_err(|e| format!("cannot read saved deliveries: {e}"))?;

    let run_projects = runs.into_iter().collect::<HashMap<_, _>>();
    let saved_run_ids = run_projects.keys().cloned().collect::<HashSet<_>>();
    let saved_pairs = expedients.into_iter().collect::<HashSet<_>>();
    let saved_delivery_ids = deliveries
        .into_iter()
        .map(|row| row.0)
        .collect::<HashSet<_>>();
    let artifact_root = canonical_existing_root(artifact_root)?;
    let storage_root = canonical_existing_root(storage_root)?;
    let manifests = scan_manifests(&artifact_root)?;
    let valid_manifest_paths = manifests
        .iter()
        .map(|manifest| manifest.manifest_path.clone())
        .collect::<HashSet<_>>();
    let mut plan = CleanPlan {
        saved_run_ids: saved_run_ids.clone(),
        ..CleanPlan::default()
    };
    let mut manifest_by_owned_path = HashMap::new();
    for manifest in manifests {
        let owned_by_saved_rows = saved_run_ids.contains(&manifest.run_id)
            && saved_pairs.contains(&(manifest.run_id.clone(), manifest.delivery_id.clone()))
            && saved_delivery_ids.contains(&manifest.delivery_id);
        if owned_by_saved_rows {
            if let Some(uuid) = manifest.vm_uuid.as_ref() {
                plan.saved_vm_uuids.insert(uuid.clone());
            }
        } else {
            manifest_by_owned_path.insert(
                manifest.storage_path.clone(),
                manifest.manifest_path.clone(),
            );
            manifest_by_owned_path
                .insert(manifest.report_dir.clone(), manifest.manifest_path.clone());
            plan.orphan_manifests.push(manifest);
        }
    }

    let mut candidates = HashSet::new();
    add_orphan_snapshots(
        &artifact_root.join("project-snapshots"),
        &saved_run_ids,
        &mut candidates,
    )?;
    add_orphan_report_trees(
        &artifact_root,
        &run_projects,
        &saved_pairs,
        &saved_delivery_ids,
        &valid_manifest_paths,
        &mut candidates,
    )?;
    add_orphan_sources(
        &artifact_root.join("sources"),
        &saved_delivery_ids,
        &mut candidates,
    )?;
    add_orphan_default_storage(
        &storage_root,
        &saved_run_ids,
        &saved_pairs,
        &saved_delivery_ids,
        &mut candidates,
    )?;

    plan.orphan_paths = candidates
        .into_iter()
        .map(|path| PlannedPath {
            manifest_path: manifest_by_owned_path
                .iter()
                .find_map(|(owned, manifest)| path.starts_with(owned).then(|| manifest.clone())),
            path,
        })
        .collect();
    plan.orphan_paths.sort_by(|a, b| a.path.cmp(&b.path));
    plan.orphan_manifests
        .sort_by(|a, b| a.manifest_path.cmp(&b.manifest_path));
    Ok(plan)
}

fn canonical_existing_root(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("cannot inspect root {}: {e}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "managed root is not a real directory: {}",
            path.display()
        ));
    }
    path.canonicalize()
        .map_err(|e| format!("cannot resolve root {}: {e}", path.display()))
}

fn scan_manifests(root: &Path) -> Result<Vec<CleanupManifest>, String> {
    fn walk(root: &Path, current: &Path, found: &mut Vec<CleanupManifest>) -> Result<(), String> {
        let Ok(entries) = fs::read_dir(current) else {
            return Ok(());
        };
        for entry in entries {
            let entry = entry.map_err(|e| format!("cannot scan artifact tree: {e}"))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|e| format!("cannot inspect artifact entry {}: {e}", path.display()))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                walk(root, &path, found)?;
            } else if metadata.is_file() && entry.file_name() == "cleanup-manifest.json" {
                let Some(path) = managed_candidate(root, &path)? else {
                    continue;
                };
                let bytes = fs::read(&path)
                    .map_err(|e| format!("cannot read cleanup manifest {}: {e}", path.display()))?;
                let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                let (Some(run_id), Some(delivery_id), Some(storage)) = (
                    value.get("run_id").and_then(Value::as_str),
                    value.get("delivery_id").and_then(Value::as_str),
                    value.get("storage_path").and_then(Value::as_str),
                ) else {
                    continue;
                };
                let report_dir = path.parent().unwrap_or(root).to_path_buf();
                found.push(CleanupManifest {
                    manifest_path: path,
                    report_dir,
                    run_id: run_id.to_owned(),
                    delivery_id: delivery_id.to_owned(),
                    vm_uuid: value
                        .get("vm_uuid")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    storage_path: PathBuf::from(storage),
                });
            }
        }
        Ok(())
    }

    let mut found = Vec::new();
    if root.exists() {
        walk(root, root, &mut found)?;
    }
    Ok(found)
}

fn managed_candidate(root: &Path, candidate: &Path) -> Result<Option<PathBuf>, String> {
    let metadata = match fs::symlink_metadata(candidate) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot inspect candidate {}: {error}",
                candidate.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() {
        return Ok(None);
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("cannot resolve candidate {}: {e}", candidate.display()))?;
    if canonical.starts_with(root) {
        Ok(Some(canonical))
    } else {
        Ok(None)
    }
}

fn safe_children(path: &Path) -> Result<Vec<PathBuf>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "cannot inspect managed directory {}: {error}",
                path.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(Vec::new());
    }
    fs::read_dir(path)
        .map_err(|e| format!("cannot read managed directory {}: {e}", path.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|e| format!("cannot read managed entry: {e}"))
        })
        .collect()
}

fn add_orphan_snapshots(
    root: &Path,
    runs: &HashSet<String>,
    out: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    for path in safe_children(root)? {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !runs.contains(name) && managed_candidate(root, &path)?.is_some() {
            out.insert(path);
        }
    }
    Ok(())
}

fn add_orphan_report_trees(
    root: &Path,
    run_projects: &HashMap<String, String>,
    pairs: &HashSet<(String, String)>,
    deliveries: &HashSet<String>,
    valid_manifests: &HashSet<PathBuf>,
    out: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    for project in safe_children(root)? {
        let name = project
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if matches!(name, "sources" | "project-snapshots") {
            continue;
        }
        for run in safe_children(&project)? {
            let run_id = run
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            if let Some(project_id) = run_projects.get(&run_id) {
                if project_id != name {
                    if managed_candidate(root, &run)?.is_some() {
                        out.insert(run);
                    }
                    continue;
                }
            } else {
                if managed_candidate(root, &run)?.is_some() {
                    out.insert(run);
                }
                continue;
            }
            for delivery in safe_children(&run)? {
                let delivery_id = delivery
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned();
                if !pairs.contains(&(run_id.clone(), delivery_id.clone()))
                    || !deliveries.contains(&delivery_id)
                {
                    let manifest_path = delivery.join("cleanup-manifest.json");
                    if manifest_path.exists()
                        && !valid_manifests.contains(
                            &manifest_path
                                .canonicalize()
                                .unwrap_or(manifest_path.clone()),
                        )
                    {
                        // An unreadable manifest may describe external VM storage. Preserve the
                        // report until an operator can inspect it; do not erase ownership evidence.
                        continue;
                    }
                    if managed_candidate(root, &delivery)?.is_some() {
                        out.insert(delivery);
                    }
                }
            }
        }
    }
    Ok(())
}

fn add_orphan_sources(
    root: &Path,
    deliveries: &HashSet<String>,
    out: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    for path in safe_children(root)? {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(id) = name.strip_suffix(".ova") else {
            continue;
        };
        if !deliveries.contains(id) && managed_candidate(root, &path)?.is_some() {
            out.insert(path);
        }
    }
    Ok(())
}

fn add_orphan_default_storage(
    root: &Path,
    runs: &HashSet<String>,
    pairs: &HashSet<(String, String)>,
    deliveries: &HashSet<String>,
    out: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    for run in safe_children(root)? {
        let run_id = run
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        if !runs.contains(&run_id) {
            if managed_candidate(root, &run)?.is_some() {
                out.insert(run);
            }
            continue;
        }
        for delivery in safe_children(&run)? {
            let delivery_id = delivery
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            if !pairs.contains(&(run_id.clone(), delivery_id.clone()))
                || !deliveries.contains(&delivery_id)
            {
                if managed_candidate(root, &delivery)?.is_some() {
                    out.insert(delivery);
                }
            }
        }
    }
    Ok(())
}
