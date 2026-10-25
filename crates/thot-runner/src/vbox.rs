//! Purpose: Exports the local VBoxManage runner and its command result types.
pub mod runner;

pub use runner::{LocalVirtualBoxRunner, OutputLimits, VBoxCommandOutput};
