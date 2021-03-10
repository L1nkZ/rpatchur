use std::boxed::Box;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use crate::archive::{serialize_as_win1252_str_into, serialize_to_win1252, GenericFileEntry};
use crate::thor::{
    ThorMode, INTEGRITY_FILE_NAME, MULTIPLE_FILES_TABLE_DESC_SIZE, THOR_HEADER_MAGIC,
};
use crate::Result;
use crc::crc32::{self, Hasher32};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::Serialize;

const THOR_HEADER_FIXED_SIZE: usize = THOR_HEADER_MAGIC.len() + 0x8;

pub struct ThorArchiveBuilder<W: Write + Seek> {
    obj: Box<W>,
    entries: HashMap<String, Option<BuilderFileEntry>>,
    finished: bool,
    use_grf_merging: bool,
    target_grf_name: String,
    include_checksums: bool,
}

struct BuilderFileEntry {
    generic: GenericFileEntry,
    checksum: u32,
}

#[derive(Debug, Serialize)]
pub struct SerializableThorHeader<'a> {
    pub magic: &'a [u8; THOR_HEADER_MAGIC.len()],
    pub use_grf_merging: u8, // 0 -> client directory, 1 -> GRF
    pub file_count: u32,
    pub mode: i16,
    // Note: bincode doesn't serialize this the way we want. See
    // `serialize_thor_str_into`.
    // pub target_grf_name_size: u8,
    // pub target_grf_name: &'a [u8],
}

#[derive(Debug, Serialize)]
pub struct SerializableFileTableDesc {
    pub file_table_compressed_size: u32,
    pub file_table_offset: u32,
}

#[derive(Debug, Serialize)]
pub struct SerializableThorFileEntryAdd {
    // relative_path_size: u8,
    // relative_path: &'a [u8],
    flags: u8,
    offset: u32,
    size_compressed: u32,
    size: u32,
}

impl<W: Write + Seek> ThorArchiveBuilder<W> {
    pub fn new(
        mut obj: W,
        use_grf_merging: bool,
        target_grf_name: Option<String>,
        include_checksums: bool,
    ) -> Result<Self> {
        let target_grf_name = target_grf_name.unwrap_or_default();
        // Placeholder for the THOR header
        let place_holder =
            vec![
                0;
                THOR_HEADER_FIXED_SIZE + target_grf_name.len() + MULTIPLE_FILES_TABLE_DESC_SIZE
            ];
        obj.write_all(place_holder.as_slice())?;
        Ok(Self {
            obj: Box::new(obj),
            entries: HashMap::new(),
            finished: false,
            use_grf_merging,
            target_grf_name,
            include_checksums,
        })
    }

    pub fn append_file_update<R>(&mut self, entry_path: String, mut data: R) -> Result<()>
    where
        R: Read,
    {
        // Compress it
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        let (data_size, data_checksum) = if self.include_checksums {
            copy_and_measure_crc32(data.by_ref(), &mut encoder)?
        } else {
            (io::copy(data.by_ref(), &mut encoder)?, 0)
        };
        // Write compressed data
        let compressed_data = encoder.finish()?;
        let compressed_data_size = compressed_data.len();

        let offset = self.obj.seek(SeekFrom::Current(0))?;
        let mut compressed_reader = Cursor::new(compressed_data);
        let _ = io::copy(&mut compressed_reader, self.obj.by_ref())?;
        self.entries.insert(
            entry_path,
            Some(BuilderFileEntry {
                generic: GenericFileEntry {
                    offset,
                    size: u32::try_from(data_size)?,
                    size_compressed: u32::try_from(compressed_data_size)?,
                },
                checksum: data_checksum,
            }),
        );
        Ok(())
    }

    pub fn append_file_removal(&mut self, entry_path: String) {
        self.entries.insert(entry_path, None);
    }

    pub fn finish(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;

        // Append 'data.integrity' if needed
        if self.include_checksums {
            self.append_data_integrity()?;
        }
        let (file_table_offset, compressed_table_size) = self.write_file_table()?;
        // Update the header
        self.obj.seek(SeekFrom::Start(0))?;
        write_thor_header(
            &mut self.obj,
            self.use_grf_merging,
            self.entries.len(),
            self.target_grf_name.as_str(),
            compressed_table_size,
            file_table_offset,
        )
    }

    fn write_file_table(&mut self) -> Result<(u64, usize)> {
        let mut table: Vec<u8> = Vec::new();
        // Generate table and write files' content
        for (relative_path, entry) in &self.entries {
            let mut rel_path_win1252 = Vec::with_capacity(relative_path.len());
            serialize_as_win1252_str_into(&mut rel_path_win1252, relative_path)?;
            match entry {
                None => {
                    // No entry, this is a file removal
                    const REMOVE_FILE: u8 = 1;
                    serialize_thor_slice_into(&mut table, rel_path_win1252.as_slice())?;
                    bincode::serialize_into(&mut table, &REMOVE_FILE)?;
                }
                Some(entry) => {
                    // File update or file creation
                    let thor_file_entry = SerializableThorFileEntryAdd {
                        flags: 0,
                        offset: u32::try_from(entry.generic.offset)?,
                        size: entry.generic.size,
                        size_compressed: entry.generic.size_compressed,
                    };
                    serialize_thor_slice_into(&mut table, rel_path_win1252.as_slice())?;
                    bincode::serialize_into(&mut table, &thor_file_entry)?;
                }
            }
        }
        // Compress the table
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&table)?;
        let compressed_table = encoder.finish()?;
        let compressed_table_size = compressed_table.len();
        let table_offset = self.obj.seek(SeekFrom::Current(0))?;
        // Write table's content
        self.obj.write_all(&compressed_table)?;
        // Return file table's offset
        Ok((table_offset, compressed_table_size))
    }

    fn append_data_integrity(&mut self) -> Result<()> {
        let data_integrity_content = self.generate_data_integrity()?;
        self.append_file_update(
            INTEGRITY_FILE_NAME.to_string(),
            data_integrity_content.as_slice(),
        )?;
        Ok(())
    }

    fn generate_data_integrity(&self) -> Result<Vec<u8>> {
        let content = self.entries.iter().fold(String::new(), |acc, v| {
            if let Some(entry) = v.1 {
                acc + format!("{}=0x{:08x}\r\n", v.0, entry.checksum).as_str()
            } else {
                acc
            }
        });
        serialize_to_win1252(content.as_str())
    }
}

impl<W: Write + Seek> Drop for ThorArchiveBuilder<W> {
    // Automatically call finish on destruction
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn write_thor_header<W: Write>(
    writer: &mut W,
    use_grf_merging: bool,
    file_count: usize,
    target_grf_name: &str,
    file_table_compressed_size: usize,
    file_table_offset: u64,
) -> Result<()> {
    let use_grf_merging: u8 = if use_grf_merging { 1 } else { 0 };
    let grf_header = SerializableThorHeader {
        magic: THOR_HEADER_MAGIC,
        use_grf_merging,
        file_count: u32::try_from(file_count)?,
        mode: thor_mode_to_i16(ThorMode::MultipleFiles).unwrap(),
    };
    let table_desc = SerializableFileTableDesc {
        file_table_compressed_size: u32::try_from(file_table_compressed_size)?,
        file_table_offset: u32::try_from(file_table_offset)?,
    };
    bincode::serialize_into(writer.by_ref(), &grf_header)?;
    serialize_thor_str_into(writer.by_ref(), target_grf_name)?;
    bincode::serialize_into(writer.by_ref(), &table_desc)?;
    Ok(())
}

fn thor_mode_to_i16(mode: ThorMode) -> Option<i16> {
    match mode {
        ThorMode::SingleFile => Some(33),
        ThorMode::MultipleFiles => Some(48),
        ThorMode::Invalid => None,
    }
}

/// In THOR archives, `str`s are serialized to
/// [str length as u8]:[str bytes without a NULL terminator]
fn serialize_thor_str_into<W: Write>(mut writer: W, string: &str) -> Result<()> {
    let str_len_as_u8 = u8::try_from(string.len())?;
    bincode::serialize_into(writer.by_ref(), &str_len_as_u8)?;
    if !string.is_empty() {
        writer.write_all(string.as_bytes())?;
    }
    Ok(())
}

/// In THOR archives, `slice`s are serialized to
/// [slice size as u8]:[slice bytes]
fn serialize_thor_slice_into<W: Write>(mut writer: W, slice: &[u8]) -> Result<()> {
    let str_len_as_u8 = u8::try_from(slice.len())?;
    bincode::serialize_into(writer.by_ref(), &str_len_as_u8)?;
    if !slice.is_empty() {
        writer.write_all(slice)?;
    }
    Ok(())
}

/// Computes a CRC32 checksum from a reader.
fn copy_and_measure_crc32<R: ?Sized, W: ?Sized>(
    reader: &mut R,
    writer: &mut W,
) -> Result<(u64, u32)>
where
    R: Read,
    W: Write,
{
    // Use an 8KiB buffer
    let mut buf = [0_u8; 8 * 1024];
    let mut digest = crc32::Digest::new(crc::crc32::IEEE);
    let mut written = 0;
    loop {
        let len = match reader.read(&mut buf) {
            Ok(0) => return Ok((written, digest.sum32())),
            Ok(len) => len,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        digest.write(&buf[..len]);
        writer.write_all(&buf[..len])?;
        written += len as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thor::{ThorArchive, ThorFileEntry};
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_empty() {
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("builder.thor");
        let output_file = File::create(&output_path).unwrap();
        let mut builder = ThorArchiveBuilder::new(output_file, false, None, false).unwrap();
        builder.finish().unwrap();
    }

    #[test]
    fn test_header() {
        let temp_dir = tempdir().unwrap();
        {
            let output_path = temp_dir.path().join("builder1.thor");
            let output_file = File::create(&output_path).unwrap();
            let mut builder = ThorArchiveBuilder::new(output_file, false, None, false).unwrap();
            builder.finish().unwrap();
            let thor_archive = ThorArchive::open(&output_path).unwrap();
            assert_eq!(thor_archive.file_count(), 0);
            assert_eq!(thor_archive.target_grf_name(), "");
            assert!(!thor_archive.use_grf_merging());
        }
        {
            let output_path = temp_dir.path().join("builder2.thor");
            let output_file = File::create(&output_path).unwrap();
            let grf_name = "myserver.grf";
            let mut builder =
                ThorArchiveBuilder::new(output_file, true, Some(grf_name.to_string()), false)
                    .unwrap();
            builder.finish().unwrap();
            let thor_archive = ThorArchive::open(&output_path).unwrap();
            assert_eq!(thor_archive.file_count(), 0);
            assert_eq!(thor_archive.target_grf_name(), "myserver.grf");
            assert!(thor_archive.use_grf_merging());
        }
    }

    #[test]
    fn test_append_file_removal() {
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("builder.thor");
        {
            let output_file = File::create(&output_path).unwrap();
            let mut builder = ThorArchiveBuilder::new(output_file, false, None, false).unwrap();
            builder.append_file_removal("data/test1".to_string());
            builder.append_file_removal("data/test2".to_string());
        }
        {
            let thor_archive = ThorArchive::open(&output_path).unwrap();
            assert_eq!(thor_archive.file_count(), 2);
            assert_eq!(thor_archive.target_grf_name(), "");
            assert!(!thor_archive.use_grf_merging());
            let file_entries: Vec<ThorFileEntry> = thor_archive.get_entries().cloned().collect();
            for file_entry in file_entries {
                assert!(file_entry.is_removed);
            }
        }
    }

    #[test]
    fn test_append_file_update() {
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("builder.thor");
        let expected_content: HashMap<&str, Vec<u8>> =
            [("data\\test1", vec![1, 2, 3]), ("data\\test2", vec![5, 6])]
                .iter()
                .cloned()
                .collect();
        {
            let output_file = File::create(&output_path).unwrap();
            let mut builder = ThorArchiveBuilder::new(output_file, false, None, false).unwrap();
            for entry in &expected_content {
                builder
                    .append_file_update(entry.0.to_string(), entry.1.as_slice())
                    .unwrap();
            }
        }
        {
            let mut thor_archive = ThorArchive::open(&output_path).unwrap();
            assert_eq!(thor_archive.file_count(), expected_content.len());
            assert_eq!(thor_archive.target_grf_name(), "");
            assert!(!thor_archive.use_grf_merging());
            let file_entries: Vec<ThorFileEntry> = thor_archive.get_entries().cloned().collect();
            for file_entry in file_entries {
                assert!(!file_entry.is_removed);
                let file_path = file_entry.relative_path.as_str();
                assert!(expected_content.contains_key(file_path));
                let expected_content = &expected_content[file_path];
                let content = thor_archive.read_file_content(file_path).unwrap();
                assert_eq!(&content, expected_content);
            }
        }
    }

    #[test]
    fn test_data_integrity() {
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("builder.thor");
        let expected_content: HashMap<&str, Vec<u8>> =
            [("data\\test1", vec![1, 2, 3]), ("data\\test2", vec![5, 6])]
                .iter()
                .cloned()
                .collect();
        {
            let output_file = File::create(&output_path).unwrap();
            let mut builder = ThorArchiveBuilder::new(output_file, false, None, true).unwrap();
            for entry in &expected_content {
                builder
                    .append_file_update(entry.0.to_string(), entry.1.as_slice())
                    .unwrap();
            }
        }
        {
            let mut thor_archive = ThorArchive::open(&output_path).unwrap();
            assert!(thor_archive.is_valid().unwrap());
        }
    }
}
