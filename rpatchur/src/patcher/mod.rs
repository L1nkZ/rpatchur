mod cache;
mod cancellation;
mod config;
mod core;
mod patching;

use std::env;
use std::ffi::OsString;
use std::io;

pub use self::config::{retrieve_patcher_configuration, PatcherConfiguration};
pub use self::core::patcher_thread_routine;

pub enum PatcherCommand {
    Start,
    Cancel, // Canceled by the user
}

pub fn get_patcher_name() -> io::Result<OsString> {
    let current_exe_path = env::current_exe()?;
    match current_exe_path.file_stem() {
        None => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Current executable path is invalid",
        )),
        Some(v) => Ok(v.to_os_string()),
    }
}
