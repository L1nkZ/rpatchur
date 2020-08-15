use std::borrow::Cow;
use std::boxed::Box;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::str;

use crate::grf::crypto::{decrypt_file_content, decrypt_file_name};
use encoding::label::encoding_from_whatwg_label;
use encoding::DecoderTrap;
use flate2::read::ZlibDecoder;
use nom::error::ErrorKind;
use nom::number::complete::{le_i32, le_u32, le_u8};
use nom::IResult;
use nom::*;

pub const GRF_HEADER_MAGIC: &str = "Master of Magic\0";
// Packed structs' sizes in bytes
pub const GRF_HEADER_SIZE: usize = GRF_HEADER_MAGIC.len() + 0x1E;

#[derive(Debug)]
pub struct GrfArchive {
    obj: Box<File>,
    container: GrfContainer,
}

impl GrfArchive {
    /// Create a new archive with the underlying object as the reader.
    pub fn open(grf_path: &Path) -> io::Result<GrfArchive> {
        let mut file = File::open(grf_path)?;
        // TODO(LinkZ): Avoid using read_to_end, reading the whole file is unnecessary
        let mut buf = vec![];
        let _bytes_read = file.read_to_end(&mut buf)?;
        let (parser_output, grf_header) = match parse_grf_header(buf.as_slice()) {
            IResult::Ok(v) => v,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Failed to parse archive",
                ))
            }
        };

        match grf_header.version_major {
            2 => {
                let (parser_output, grf_table_info) = match parse_grf_table_info_200(
                    &parser_output[grf_header.file_table_offset as usize..],
                ) {
                    IResult::Ok(v) => v,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Failed to parse archive",
                        ))
                    }
                };
                if grf_table_info.table_size_compressed == 0 || grf_table_info.table_size == 0 {
                    return Ok(GrfArchive {
                        obj: Box::new(file),
                        container: GrfContainer {
                            header: grf_header,
                            table_info: GrfTableInfo::Compressed(grf_table_info),
                            entries: HashMap::new(),
                        },
                    });
                }
                // Decompress the table with zlib
                let mut decoder =
                    ZlibDecoder::new(&parser_output[..grf_table_info.table_size_compressed]);
                let mut decompressed_table = vec![];
                let _decompressed_size = match decoder.read_to_end(&mut decompressed_table) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Failed to decompress file table",
                        ))
                    }
                };
                // Parse entries
                let (_output, entries) = match parse_grf_file_entries_200(
                    decompressed_table.as_slice(),
                    grf_header.file_count,
                ) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Failed to parse file table",
                        ))
                    }
                };
                Ok(GrfArchive {
                    obj: Box::new(file),
                    container: GrfContainer {
                        header: grf_header,
                        table_info: GrfTableInfo::Compressed(grf_table_info),
                        entries,
                    },
                })
            }
            1 => {
                // Only versions 1.1, 1.2 and 1.3 are supported
                if grf_header.version_minor < 1 || grf_header.version_minor > 3 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Unsupported archive version",
                    ));
                }
                let table_size = parser_output.len();
                if table_size == 0 {
                    return Ok(GrfArchive {
                        obj: Box::new(file),
                        container: GrfContainer {
                            header: grf_header,
                            table_info: GrfTableInfo::Uncompressed(GrfTableInfo1 { table_size }),
                            entries: HashMap::new(),
                        },
                    });
                }
                // Parse entries
                let (_parser_output, entries) = match parse_grf_file_entries_101(
                    &parser_output[grf_header.file_table_offset as usize..],
                    grf_header.file_count,
                ) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Failed to parse file table",
                        ))
                    }
                };
                Ok(GrfArchive {
                    obj: Box::new(file),
                    container: GrfContainer {
                        header: grf_header,
                        table_info: GrfTableInfo::Uncompressed(GrfTableInfo1 { table_size }),
                        entries,
                    },
                })
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported archive version",
            )),
        }
    }

    pub fn file_count(&self) -> usize {
        self.container.header.file_count
    }

    pub fn version_major(&self) -> u32 {
        self.container.header.version_major
    }

    pub fn version_minor(&self) -> u32 {
        self.container.header.version_minor
    }

    pub fn read_file_content<S: AsRef<str> + Hash>(&mut self, file_path: S) -> io::Result<Vec<u8>> {
        let file_entry = match self.get_file_entry(file_path) {
            Some(v) => v.clone(),
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "File not found")),
        };
        self.obj.seek(SeekFrom::Start(file_entry.offset))?;
        let mut content: Vec<u8> = vec![0; file_entry.size_compressed_aligned];
        self.obj.read_exact(content.as_mut_slice())?;
        match file_entry.encryption {
            GrfFileEncryption::Unencrypted => {}
            GrfFileEncryption::Encrypted(cycle) => {
                decrypt_file_content(&mut content, cycle);
            }
        }
        // Decompress the content with zlib
        let mut decoder = ZlibDecoder::new(content.as_slice());
        let mut decompressed_content = Vec::new();
        let decompressed_size = decoder.read_to_end(&mut decompressed_content)?;
        if decompressed_size != file_entry.size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Decompressed content is not as expected",
            ));
        }
        Ok(decompressed_content)
    }

    pub fn contains_file<S: AsRef<str> + Hash>(&self, file_path: S) -> bool {
        self.container.entries.contains_key(file_path.as_ref())
    }

    pub fn get_file_entry<S: AsRef<str> + Hash>(&self, file_path: S) -> Option<&GrfFileEntry> {
        self.container.entries.get(file_path.as_ref())
    }

    pub fn get_entries(&self) -> impl Iterator<Item = &'_ GrfFileEntry> {
        self.container.entries.values()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct GrfContainer {
    pub header: GrfHeader,
    pub table_info: GrfTableInfo,
    pub entries: HashMap<String, GrfFileEntry>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct GrfHeader {
    pub key: [u8; 14],
    pub file_table_offset: u64,
    pub seed: i32,
    pub file_count: usize,
    pub version_major: u32,
    pub version_minor: u32,
}

#[derive(Debug, PartialEq, Eq)]
enum GrfTableInfo {
    Uncompressed(GrfTableInfo1),
    Compressed(GrfTableInfo2),
}

#[derive(Debug, PartialEq, Eq)]
struct GrfTableInfo1 {
    pub table_size: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct GrfTableInfo2 {
    pub table_size_compressed: usize,
    pub table_size: usize,
}

#[derive(Debug, Clone, Eq)]
pub struct GrfFileEntry {
    pub relative_path: String,
    pub size_compressed: usize,
    pub size_compressed_aligned: usize,
    pub size: usize,
    pub entry_type: u8,
    pub offset: u64,
    pub encryption: GrfFileEncryption,
}

impl Hash for GrfFileEntry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.relative_path.hash(state);
    }
}

impl PartialEq for GrfFileEntry {
    fn eq(&self, other: &GrfFileEntry) -> bool {
        self.relative_path == other.relative_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrfFileEncryption {
    Unencrypted,
    Encrypted(usize), // Contains the cycle as usize
}

named!(parse_grf_header<&[u8], GrfHeader>,
    do_parse!(
        tag!(GRF_HEADER_MAGIC)
            >> key: take!(14)
            >> file_table_offset: le_u32
            >> seed: le_i32
            >> v_files_count: le_i32
            >> version: le_u32
            >> (GrfHeader {
                key: key.try_into().unwrap(),
                file_table_offset: file_table_offset as u64,
                seed,
                file_count: (v_files_count - seed - 7) as usize,
                version_major: (version >> 8) & 0xFF,
                version_minor: version & 0xFF
            }
    )
));

named!(parse_grf_table_info_200<&[u8], GrfTableInfo2>,
    do_parse!(
        table_size_compressed: le_u32
            >> table_size: le_u32
            >> (GrfTableInfo2 {
                table_size_compressed: table_size_compressed as usize,
                table_size: table_size as usize,
            }
    )
));

fn string_from_win_1252(v: &[u8]) -> Result<String, Cow<'static, str>> {
    let decoder = match encoding_from_whatwg_label("windows-1252") {
        Some(v) => v,
        None => return Err(Cow::Borrowed("Decoder unavailable")),
    };
    decoder.decode(v, DecoderTrap::Strict)
}

macro_rules! take_obfuscated_name_101 (
    ( $i:expr, $size:expr ) => (
        {
            let input: &[u8] = $i;
            let (parser_output, file_name_bytes) = map_res!(input, take!($size), decrypt_file_name)?;
            match string_from_win_1252(file_name_bytes.as_slice()) {
                Ok(v) => Ok((parser_output , v)),
                Err(_) => Err(nom::Err::Failure((parser_output, ErrorKind::AlphaNumeric))),
            }
        }
     );
);

fn determine_file_encryption_101(file_name: &str, size_compressed: usize) -> GrfFileEncryption {
    const SPECIAL_EXTENSIONS: [&str; 4] = [".gnd", ".gat", ".act", ".str"];
    let file_name_len = file_name.len();
    if file_name_len < 4 {
        return GrfFileEncryption::Encrypted(0);
    }
    let file_extension = &file_name[file_name_len - 4..];
    match SPECIAL_EXTENSIONS.iter().position(|&r| r == file_extension) {
        Some(_) => GrfFileEncryption::Encrypted(0),
        None => GrfFileEncryption::Encrypted(digit_count(size_compressed)),
    }
}

/// Counts digits naively
fn digit_count(n: usize) -> usize {
    let mut result = 1;
    let mut acc = 10;
    loop {
        if n < acc {
            break;
        }
        acc *= 10;
        result += 1;
    }

    result
}

// Parses file table entries for GRF 1.1, 1.2 and 1.3
named!(parse_grf_file_entry_101<&[u8], GrfFileEntry>,
    do_parse!(
        path_size_padded: le_u32
            >> take!(2) // Null chars
            >> relative_path: take_obfuscated_name_101!(path_size_padded - 6)
            >> take!(4) // Null chars
            >> size_tot_enc: le_u32
            >> size_compressed_aligned_enc: le_u32
            >> size: le_u32
            >> entry_type: le_u8
            >> offset: le_u32
            >> (GrfFileEntry {
                size_compressed: (size_tot_enc - size - 0x02CB) as usize,
                size_compressed_aligned: (size_compressed_aligned_enc - 0x92CB) as usize,
                size: size as usize,
                entry_type,
                offset: GRF_HEADER_SIZE as u64 + offset as u64,
                encryption: determine_file_encryption_101(&relative_path, (size_tot_enc - size - 0x02CB) as usize),
                relative_path,
            }
        )
    )
);

// Parses file table entries for GRF 2.0
named!(parse_grf_file_entry_200<&[u8], GrfFileEntry>,
    do_parse!(
        relative_path: map_res!(take_while!(|ch: u8| ch != 0), string_from_win_1252)
            >> take!(1) // Null char terminator
            >> size_compressed: le_u32
            >> size_compressed_aligned: le_u32
            >> size: le_u32
            >> entry_type: le_u8
            >> offset: le_u32
            >> (GrfFileEntry {
                relative_path,
                size_compressed: size_compressed as usize,
                size_compressed_aligned: size_compressed_aligned as usize,
                size: size as usize,
                entry_type,
                offset: GRF_HEADER_SIZE as u64 + offset as u64,
                encryption: GrfFileEncryption::Unencrypted,
            }
        )
    )
);

named_args!(parse_grf_file_entries_101(files_count: usize)<&[u8], HashMap<String, GrfFileEntry>>,
fold_many_m_n!(1, files_count - 1, parse_grf_file_entry_101, HashMap::new(), |mut acc: HashMap<_, _>, item| {
        acc.insert(item.relative_path.clone(), item);
        acc
    })
);

named_args!(parse_grf_file_entries_200(files_count: usize)<&[u8], HashMap<String, GrfFileEntry>>,
fold_many_m_n!(1, files_count, parse_grf_file_entry_200, HashMap::new(), |mut acc: HashMap<_, _>, item| {
        acc.insert(item.relative_path.clone(), item);
        acc
    })
);

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;
    use std::path::PathBuf;
    use twox_hash::XxHash64;

    #[test]
    fn test_open_grf_container() {
        let grf_dir_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/tests/grf");
        let expected_content: HashMap<&str, (usize, u64)> = [
            (
                "data\\06guild_r.gat",
                (800014, u64::from_be_bytes(hex!("b740a01075ce37f2"))),
            ),
            (
                "data\\06guild_r.gnd",
                (454622, u64::from_be_bytes(hex!("213f0c61fff67856"))),
            ),
            (
                "data\\06guild_r.rsw",
                (69798, u64::from_be_bytes(hex!("519d99273b1b4d38"))),
            ),
            (
                "data\\sprite\\\u{B8}\u{F3}\u{BD}\u{BA}\u{C5}\u{CD}\\high_orc.act",
                (491076, u64::from_be_bytes(hex!("5f26d5f20679a2af"))),
            ),
            (
                "data\\sprite\\\u{B8}\u{F3}\u{BD}\u{BA}\u{C5}\u{CD}\\high_orc.spr",
                (250592, u64::from_be_bytes(hex!("b8356a4d4517df6e"))),
            ),
            (
                "data\\texture\\chdesk-side1.bmp",
                (33844, u64::from_be_bytes(hex!("b4bc113b3ca8a655"))),
            ),
            (
                "data\\texture\\chdesk-side2.bmp",
                (33844, u64::from_be_bytes(hex!("c81a827857725179"))),
            ),
            (
                "data\\texture\\chdesk-side3.bmp",
                (17460, u64::from_be_bytes(hex!("2c796a702a93682f"))),
            ),
        ]
        .iter()
        .cloned()
        .collect();
        let check_small_grf_entries = |grf: &mut GrfArchive| {
            let file_entries: Vec<GrfFileEntry> = grf.get_entries().map(|e| e.clone()).collect();
            for file_entry in file_entries {
                let file_path: &str = &file_entry.relative_path[..];
                assert!(expected_content.contains_key(file_path));
                let (expected_size, expected_hash) = expected_content[file_path];
                assert_eq!(file_entry.size, expected_size);
                // Size check
                let file_content = grf.read_file_content(file_path).unwrap();
                assert_eq!(file_content.len(), expected_size);
                // Hash check
                let mut hasher = XxHash64::default();
                hasher.write(file_content.as_slice());
                assert_eq!(hasher.finish(), expected_hash);
            }
        };
        {
            let grf_path = grf_dir_path.join("200-empty.grf");
            let grf = GrfArchive::open(&grf_path).unwrap();
            assert_eq!(grf.file_count(), 0);
            assert_eq!(grf.version_major(), 2);
            assert_eq!(grf.version_minor(), 0);
        }

        {
            let grf_path = grf_dir_path.join("200-small.grf");
            let mut grf = GrfArchive::open(&grf_path).unwrap();
            assert_eq!(grf.file_count(), 8);
            assert_eq!(grf.version_major(), 2);
            assert_eq!(grf.version_minor(), 0);
            check_small_grf_entries(&mut grf);
        }

        {
            let grf_path = grf_dir_path.join("103-empty.grf");
            let grf = GrfArchive::open(&grf_path).unwrap();
            assert_eq!(grf.file_count(), 0);
            assert_eq!(grf.version_major(), 1);
            assert_eq!(grf.version_minor(), 3);
        }

        {
            let grf_path = grf_dir_path.join("103-small.grf");
            let mut grf = GrfArchive::open(&grf_path).unwrap();
            assert_eq!(grf.file_count(), 8);
            assert_eq!(grf.version_major(), 1);
            assert_eq!(grf.version_minor(), 3);
            check_small_grf_entries(&mut grf);
        }

        {
            let grf_path = grf_dir_path.join("102-empty.grf");
            let grf = GrfArchive::open(&grf_path).unwrap();
            assert_eq!(grf.file_count(), 0);
            assert_eq!(grf.version_major(), 1);
            assert_eq!(grf.version_minor(), 2);
        }

        {
            let grf_path = grf_dir_path.join("102-small.grf");
            let mut grf = GrfArchive::open(&grf_path).unwrap();
            assert_eq!(grf.file_count(), 8);
            assert_eq!(grf.version_major(), 1);
            assert_eq!(grf.version_minor(), 2);
            check_small_grf_entries(&mut grf);
        }
    }

    #[test]
    fn test_digit_count() {
        assert_eq!(1, digit_count(0));
        assert_eq!(1, digit_count(8));
        assert_eq!(2, digit_count(13));
        assert_eq!(2, digit_count(99));
        assert_eq!(3, digit_count(100));
        assert_eq!(8, digit_count(87654321));
    }
}
