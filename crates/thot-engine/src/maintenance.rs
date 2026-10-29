//! Purpose: Cleans orphaned data and safely prunes or resets saved Thot runtime state.
use crate::{
    maintenance_plan::{self, CleanPlan, PlannedPath},
    maintenance_vms,
    vm_activity::VmActivityRuntime,
};
use serde::Serialize;
use sqlx::SqlitePool;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, Serialize)]
pub struct MaintenanceReport {
    pub operation: String,
    pub removed_paths: Vec<String>,
    pub unregistered_vms: Vec<String>,
    pub retained_run_ids: Vec<String>,
    pub deleted_run_ids: Vec<String>,
    pub preserved_paths: Vec<String>,
    pub failures: Vec<String>,
}

pub async fn clean_orphans(
    pool: &SqlitePool,
    artifact_root: &Path,
    storage_root: &Path,
    runtime: &VmActivityRuntime,
) -> Result<MaintenanceReport, String> {
    let plan = maintenance_plan::build_clean_plan(pool, artifact_root, storage_root).await?;
    clean_with_plan(&plan, artifact_root, storage_root, runtime).await
}

async fn clean_with_plan(
    plan: &CleanPlan,
    artifact_root: &Path,
    storage_root: &Path,
    runtime: &VmActivityRuntime,
) -> Result<MaintenanceReport, String> {
    let vm_report = maintenance_vms::clean_unassigned_vms(plan, runtime).await;
    let mut report = MaintenanceReport {
        operation: "clean".into(),
        unregistered_vms: vm_report.unregistered_uuids,
        preserved_paths: vm_report.preserved_paths,
        failures: vm_report.failures,
        ..MaintenanceReport::default()
    };
    let failed_dirs = vm_report
        .failed_manifest_dirs
        .into_iter()
        .collect::<HashSet<_>>();
    let failed_manifests = plan
        .orphan_manifests
        .iter()
        .filter(|manifest| failed_dirs.contains(&manifest.report_dir.display().to_string()))
        .map(|manifest| manifest.manifest_path.clone())
        .collect::<HashSet<_>>();
    let artifact_root = canonical_root(artifact_root)?;
    let storage_root = canonical_root(storage_root)?;
    for candidate in &plan.orphan_paths {
        if manifest_cleanup_failed(candidate, &failed_manifests, &failed_dirs) {
            report
                .preserved_paths
                .push(candidate.path.display().to_string());
            continue;
        }
        match remove_managed_path(candidate, &artifact_root, &storage_root) {
            Ok(true) => report
                .removed_paths
                .push(candidate.path.display().to_string()),
            Ok(false) => {}
            Err(error) => {
                report
                    .preserved_paths
                    .push(candidate.path.display().to_string());
                report.failures.push(error);
            }
        }
    }
    report.removed_paths.extend(vm_report.removed_storage_paths);
    remove_empty_generated_parents(&artifact_root, &mut report);
    remove_empty_generated_parents(&storage_root, &mut report);
    report.removed_paths.sort();
    report.removed_paths.dedup();
    report.preserved_paths.sort();
    report.preserved_paths.dedup();
    Ok(report)
}

fn manifest_cleanup_failed(
    candidate: &PlannedPath,
    failed: &HashSet<PathBuf>,
    failed_dirs: &HashSet<String>,
) -> bool {
    if let Some(manifest) = candidate.manifest_path.as_ref() {
        if failed.contains(manifest)
            || manifest
                .parent()
                .is_some_and(|parent| failed_dirs.contains(&parent.display().to_string()))
        {
            return true;
        }
    }
    false
}

fn canonical_root(root: &Path) -> Result<PathBuf, String> {
    if !root.exists() {
        return Ok(root.to_path_buf());
    }
    let metadata = fs::symlink_metadata(root)
        .map_err(|e| format!("cannot inspect managed root {}: {e}", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "managed root must be a real directory: {}",
            root.display()
        ));
    }
    root.canonicalize()
        .map_err(|e| format!("cannot resolve managed root {}: {e}", root.display()))
}

fn remove_managed_path(
    candidate: &PlannedPath,
    artifact_root: &Path,
    storage_root: &Path,
) -> Result<bool, String> {
    let path = &candidate.path;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "cannot inspect orphan path {}: {error}",
                path.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "refusing to follow symlink at orphan path {}",
            path.display()
        ));
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("cannot resolve orphan path {}: {e}", path.display()))?;
    let managed = canonical.starts_with(artifact_root) || canonical.starts_with(storage_root);
    if !managed || canonical == artifact_root || canonical == storage_root {
        return Err(format!(
            "orphan path is outside a managed root: {}",
            path.display()
        ));
    }
    if metadata.is_dir() {
        fs::remove_dir_all(&canonical).map_err(|e| {
            format!(
                "cannot remove orphan directory {}: {e}",
                canonical.display()
            )
        })?;
    } else if metadata.is_file() {
        fs::remove_file(&canonical)
            .map_err(|e| format!("cannot remove orphan file {}: {e}", canonical.display()))?;
    } else {
        return Err(format!(
            "refusing to remove special orphan path {}",
            canonical.display()
        ));
    }
    Ok(true)
}

fn remove_empty_generated_parents(root: &Path, report: &mut MaintenanceReport) {
    if !root.exists() {
        return;
    }
    fn remove_empty(path: &Path, protected_root: &Path, report: &mut MaintenanceReport) {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let child = entry.path();
            if fs::symlink_metadata(&child)
                .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
            {
                remove_empty(&child, protected_root, report);
            }
        }
        if path != protected_root
            && fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
            && fs::remove_dir(path).is_ok()
        {
            report.removed_paths.push(path.display().to_string());
        }
    }
    if let Ok(root) = root.canonicalize() {
        remove_empty(&root, &root, report);
    }
}

pub async fn prune_runs(
    pool: &SqlitePool,
    artifact_root: &Path,
    storage_root: &Path,
    runtime: &VmActivityRuntime,
) -> Result<MaintenanceReport, String> {
    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT run_id, project_id, created_at FROM vm_activity_runs ORDER BY created_at DESC, run_id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("cannot list runs for pruning: {e}"))?;
    let active = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM vm_activity_runs WHERE status IN ('queued','running','cancelling')",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("cannot check active runs: {e}"))?;
    if active > 0 {
        return Err("cannot prune while a run is queued, running, or cancelling".into());
    }

    let mut seen_projects = HashSet::new();
    let mut retained = Vec::new();
    let mut deleted = Vec::new();
    for (run_id, project_id, _) in rows {
        if seen_projects.insert(project_id.clone()) {
            retained.push(run_id);
        } else {
            deleted.push((run_id, project_id));
        }
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("cannot begin prune transaction: {e}"))?;
    let mut removed_project_deliveries = HashSet::new();
    for (run_id, project_id) in &deleted {
        let ids = sqlx::query_scalar::<_, String>(
            "SELECT delivery_id FROM vm_activity_expedients WHERE run_id=?",
        )
        .bind(run_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("cannot read deliveries for run {run_id}: {e}"))?;
        removed_project_deliveries.extend(
            ids.into_iter()
                .map(|delivery_id| (project_id.clone(), delivery_id)),
        );
        sqlx::query("DELETE FROM vm_activity_expedients WHERE run_id=?")
            .bind(run_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("cannot delete expedients for run {run_id}: {e}"))?;
        sqlx::query("DELETE FROM vm_activity_runs WHERE run_id=?")
            .bind(run_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("cannot delete run {run_id}: {e}"))?;
    }
    let mut orphan_delivery_ids = HashSet::new();
    for (project_id, delivery_id) in removed_project_deliveries {
        sqlx::query("DELETE FROM project_assignments WHERE project_id=? AND delivery_id=? AND NOT EXISTS (SELECT 1 FROM vm_activity_expedients e JOIN vm_activity_runs r ON r.run_id=e.run_id WHERE e.delivery_id=? AND r.project_id=?)")
            .bind(&project_id).bind(&delivery_id).bind(&delivery_id).bind(&project_id)
            .execute(&mut *tx).await
            .map_err(|e| format!("cannot remove project assignments for delivery {delivery_id}: {e}"))?;
        orphan_delivery_ids.insert(delivery_id);
    }
    for delivery_id in orphan_delivery_ids {
        sqlx::query("DELETE FROM student_assignments WHERE delivery_id=? AND NOT EXISTS (SELECT 1 FROM vm_activity_expedients WHERE delivery_id=?)")
            .bind(&delivery_id).bind(&delivery_id).execute(&mut *tx).await
            .map_err(|e| format!("cannot remove assignments for delivery {delivery_id}: {e}"))?;
        sqlx::query("DELETE FROM vm_activity_deliveries WHERE delivery_id=? AND NOT EXISTS (SELECT 1 FROM vm_activity_expedients WHERE delivery_id=?)")
            .bind(&delivery_id).bind(&delivery_id).execute(&mut *tx).await
            .map_err(|e| format!("cannot remove unreferenced delivery {delivery_id}: {e}"))?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("cannot commit prune transaction: {e}"))?;

    let mut report = match clean_orphans(pool, artifact_root, storage_root, runtime).await {
        Ok(report) => report,
        Err(error) => MaintenanceReport {
            operation: "prune".into(),
            failures: vec![format!("cleanup after pruning failed: {error}")],
            ..MaintenanceReport::default()
        },
    };
    report.operation = "prune".into();
    report.retained_run_ids = retained;
    report.deleted_run_ids = deleted.into_iter().map(|(run_id, _)| run_id).collect();
    Ok(report)
}

pub async fn reset_data(
    pool: &SqlitePool,
    artifact_root: &Path,
    storage_root: &Path,
    runtime: &VmActivityRuntime,
) -> Result<MaintenanceReport, String> {
    let active = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM vm_activity_runs WHERE status IN ('queued','running','cancelling')",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("cannot check active runs: {e}"))?;
    if active > 0 {
        return Err("cannot reset while a run is queued, running, or cancelling".into());
    }
    let deleted =
        sqlx::query_scalar::<_, String>("SELECT run_id FROM vm_activity_runs ORDER BY run_id")
            .fetch_all(pool)
            .await
            .map_err(|e| format!("cannot list runs for reset: {e}"))?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("cannot begin reset transaction: {e}"))?;
    sqlx::query("DELETE FROM vm_activity_expedients")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("cannot delete expedients: {e}"))?;
    sqlx::query("DELETE FROM student_assignments")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("cannot delete assignments: {e}"))?;
    sqlx::query("DELETE FROM project_assignments")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("cannot delete project assignments: {e}"))?;
    sqlx::query("DELETE FROM project_students")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("cannot delete project student links: {e}"))?;
    sqlx::query("DELETE FROM thot_runtime_context")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("cannot delete current project context: {e}"))?;
    sqlx::query("DELETE FROM thot_projects")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("cannot delete projects: {e}"))?;
    sqlx::query("DELETE FROM vm_activity_runs")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("cannot delete runs: {e}"))?;
    sqlx::query("DELETE FROM vm_activity_deliveries")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("cannot delete deliveries: {e}"))?;
    sqlx::query("DELETE FROM students")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("cannot delete students: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("cannot commit reset transaction: {e}"))?;
    let mut report = match clean_orphans(pool, artifact_root, storage_root, runtime).await {
        Ok(report) => report,
        Err(error) => MaintenanceReport {
            operation: "reset".into(),
            failures: vec![format!("cleanup after reset failed: {error}")],
            ..MaintenanceReport::default()
        },
    };
    report.operation = "reset".into();
    report.deleted_run_ids = deleted;
    Ok(report)
}
