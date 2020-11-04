use std::fs::File;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct PatcherCache {
    pub last_patch_index: usize,
}

pub async fn read_cache_file<P: AsRef<Path>>(cache_file_path: P) -> io::Result<PatcherCache> {
    let file = File::open(cache_file_path)?;
    match bincode::deserialize_from(file) {
        Ok(v) => Ok(v),
        Err(e) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to deserialize patcher cache: {}", e),
        )),
    }
}

pub async fn write_cache_file<P: AsRef<Path>>(
    cache_file_path: P,
    new_cache: PatcherCache,
) -> io::Result<()> {
    let file = File::create(cache_file_path)?;
    match bincode::serialize_into(file, &new_cache) {
        Ok(_) => Ok(()),
        Err(e) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to serialize patcher cache: {}", e),
        )),
    }
}
