use std::boxed::Box;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs::{File, OpenOptions};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::archive::{serialize_as_win1252_cstr_into, GenericFileEntry};
use crate::grf::dyn_alloc::{self, AvailableChunkList};
use crate::grf::{GrfArchive, GRF_HEADER_MAGIC, GRF_HEADER_SIZE};
use crate::thor::ThorArchive;
use crate::{GrufError, Result};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::Serialize;

const GRF_FIXED_KEY: [u8; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];

pub struct GrfArchiveBuilder<W: Write + Seek> {
    obj: Box<W>,
    start_offset: u64,
    finished: bool,
    version_major: u32,
    version_minor: u32,
    entries: HashMap<String, GenericFileEntry>,
    chunks: AvailableChunkList,
}

#[derive(Debug, Serialize)]
struct SerializableGrfHeader {
    pub key: [u8; 14],
    pub file_table_offset: u32,
    pub seed: i32,
    pub v_file_count: i32,
    pub version: u32,
}

#[derive(Debug, Serialize)]
struct SerializableGrfFileEntry200 {
    // Note(LinkZ): relative_path isn't fixed-length
    // relative_path: String,
    size_compressed: u32,
    size_compressed_aligned: u32,
    size: u32,
    entry_type: u8,
    offset: u32,
}

impl<W: Write + Seek> GrfArchiveBuilder<W> {
    pub fn create(mut obj: W, version_major: u32, version_minor: u32) -> Result<Self> {
        let start_offset = obj.seek(io::SeekFrom::Current(0)).unwrap_or(0);
        // Placeholder for the GRF header
        obj.write_all(&[0; GRF_HEADER_SIZE])?;
        Ok(Self {
            obj: Box::new(obj),
            start_offset,
            finished: false,
            version_major,
            version_minor,
            entries: HashMap::new(),
            chunks: AvailableChunkList::new(),
        })
    }

    pub fn import_raw_entry_from_grf(
        &mut self,
        archive: &mut GrfArchive,
        relative_path: String,
    ) -> Result<()> {
        let entry = archive
            .get_file_entry(&relative_path)
            .ok_or(GrufError::EntryNotFound)?
            .clone();
        let content = archive.get_entry_raw_data(&relative_path)?;
        let offset = {
            if let Some(grf_entry) = self.entries.get(&relative_path) {
                self.chunks.realloc_chunk(
                    grf_entry.offset,
                    grf_entry.size_compressed as usize,
                    content.len(),
                )?
            } else {
                self.chunks.alloc_chunk(content.len())?
            }
        };

        self.obj.seek(SeekFrom::Start(self.start_offset + offset))?;
        let mut content_reader = Cursor::new(content);
        let content_size = io::copy(&mut content_reader, self.obj.by_ref())?;
        debug_assert_eq!(entry.size_compressed_aligned as u64, content_size);
        self.entries.insert(
            relative_path,
            GenericFileEntry {
                offset,
                size: entry.size as u32,
                size_compressed: entry.size_compressed_aligned as u32,
            },
        );
        Ok(())
    }

    pub fn import_raw_entry_from_thor<R: Read + Seek>(
        &mut self,
        thor_archive: &mut ThorArchive<R>,
        relative_path: String,
    ) -> Result<()> {
        let entry = thor_archive
            .get_file_entry(&relative_path)
            .ok_or(GrufError::EntryNotFound)?
            .clone();
        let content = thor_archive.get_entry_raw_data(&relative_path)?;
        let offset = {
            if let Some(grf_entry) = self.entries.get(&relative_path) {
                self.chunks.realloc_chunk(
                    grf_entry.offset,
                    grf_entry.size_compressed as usize,
                    content.len(),
                )?
            } else {
                self.chunks.alloc_chunk(content.len())?
            }
        };

        self.obj.seek(SeekFrom::Start(self.start_offset + offset))?;
        let mut content_reader = Cursor::new(content);
        let _ = io::copy(&mut content_reader, self.obj.by_ref())?;
        self.entries.insert(
            relative_path,
            GenericFileEntry {
                offset,
                size: entry.size as u32,
                size_compressed: entry.size_compressed as u32,
            },
        );
        Ok(())
    }

    pub fn add_file<R: Read>(&mut self, relative_path: String, mut data: R) -> Result<()> {
        // Compress it
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        let data_size = io::copy(data.by_ref(), &mut encoder)?;
        let data_size_u32 = u32::try_from(data_size)?;
        // Write compressed data
        let compressed_data = encoder.finish()?;
        let compressed_data_size = compressed_data.len();
        let offset = {
            if let Some(grf_entry) = self.entries.get(&relative_path) {
                self.chunks.realloc_chunk(
                    grf_entry.offset,
                    grf_entry.size_compressed as usize,
                    compressed_data_size,
                )?
            } else {
                self.chunks.alloc_chunk(compressed_data_size)?
            }
        };

        self.obj.seek(SeekFrom::Start(self.start_offset + offset))?;
        let mut compressed_reader = Cursor::new(compressed_data);
        let _ = io::copy(&mut compressed_reader, self.obj.by_ref())?;
        let compressed_data_size_u32 = u32::try_from(compressed_data_size)?;
        self.entries.insert(
            relative_path,
            GenericFileEntry {
                offset,
                size: data_size_u32,
                size_compressed: compressed_data_size_u32,
            },
        );
        Ok(())
    }

    pub fn remove_file<S: AsRef<str>>(&mut self, relative_path: S) -> Result<bool> {
        if let Some(entry) = self.entries.remove(relative_path.as_ref()) {
            self.chunks
                .free_chunk(entry.offset, entry.size_compressed as usize)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn finish(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;

        let v_file_count = i32::try_from(self.entries.len() + 7)?;
        let file_table_offset = match self.version_major {
            2 => self.write_grf_table_200()?,
            1 => std::unimplemented!(), // TODO(LinkZ): Implement
            _ => return Err(GrufError::serialization_error("Wrong file format version")),
        };
        // Update the header
        self.obj.seek(SeekFrom::Start(self.start_offset))?;
        write_grf_header(
            (self.version_major << 8) | (self.version_minor),
            (file_table_offset - GRF_HEADER_SIZE as u64) as u32,
            v_file_count,
            &mut self.obj,
        )
    }

    fn write_grf_table_200(&mut self) -> Result<u64> {
        let mut table: Vec<u8> = Vec::new();
        // Generate table and write files' content
        for (relative_path, entry) in &self.entries {
            let grf_file_entry = SerializableGrfFileEntry200 {
                size_compressed: entry.size_compressed,
                size_compressed_aligned: entry.size_compressed,
                size: entry.size,
                entry_type: 1,
                offset: (entry.offset - GRF_HEADER_SIZE as u64) as u32,
            };
            serialize_as_win1252_cstr_into(&mut table, &relative_path)?;
            bincode::serialize_into(&mut table, &grf_file_entry)?;
        }
        // Compress the table
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&table)?;
        let compressed_table = encoder.finish()?;
        let compressed_table_size = compressed_table.len();
        let table_offset = self
            .chunks
            .alloc_chunk(compressed_table_size + 2 * std::mem::size_of::<u32>())?;
        let table_size_u32 = u32::try_from(table.len())?;
        let compressed_table_size_u32 = u32::try_from(compressed_table_size)?;
        self.obj
            .seek(SeekFrom::Start(self.start_offset + table_offset))?;
        // Write table's offset and size
        bincode::serialize_into(self.obj.by_ref(), &compressed_table_size_u32)?;
        bincode::serialize_into(self.obj.by_ref(), &table_size_u32)?;
        // Write table's content
        self.obj.write_all(&compressed_table)?;
        // Return file table's offset
        Ok(table_offset)
    }
}

impl GrfArchiveBuilder<File> {
    pub fn open<P: AsRef<Path>>(grf_path: P) -> Result<Self> {
        let mut grf_archive = GrfArchive::open(&grf_path)?;
        let chunks = dyn_alloc::list_available_chunks(&mut grf_archive)?;
        let mut entries = HashMap::with_capacity(grf_archive.file_count());
        for entry in grf_archive.get_entries() {
            entries.insert(
                entry.relative_path.clone(),
                GenericFileEntry {
                    offset: entry.offset,
                    size: entry.size as u32,
                    size_compressed: entry.size_compressed_aligned as u32,
                },
            );
        }

        let file = OpenOptions::new().read(true).write(true).open(&grf_path)?;
        Ok(Self {
            obj: Box::new(file),
            start_offset: 0,
            finished: false,
            version_major: grf_archive.version_major(),
            version_minor: grf_archive.version_minor(),
            entries,
            chunks,
        })
    }
}

impl<W: Write + Seek> Drop for GrfArchiveBuilder<W> {
    // Automatically call finish on destruction
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn write_grf_header<W: Write>(
    version: u32,
    file_table_offset: u32,
    v_file_count: i32,
    writer: &mut W,
) -> Result<()> {
    let grf_header = SerializableGrfHeader {
        key: GRF_FIXED_KEY,
        file_table_offset,
        seed: 0,
        v_file_count,
        version,
    };
    writer.write_all(GRF_HEADER_MAGIC.as_bytes())?;
    bincode::serialize_into(writer, &grf_header)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs::File;
    use std::path::PathBuf;

    use crate::grf::{GrfArchive, GrfArchiveBuilder, GrfFileEntry};
    use tempfile::tempdir;

    #[test]
    fn test_add_file() {
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("200-builder.grf");
        let test_content = vec![
            ("data\\file.gat", vec![0u8; 60]),
            ("data\\subfolder\\file.gnd", vec![0xCCu8; 341]),
            ("data\\file.gat", (0..129).collect()), // Overwrite
            ("data\\file2.gat", vec![3u8; 60]),
        ];
        let expected_content: HashMap<&str, Vec<u8>> = [
            ("data\\file.gat", (0..129).collect()),
            ("data\\file2.gat", vec![3u8; 60]),
            ("data\\subfolder\\file.gnd", vec![0xCCu8; 341]),
        ]
        .iter()
        .cloned()
        .collect();
        // Generate
        {
            let output_file = File::create(&output_path).unwrap();
            let mut builder = GrfArchiveBuilder::create(output_file, 2, 0).unwrap();
            for (name, content) in &test_content {
                builder
                    .add_file(name.to_string(), content.as_slice())
                    .unwrap();
            }
            // Call finish manually, even though builder will be dropped on scope exit
            builder.finish().unwrap();
        }
        // Check result
        {
            let mut grf_archive = GrfArchive::open(&output_path).unwrap();
            let file_entries: Vec<GrfFileEntry> = grf_archive.get_entries().cloned().collect();
            for entry in file_entries {
                let file_path: &str = entry.relative_path.as_str();
                assert!(expected_content.contains_key(file_path));
                let expected_data = &expected_content[file_path];
                // Size check
                assert_eq!(expected_data.len(), entry.size);
                // Content check
                assert_eq!(
                    expected_data,
                    &grf_archive.read_file_content(file_path).unwrap()
                );
            }
        }
    }

    #[test]
    fn test_import_raw_entry_from_grf() {
        let grf_dir_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/tests/grf");
        let grf_path = grf_dir_path.join("200-small.grf");
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("200-builder.grf");
        // Generate
        {
            let mut grf = GrfArchive::open(&grf_path).unwrap();
            let output_file = File::create(&output_path).unwrap();
            let mut builder = GrfArchiveBuilder::create(output_file, 2, 0).unwrap();
            let grf_entries: Vec<GrfFileEntry> = grf.get_entries().cloned().collect();
            for entry in grf_entries {
                builder
                    .import_raw_entry_from_grf(&mut grf, entry.relative_path)
                    .unwrap();
            }
        }
        // Check result
        {
            let mut grf = GrfArchive::open(&grf_path).unwrap();
            let mut ouput_archive = GrfArchive::open(&output_path).unwrap();
            let file_entries: Vec<GrfFileEntry> = ouput_archive.get_entries().cloned().collect();
            for entry in file_entries {
                let expected_content = grf.read_file_content(&entry.relative_path).unwrap();
                // Size check
                assert_eq!(expected_content.len(), entry.size);
                // Content check
                assert_eq!(
                    expected_content,
                    ouput_archive
                        .read_file_content(&entry.relative_path)
                        .unwrap()
                );
            }
        }
    }
}
