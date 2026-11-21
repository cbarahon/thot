use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::ThotError;
use crate::kvm_alpine::modules::{Preflight, PrepareBaseline, CreateStudentEnvironment, DispatchTask, DestroyStudentEnvironment, ReconcileOwnedResources};
use crate::settings::Settings;

pub struct KvmAlpineHost {
    admin_policy: Arc<Settings>,
    preflight: Preflight,
    prepare_baseline: PrepareBaseline,
    create_student_environment: CreateStudentEnvironment,
    dispatch_task: DispatchTask,
    destroy_student_environment: DestroyStudentEnvironment,
    reconcile_owned_resources: ReconcileOwnedResources,
}

impl KvmAlpineHost {
    pub fn new(settings: Arc<Settings>) -> Result<Self, ThotError> {
        let preflight = Preflight::new(&settings)?;
        let prepare_baseline = PrepareBaseline::new(&settings)?;
        let create_student_environment = CreateStudentEnvironment::new(&settings)?;
        let dispatch_task = DispatchTask::new(&settings)?;
        let destroy_student_environment = DestroyStudentEnvironment::new(&settings)?;
        let reconcile_owned_resources = ReconcileOwnedResources::new(&settings)?;

        Ok(KvmAlpineHost {
            admin_policy: settings,
            preflight,
            prepare_baseline,
            create_student_environment,
            dispatch_task,
            destroy_student_environment,
            reconcile_owned_resources,
        })
    }

    pub async fn preflight(&self) -> Result<(), ThotError> {
        self.preflight.run().await
    }

    pub async fn prepare_baseline(&self) -> Result<(), ThotError> {
        self.prepare_baseline.run().await
    }

    pub async fn create_student_environment(&self) -> Result<(), ThotError> {
        self.create_student_environment.run().await
    }

    pub async fn dispatch_task(&self) -> Result<(), ThotError> {
        self.dispatch_task.run().await
    }

    pub async fn destroy_student_environment(&self) -> Result<(), ThotError> {
        self.destroy_student_environment.run().await
    }

    pub async fn reconcile_owned_resources(&self) -> Result<(), ThotError> {
        self.reconcile_owned_resources.run().await
    }
}