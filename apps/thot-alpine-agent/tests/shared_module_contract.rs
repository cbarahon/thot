use std::{fs, path::Path};
use thot_core::sandbox_contracts::parse_test_module_manifest;

fn fixture(path: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/sandbox-contracts")
            .join(path),
    )
    .expect("shared sandbox contract fixture")
}

#[test]
fn alpine_agent_accepts_and_verifies_shared_module_fixture() {
    let manifest = parse_test_module_manifest(&fixture("module-manifest.json")).unwrap();
    manifest
        .verify_package_bytes(&fixture("module-package.fixture"))
        .unwrap();
    manifest
        .verify_asset_bytes("tests/required-os.json", &fixture("tests/required-os.json"))
        .unwrap();
    assert!(parse_test_module_manifest(&fixture("invalid-module-manifest.json")).is_err());
}
