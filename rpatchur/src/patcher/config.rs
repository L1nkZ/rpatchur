use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use super::get_patcher_name;
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct PatcherConfiguration {
    pub window: WindowConfiguration,
    pub play: PlayConfiguration,
    pub setup: SetupConfiguration,
    pub web: WebConfiguration,
    pub client: ClientConfiguration,
    pub patching: PatchingConfiguration,
}

#[derive(Deserialize, Clone)]
pub struct WindowConfiguration {
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub resizable: bool,
}

#[derive(Deserialize, Clone)]
pub struct PlayConfiguration {
    pub path: String,
    pub arguments: Vec<String>,
    pub exit_on_success: Option<bool>,
}

#[derive(Deserialize, Clone)]
pub struct SetupConfiguration {
    pub path: String,
    pub arguments: Vec<String>,
    pub exit_on_success: Option<bool>,
}

#[derive(Deserialize, Clone)]
pub struct WebConfiguration {
    pub index_url: String, // URL of the index file implementing the UI
    pub preferred_patch_server: Option<String>, // Name of the patch server to use in priority
    pub patch_servers: Vec<PatchServerInfo>,
}

#[derive(Deserialize, Clone)]
pub struct PatchServerInfo {
    pub name: String,      // Name of that identifies the patch server
    pub plist_url: String, // URL of the plist.txt file
    pub patch_url: String, // URL of the directory containing .thor files
}

#[derive(Deserialize, Clone)]
pub struct ClientConfiguration {
    pub default_grf_name: String, // GRF file to patch by default
}

#[derive(Deserialize, Clone)]
pub struct PatchingConfiguration {
    pub in_place: bool,        // In-place GRF patching
    pub check_integrity: bool, // Check THOR archives' integrity
    pub create_grf: bool,      // Create new GRFs if they don't exist
}

pub fn retrieve_patcher_configuration(
    config_file_path: Option<PathBuf>,
) -> Result<PatcherConfiguration> {
    let patcher_name = get_patcher_name()?;
    // Use given configuration path if present
    let config_file_path =
        config_file_path.unwrap_or_else(|| PathBuf::from(patcher_name).with_extension("yml"));
    // Read the YAML content of the file as an instance of `PatcherConfiguration`.
    parse_configuration(config_file_path)
}

fn parse_configuration(config_file_path: impl AsRef<Path>) -> Result<PatcherConfiguration> {
    let config_file = File::open(config_file_path)?;
    let config_reader = BufReader::new(config_file);
    serde_yaml::from_reader(config_reader).context("Invalid configuration")
}
