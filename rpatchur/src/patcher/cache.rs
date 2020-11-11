use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct PatcherCache {
    pub last_patch_index: usize,
}

pub async fn read_cache_file<P: AsRef<Path>>(cache_file_path: P) -> Result<PatcherCache> {
    let file = File::open(cache_file_path)?;
    Ok(bincode::deserialize_from(file).context("Failed to deserialize patcher cache")?)
}

pub async fn write_cache_file<P: AsRef<Path>>(
    cache_file_path: P,
    new_cache: PatcherCache,
) -> Result<()> {
    let file = File::create(cache_file_path)?;
    Ok(bincode::serialize_into(file, &new_cache).context("Failed to serialize patcher cache")?)
}
