use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct PatchDefinition {
    #[serde(default)] // Defaults to false
    pub include_checksums: bool,
    pub use_grf_merging: bool,
    pub target_grf_name: Option<String>,
    pub entries: Vec<PatchEntry>,
}

#[derive(Deserialize, Clone)]
pub struct PatchEntry {
    pub relative_path: String,
    #[serde(default)] // Defaults to false
    pub is_removed: bool,
    pub in_grf_path: Option<String>
}

pub fn parse_patch_definition(file_path: impl AsRef<Path>) -> Result<PatchDefinition> {
    let file = File::open(file_path)?;
    let file_reader = BufReader::new(file);
    let patch_definition = serde_yaml::from_reader(file_reader).context("Invalid configuration")?;
    Ok(patch_definition)
}
