mod cache;
mod config;
mod core;
mod patching;

use std::env;
use std::ffi::OsString;

pub use self::config::{retrieve_patcher_configuration, PatcherConfiguration};
pub use self::core::patcher_thread_routine;

pub enum PatcherCommand {
    Start,
    Cancel, // Canceled by the user
    Exit,   // Program is exiting
}

pub fn get_patcher_name() -> Option<OsString> {
    match env::current_exe() {
        Err(_) => None,
        Ok(v) => match v.file_stem() {
            Some(v) => Some(v.to_os_string()),
            None => None,
        },
    }
}
