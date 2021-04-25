mod cache;
mod cancellation;
mod config;
mod core;
mod patching;

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

pub use self::config::{retrieve_patcher_configuration, PatcherConfiguration};
pub use self::core::patcher_thread_routine;
use anyhow::{Context, Result};

pub enum PatcherCommand {
    StartUpdate,
    CancelUpdate,        // Canceled by the user
    ApplyPatch(PathBuf), // Manual patch submitted by the user
    Quit,                // Exit requested
}

pub fn get_patcher_name() -> Result<OsString> {
    let current_exe_path = env::current_exe()?;
    Ok(current_exe_path
        .file_stem()
        .context("Current executable path is invalid")?
        .to_os_string())
}
