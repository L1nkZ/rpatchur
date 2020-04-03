use std::fs;
use std::io;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use crate::thor::*;

// pub fn apply_patch_to_grf<R: Read + Seek>(grf_file_path: &Path, thor_archive: &ThorArchive<R>) {}

pub fn apply_patch_to_disk<R: Read + Seek>(
    root_directory: &Path,
    thor_archive: &mut ThorArchive<R>,
) -> io::Result<()> {
    // TODO(LinkZ): Do not extract data.integrity
    // TODO(LinkZ): Save original files before updating/removing them in order
    // to be able to restore them in case of failure
    // TODO(LinkZ): Make async?
    let file_entries: Vec<ThorFileEntry> = thor_archive.get_entries().map(|e| e.clone()).collect();
    for entry in file_entries {
        let dest_path = join_windows_relative_path(&root_directory, &entry.relative_path);
        if entry.is_removed {
            fs::remove_file(dest_path)?;
        } else {
            // Create parent directory if needed
            match dest_path.parent() {
                Some(parent_dir) => fs::create_dir_all(parent_dir)?,
                None => {}
            }
            // Extract file
            thor_archive.extract_file(&entry.relative_path, &dest_path)?;
        }
    }
    Ok(())
}

fn join_windows_relative_path(path: &Path, windows_relative_path: &str) -> PathBuf {
    let mut result = PathBuf::from(path);
    for component in windows_relative_path.split('\\') {
        result.push(component);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_apply_patch_to_disk() {
        let thor_dir_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/tests/thor");
        {
            let temp_dir = tempdir().unwrap();
            let expected_file_path = temp_dir
                .path()
                .join("data/wav/se_subterranean_rustyengine.wav");
            let thor_archive_path = thor_dir_path.join("small.thor");
            let mut thor_archive = ThorArchive::open(&thor_archive_path).unwrap();
            assert!(!expected_file_path.exists());
            apply_patch_to_disk(temp_dir.path(), &mut thor_archive).unwrap();
            assert!(expected_file_path.exists());
            // Just in case something goes wrong when removing the directory
            temp_dir.close().unwrap();
        }
    }
}
