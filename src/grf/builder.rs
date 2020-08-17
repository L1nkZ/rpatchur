use std::convert::TryFrom;
use std::io;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use crate::grf::{GrfArchive, GRF_HEADER_MAGIC, GRF_HEADER_SIZE};
use crate::thor::ThorArchive;
use encoding::label::encoding_from_whatwg_label;
use encoding::EncoderTrap;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::Serialize;

const GRF_FIXED_KEY: [u8; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];

pub struct GrfArchiveBuilder<W: Write + Seek> {
    obj: Option<W>,
    start_offset: u64,
    finished: bool,
    version_major: u32,
    version_minor: u32,
    entries: Vec<GenericFileEntry>,
}

struct GenericFileEntry {
    pub relative_path: String,
    // Note(LinkZ): u32 limited by the GRF file format
    pub size: u32,
    pub size_compressed: u32,
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

fn serialize_win1252cstring<W>(string: &str, mut writer: W) -> io::Result<()>
where
    W: Write,
{
    let decoder = match encoding_from_whatwg_label("windows-1252") {
        Some(v) => v,
        None => return Err(io::Error::new(io::ErrorKind::Other, "Encoder unavailable")),
    };
    let mut vec = match decoder.encode(string, EncoderTrap::Strict) {
        Ok(v) => v,
        Err(_) => return Err(io::Error::new(io::ErrorKind::Other, "Encoding failed")),
    };
    vec.push(0); // NUL char terminator
    writer.write_all(vec.as_slice())
}

impl<W: Write + Seek> GrfArchiveBuilder<W> {
    pub fn new(
        mut obj: W,
        version_major: u32,
        version_minor: u32,
    ) -> io::Result<GrfArchiveBuilder<W>> {
        let start_offset = obj.seek(io::SeekFrom::Current(0)).unwrap_or(0);
        // Placeholder for the GRF header
        obj.write_all(&[0; GRF_HEADER_SIZE])?;
        Ok(GrfArchiveBuilder {
            obj: Some(obj),
            start_offset,
            finished: false,
            version_major,
            version_minor,
            entries: Vec::new(),
        })
    }

    pub fn import_raw_entry_from_grf(
        &mut self,
        archive: &mut GrfArchive,
        relative_path: String,
    ) -> io::Result<()> {
        match &mut self.obj {
            None => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Inner object was already closed",
            )),
            Some(w) => {
                let entry = match archive.get_file_entry(&relative_path) {
                    None => return Err(io::Error::new(io::ErrorKind::NotFound, "File not found")),
                    Some(v) => v.clone(),
                };
                let content = archive.get_entry_raw_data(&relative_path)?;
                let mut content_reader = Cursor::new(content);
                let content_size = io::copy(&mut content_reader, w.by_ref())?;
                debug_assert_eq!(entry.size_compressed_aligned as u64, content_size);
                self.entries.push(GenericFileEntry {
                    relative_path,
                    size: entry.size as u32,
                    size_compressed: entry.size_compressed_aligned as u32,
                });
                Ok(())
            }
        }
    }

    pub fn import_raw_entry_from_thor<R: Read + Seek>(
        &mut self,
        archive: &mut ThorArchive<R>,
        relative_path: String,
    ) -> io::Result<()> {
        match &mut self.obj {
            None => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Inner object was already closed",
            )),
            Some(w) => {
                let entry = match archive.get_file_entry(&relative_path) {
                    None => return Err(io::Error::new(io::ErrorKind::NotFound, "File not found")),
                    Some(v) => v.clone(),
                };
                let content = archive.get_entry_raw_data(&relative_path)?;
                let mut content_reader = Cursor::new(content);
                let _ = io::copy(&mut content_reader, w.by_ref())?;
                self.entries.push(GenericFileEntry {
                    relative_path,
                    size: entry.size as u32,
                    size_compressed: entry.size_compressed as u32,
                });
                Ok(())
            }
        }
    }

    pub fn append_file<R: Read>(&mut self, relative_path: String, mut data: R) -> io::Result<()> {
        match &mut self.obj {
            Some(w) => {
                // Compress it
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                let data_size = io::copy(data.by_ref(), &mut encoder)?;
                let data_size_u32 = match u32::try_from(data_size) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "File too big"))
                    }
                };
                // Write compressed data
                let mut compressed_reader = Cursor::new(encoder.finish()?);
                let compressed_data_size = io::copy(&mut compressed_reader, w.by_ref())?;
                let compressed_data_size_u32 = u32::try_from(compressed_data_size).unwrap();
                self.entries.push(GenericFileEntry {
                    relative_path,
                    size: data_size_u32,
                    size_compressed: compressed_data_size_u32,
                });
                Ok(())
            }
            None => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Inner object was already closed",
            )),
        }
    }

    pub fn finish(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;

        let v_file_count = match i32::try_from(self.entries.len() + 7) {
            Ok(v) => v,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Too many file entries",
                ))
            }
        };
        let file_table_offset = match self.version_major {
            2 => self.write_grf_table_200()?,
            1 => 0, // TODO(LinkZ): Implement
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Wrong file format version",
                ))
            }
        };
        match &mut self.obj {
            Some(w) => {
                // Update the header
                w.seek(SeekFrom::Start(self.start_offset))?;
                write_grf_header(
                    (self.version_major << 8) | (self.version_minor),
                    file_table_offset,
                    v_file_count,
                    w,
                )?;
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Inner object was already closed",
                ))
            }
        }

        Ok(())
    }

    fn write_grf_table_200(&mut self) -> io::Result<u32> {
        let mut table: Vec<u8> = Vec::new();
        let mut current_offset: u32 = 0;
        // Generate table and write files' content
        for entry in &self.entries {
            let grf_file_entry = SerializableGrfFileEntry200 {
                size_compressed: entry.size_compressed,
                size_compressed_aligned: entry.size_compressed,
                size: entry.size,
                entry_type: 1,
                offset: current_offset,
            };
            current_offset += entry.size_compressed;
            serialize_win1252cstring(&entry.relative_path, &mut table)?;
            bincode::serialize_into(&mut table, &grf_file_entry).unwrap();
        }
        let table_size = u32::try_from(table.len()).unwrap();
        // Compress it
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&table)?;
        let compressed_table = encoder.finish()?;
        let compressed_table_size = u32::try_from(compressed_table.len()).unwrap();
        match &mut self.obj {
            Some(w) => {
                // Write table's offset and size
                bincode::serialize_into(w.by_ref(), &compressed_table_size).unwrap();
                bincode::serialize_into(w.by_ref(), &table_size).unwrap();
                // Write table's content
                w.write_all(&compressed_table)?;
            }
            None => {}
        }
        // Return file table's offset
        Ok(current_offset)
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
) -> io::Result<()> {
    let grf_header = SerializableGrfHeader {
        key: GRF_FIXED_KEY,
        file_table_offset,
        seed: 0,
        v_file_count,
        version,
    };
    writer.write_all(GRF_HEADER_MAGIC.as_bytes())?;
    match bincode::serialize_into(writer, &grf_header) {
        Ok(_) => (),
        Err(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Failed to serialize header",
            ))
        }
    }
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
    fn test_append_file() {
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("200-builder.grf");
        let test_content: HashMap<&str, Vec<u8>> = [
            ("data\\file.gat", vec![0u8; 60]),
            ("data\\subfolder\\file.gnd", vec![0xCCu8; 341]),
        ]
        .iter()
        .cloned()
        .collect();
        // Generate
        {
            let output_file = File::create(&output_path).unwrap();
            let mut builder = GrfArchiveBuilder::new(output_file, 2, 0).unwrap();
            for test_file in &test_content {
                builder
                    .append_file(test_file.0.to_string(), test_file.1.as_slice())
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
                assert!(test_content.contains_key(file_path));
                let expected_content = &test_content[file_path];
                // Size check
                assert_eq!(expected_content.len(), entry.size);
                // Content check
                assert_eq!(
                    expected_content,
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
            let mut builder = GrfArchiveBuilder::new(output_file, 2, 0).unwrap();
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
