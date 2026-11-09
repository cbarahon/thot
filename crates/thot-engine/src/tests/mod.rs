//! Purpose: Dispatches configured test references to external, SSH, or VirtualBox adapters.
pub mod aggr;
#[path = "extern.rs"]
pub mod extern_tests;
pub mod import_csv;
pub mod libguestfs;
pub mod network;
pub mod ssh;
pub mod stats;
pub mod virtualbox;

use crate::test_execution::{TestInvocation, TestOutcome};

pub async fn dispatch(invocation: &TestInvocation) -> TestOutcome {
    if invocation.test_reference.starts_with("extern/") {
        return extern_tests::run(invocation).await;
    }
    if invocation.test_reference.starts_with("stats/") {
        return stats::run(invocation).await;
    }
    if invocation.test_reference.starts_with("aggr/") {
        return aggr::run(invocation).await;
    }
    if invocation.test_reference == "network/nmap" {
        return network::run(invocation).await;
    }
    if invocation.test_reference.starts_with("libguestfs/") {
        return libguestfs::run(invocation).await;
    }
    match invocation.test_reference.as_str() {
        "import/csv" => import_csv::run(invocation).await,
        "ssh/get_host_info" => ssh::run(invocation).await,
        reference if reference.starts_with("VirtualBox/") => virtualbox::run(invocation).await,
        _ => TestOutcome::error(format!(
            "unknown test reference {:?}",
            invocation.test_reference
        )),
    }
}
