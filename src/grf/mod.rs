extern crate encoding;
extern crate flate2;
extern crate nom;

mod crypto;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;
use std::str;

use crypto::decrypt_file_name;
use encoding::label::encoding_from_whatwg_label;
use encoding::DecoderTrap;
use flate2::read::ZlibDecoder;
use nom::error::ErrorKind;
use nom::number::complete::{le_i32, le_u32, le_u8};
use nom::IResult;
use nom::*;

const HEADER_MAGIC: &str = "Master of Magic\0";

pub fn open_grf_container(grf_path: &Path) -> io::Result<GrfContainer> {
    let mut file = File::open(grf_path)?;
    // TODO(LinkZ): Avoid using read_to_end, reading the whole file is unnecessary
    let mut buf = vec![];
    let _bytes_read = file.read_to_end(&mut buf)?;
    let (parser_output, grf_header) = match parse_grf_header(buf.as_slice()) {
        IResult::Ok(v) => v,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Failed to parse archive.",
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
                        "Failed to parse archive.",
                    ))
                }
            };
            if grf_table_info.table_size_compressed == 0 || grf_table_info.table_size == 0 {
                return Ok(GrfContainer {
                    header: grf_header,
                    table_info: GrfTableInfo::Compressed(grf_table_info),
                    entries: HashMap::new(),
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
                        "Failed to decompress file table.",
                    ))
                }
            };
            // Parse entries
            let (_output, entries) = match parse_grf_file_entries_200(
                decompressed_table.as_slice(),
                grf_header.files_count,
            ) {
                Ok(v) => v,
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Failed to parse file table.",
                    ))
                }
            };
            Ok(GrfContainer {
                header: grf_header,
                table_info: GrfTableInfo::Compressed(grf_table_info),
                entries: entries,
            })
        }
        1 => {
            // Only versions 1.1, 1.2 and 1.3 are supported
            if grf_header.version_minor < 1 || grf_header.version_minor > 3 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Unsupported archive version.",
                ));
            }
            let table_size = parser_output.len();
            if table_size == 0 {
                return Ok(GrfContainer {
                    header: grf_header,
                    table_info: GrfTableInfo::Uncompressed(GrfTableInfo1 {
                        table_size: table_size,
                    }),
                    entries: HashMap::new(),
                });
            }
            // Parse entries
            let (_parser_output, entries) = match parse_grf_file_entries_101(
                &parser_output[grf_header.file_table_offset as usize..],
                grf_header.files_count,
            ) {
                Ok(v) => v,
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Failed to parse file table.",
                    ))
                }
            };
            Ok(GrfContainer {
                header: grf_header,
                table_info: GrfTableInfo::Uncompressed(GrfTableInfo1 {
                    table_size: table_size,
                }),
                entries: entries,
            })
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported archive version.",
            ))
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct GrfContainer {
    pub header: GrfHeader,
    pub table_info: GrfTableInfo,
    pub entries: HashMap<String, GrfFileEntry>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct GrfHeader {
    pub key: String,
    pub file_table_offset: u32,
    pub seed: i32,
    pub files_count: usize,
    pub version_major: u32,
    pub version_minor: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GrfTableInfo {
    Uncompressed(GrfTableInfo1),
    Compressed(GrfTableInfo2),
}

#[derive(Debug, PartialEq, Eq)]
pub struct GrfTableInfo1 {
    pub table_size: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct GrfTableInfo2 {
    pub table_size_compressed: usize,
    pub table_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrfFileEntry {
    pub relative_path: String,
    pub size_compressed: usize,
    pub size_compressed_aligned: usize,
    pub size: usize,
    pub entry_type: u8,
    pub offset: u32,
}

named!(parse_grf_header<&[u8], GrfHeader>,
    do_parse!(
        tag!(HEADER_MAGIC)
            >> key: take_str!(14)
            >> file_table_offset: le_u32
            >> seed: le_i32
            >> v_files_count: le_i32
            >> version: le_u32
            >> (GrfHeader {
                key: key.to_string(),
                file_table_offset: file_table_offset,
                seed: seed,
                files_count: (v_files_count - seed - 7) as usize,
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
                relative_path: relative_path,
                size_compressed: (size_tot_enc - size - 0x02CB) as usize,
                size_compressed_aligned: (size_compressed_aligned_enc - 0x92CB) as usize,
                size: size as usize,
                entry_type: entry_type,
                offset: offset
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
                relative_path: relative_path.to_string(),
                size_compressed: size_compressed as usize,
                size_compressed_aligned: size_compressed_aligned as usize,
                size: size as usize,
                entry_type: entry_type,
                offset: offset
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
    use std::path::PathBuf;

    #[test]
    fn test_open_grf_container() {
        let grf_dir_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/tests/grf");
        let expected_sizes: HashMap<&str, usize> = [
            ("data\\06guild_r.gat", 800014),
            ("data\\06guild_r.gnd", 454622),
            ("data\\06guild_r.rsw", 69798),
            (
                "data\\sprite\\\u{B8}\u{F3}\u{BD}\u{BA}\u{C5}\u{CD}\\high_orc.act",
                491076,
            ),
            (
                "data\\sprite\\\u{B8}\u{F3}\u{BD}\u{BA}\u{C5}\u{CD}\\high_orc.spr",
                250592,
            ),
            ("data\\texture\\chdesk-side1.bmp", 33844),
            ("data\\texture\\chdesk-side2.bmp", 33844),
            ("data\\texture\\chdesk-side3.bmp", 17460),
        ]
        .iter()
        .cloned()
        .collect();
        let check_small_grf_entries = |entries: HashMap<String, GrfFileEntry>| {
            for file_entry in entries.values() {
                let file_path: &str = &file_entry.relative_path[..];
                assert!(expected_sizes.contains_key(file_path));
                assert_eq!(file_entry.size, expected_sizes[file_path]);
            }
        };
        {
            let grf_path = grf_dir_path.join("200-empty.grf");
            let grf = open_grf_container(&grf_path).unwrap();
            assert_eq!(grf.header.files_count, 0);
            assert_eq!(grf.header.version_major, 2);
            assert_eq!(grf.header.version_minor, 0);
        }

        {
            let grf_path = grf_dir_path.join("200-small.grf");
            let grf = open_grf_container(&grf_path).unwrap();
            assert_eq!(grf.header.files_count, 8);
            assert_eq!(grf.header.version_major, 2);
            assert_eq!(grf.header.version_minor, 0);
            check_small_grf_entries(grf.entries);
        }

        {
            let grf_path = grf_dir_path.join("103-empty.grf");
            let grf = open_grf_container(&grf_path).unwrap();
            assert_eq!(grf.header.files_count, 0);
            assert_eq!(grf.header.version_major, 1);
            assert_eq!(grf.header.version_minor, 3);
        }

        {
            let grf_path = grf_dir_path.join("103-small.grf");
            let grf = open_grf_container(&grf_path).unwrap();
            assert_eq!(grf.header.files_count, 8);
            assert_eq!(grf.header.version_major, 1);
            assert_eq!(grf.header.version_minor, 3);
            check_small_grf_entries(grf.entries);
        }

        {
            let grf_path = grf_dir_path.join("102-empty.grf");
            let grf = open_grf_container(&grf_path).unwrap();
            assert_eq!(grf.header.files_count, 0);
            assert_eq!(grf.header.version_major, 1);
            assert_eq!(grf.header.version_minor, 2);
        }

        {
            let grf_path = grf_dir_path.join("102-small.grf");
            let grf = open_grf_container(&grf_path).unwrap();
            assert_eq!(grf.header.files_count, 8);
            assert_eq!(grf.header.version_major, 1);
            assert_eq!(grf.header.version_minor, 2);
            check_small_grf_entries(grf.entries);
        }
    }
}
