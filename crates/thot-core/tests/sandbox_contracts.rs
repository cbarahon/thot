use thot_core::sandbox_contracts::{
    HostCapabilities, NestedVirtualization, PlatformCapability, PreparationSpec, ProviderKind,
    ResourceCeiling, SandboxPlatformRequest, TestModuleReference, parse_test_module_manifest,
};

fn platform_request(nested_virtualization_required: bool) -> SandboxPlatformRequest {
    SandboxPlatformRequest {
        contract_version: 1,
        platform_id: "kvm-alpine-main".into(),
        provider: ProviderKind::KvmAlpine,
        isolation_scope: "kvm-guest".into(),
        vcpu: 2,
        memory_mib: 2048,
        disk_bytes: 8 * 1024 * 1024 * 1024,
        nested_virtualization_required,
        preparation: PreparationSpec {
            timeout_seconds: 60,
            network_policy: "offline".into(),
            packages: vec![],
            scripts: vec![],
        },
        network_policy: "offline".into(),
    }
}

fn host_capabilities(nested: NestedVirtualization) -> HostCapabilities {
    HostCapabilities {
        provider: ProviderKind::KvmAlpine,
        provider_version: "test".into(),
        capabilities: vec![],
        nested_virtualization: nested,
        resource_ceiling: ResourceCeiling {
            vcpu: 4,
            memory_mib: 4096,
            disk_bytes: 16 * 1024 * 1024 * 1024,
            concurrent_environments: 1,
        },
    }
}

#[test]
fn admission_rejects_unavailable_required_nesting_and_resource_overage() {
    assert!(
        host_capabilities(NestedVirtualization::Unavailable)
            .validate_admission(&platform_request(true))
            .is_err()
    );
    let mut over_limit = platform_request(false);
    over_limit.memory_mib = 8192;
    assert!(
        host_capabilities(NestedVirtualization::NotRequested)
            .validate_admission(&over_limit)
            .is_err()
    );
}

#[test]
fn module_preflight_rejects_a_missing_versioned_capability() {
    let module = TestModuleReference {
        module_id: "thot.ova.partition-table".into(),
        version: "1.0.0".into(),
        package_sha256: "a".repeat(64),
        manifest_version: 1,
        capabilities: vec![PlatformCapability {
            id: "disk.inspect".into(),
            version: 1,
        }],
    };
    assert!(
        host_capabilities(NestedVirtualization::NotRequested)
            .validate_module(&module)
            .is_err()
    );
}

#[test]
fn manifest_parser_rejects_unversioned_unknown_fields() {
    let manifest = serde_json::json!({
        "manifest_version": 1,
        "module_id": "thot.ova.partition-table",
        "version": "1.0.0",
        "package_sha256": "a".repeat(64),
        "capabilities": [],
        "assets": [],
        "runtime": "unreviewed-shell"
    });
    assert!(parse_test_module_manifest(&serde_json::to_vec(&manifest).unwrap()).is_err());
}

#[test]
fn schema_v3_accepts_independent_evaluation_items_and_v2_stays_legacy() {
    use thot_core::sandbox_project::{SandboxCriterion, SandboxEvaluationItem, SandboxProject};

    let source = include_str!("../../../examples/kvm-alpine-poc/configuration.thot.yaml");
    let mut project: SandboxProject = serde_yaml::from_str(source).unwrap();
    project.schema_version = "thot.dev/v3".into();
    let evaluation = &mut project.sandbox_platforms[0].evaluation;
    evaluation.rubric = None;
    evaluation.items = vec![
        SandboxEvaluationItem {
            id: "system-layout".into(),
            title: "System layout".into(),
            calculation: "weighted_sum".into(),
            scale: 10,
            criteria: vec![SandboxCriterion {
                check: "syntax_check".into(),
                points: 10,
            }],
        },
        SandboxEvaluationItem {
            id: "boot-and-shared-data".into(),
            title: "Boot and shared data".into(),
            calculation: "weighted_sum".into(),
            scale: 10,
            criteria: vec![SandboxCriterion {
                check: "functional_check".into(),
                points: 10,
            }],
        },
    ];
    project.validate().unwrap();

    project.sandbox_platforms[0].evaluation.items[1].id = "system-layout".into();
    assert!(
        project
            .validate()
            .unwrap_err()
            .contains("duplicate evaluation item")
    );

    project.schema_version = "thot.dev/v2".into();
    assert!(project.validate().is_err());
}
