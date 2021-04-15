use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct PatcherCache {
    pub last_patch_index: usize,
}

pub async fn read_cache_file(cache_file_path: impl AsRef<Path>) -> Result<PatcherCache> {
    let file = File::open(cache_file_path)?;
    serde_json::from_reader(file).context("Failed to deserialize patcher cache")
}

pub async fn write_cache_file(
    cache_file_path: impl AsRef<Path>,
    new_cache: PatcherCache,
) -> Result<()> {
    let file = File::create(cache_file_path)?;
    serde_json::to_writer(file, &new_cache).context("Failed to serialize patcher cache")
}
