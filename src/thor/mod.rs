use std::borrow::Cow;
use std::boxed::Box;
use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crc::crc32;
use encoding::label::encoding_from_whatwg_label;
use encoding::DecoderTrap;
use flate2::read::ZlibDecoder;
use nom::error::ErrorKind;
use nom::number::complete::{le_i16, le_i32, le_u32, le_u8};
use nom::IResult;
use nom::*;

const HEADER_MAGIC: &str = "ASSF (C) 2007 Aeomin DEV";
const INTEGRITY_FILE_NAME: &str = "data.integrity";

type ThorPatchList = Vec<ThorPatchInfo>;

/// Parses Thor's plist.txt file
pub fn patch_list_from_string(content: &str) -> ThorPatchList {
    let vec_lines: Vec<&str> = content.lines().collect();
    let mut sorted_patch_list: ThorPatchList = vec_lines
        .into_iter()
        .filter_map(|elem| ThorPatchInfo::from_string(&elem))
        .collect();
    // Sort patch list by index
    sorted_patch_list.sort_by(|a, b| a.index.cmp(&b.index));
    sorted_patch_list
}

#[derive(Debug)]
pub struct ThorPatchInfo {
    pub index: usize,
    pub file_name: String,
}

impl ThorPatchInfo {
    /// Parses a line to extract patch index and patch file name.
    /// Returns a PatchInfo struct in case of success.
    /// Returns None in case of failure
    fn from_string(line: &str) -> Option<ThorPatchInfo> {
        let words: Vec<_> = line.trim().split_whitespace().collect();
        let index_str = words.get(0)?;
        let index = match str::parse(index_str) {
            Ok(v) => v,
            Err(_) => {
                return None;
            }
        };
        let file_name = words.get(1)?;
        Some(ThorPatchInfo {
            index,
            file_name: (*file_name).to_string(),
        })
    }
}

fn parse_data_integrity_info(data: &str) -> HashMap<&str, u32> {
    let vec_lines: Vec<_> = data.lines().collect();
    vec_lines
        .into_iter()
        .filter_map(|line| {
            let words: Vec<&str> = line.trim().split('=').collect();
            let file_name = words.get(0)?;
            let hash_str = words.get(1)?;
            let hash = match u32::from_str_radix(hash_str.trim_start_matches("0x"), 16) {
                Ok(v) => v,
                Err(_) => return None,
            };
            Some((*file_name, hash))
        })
        .collect()
}

#[derive(Debug)]
pub struct ThorArchive<R: ?Sized> {
    obj: Box<R>,
    container: ThorContainer,
}

impl ThorArchive<File> {
    pub fn open(thor_archive_path: &Path) -> io::Result<ThorArchive<File>> {
        let file = File::open(thor_archive_path)?;
        ThorArchive::new(file)
    }
}

impl<R: Read + Seek> ThorArchive<R> {
    /// Create a new archive with the underlying object as the reader.
    pub fn new(mut obj: R) -> io::Result<ThorArchive<R>> {
        let mut buf = Vec::new();
        // TODO(LinkZ): Avoid using read_to_end, reading the whole file is unnecessary
        let _bytes_read = obj.read_to_end(&mut buf)?;
        let (_, thor_patch) = match parse_thor_patch(buf.as_slice()) {
            IResult::Ok(v) => v,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Failed to parse archive.",
                ))
            }
        };
        Ok(ThorArchive {
            obj: Box::new(obj),
            container: thor_patch,
        })
    }

    pub fn use_grf_merging(&self) -> bool {
        self.container.header.use_grf_merging
    }

    pub fn file_count(&self) -> usize {
        self.container.header.file_count
    }

    pub fn target_grf_name(&self) -> String {
        self.container.header.target_grf_name.clone()
    }

    pub fn read_file_content<S: AsRef<str> + Hash>(&mut self, file_path: S) -> io::Result<Vec<u8>> {
        let file_entry = match self.get_file_entry(file_path) {
            Some(v) => v.clone(),
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "File not found")),
        };
        self.obj.seek(SeekFrom::Start(file_entry.offset))?;
        let mut content: Vec<u8> = vec![0; file_entry.size_compressed];
        self.obj.read_exact(content.as_mut_slice())?;
        // Decompress the table with zlib
        let mut decoder = ZlibDecoder::new(&content[..]);
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

    pub fn extract_file<S: AsRef<str> + Hash>(
        &mut self,
        file_path: S,
        destination_path: &Path,
    ) -> io::Result<()> {
        let content = self.read_file_content(file_path)?;
        let mut file = File::create(destination_path)?;
        file.write_all(content.as_slice())
    }

    pub fn get_file_entry<S: AsRef<str> + Hash>(&self, file_path: S) -> Option<&ThorFileEntry> {
        self.container.entries.get(file_path.as_ref())
    }

    pub fn get_entries(&self) -> impl Iterator<Item = &'_ ThorFileEntry> {
        self.container.entries.values()
    }

    /// Checks if the container has been unintentionnaly corrupted
    pub fn is_valid(&mut self) -> bool {
        let integrity_data = match self.read_file_content(INTEGRITY_FILE_NAME) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let integrity_data_as_str = match string_from_win_1252(integrity_data.as_slice()) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let integrity_info = parse_data_integrity_info(integrity_data_as_str.as_str());
        for (file_path, hash) in integrity_info {
            let file_content = match self.read_file_content(file_path) {
                Ok(v) => v,
                Err(_) => return false,
            };
            if crc32::checksum_ieee(file_content.as_slice()) != hash {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ThorContainer {
    pub header: ThorHeader,
    table: ThorTable,
    pub entries: HashMap<String, ThorFileEntry>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ThorHeader {
    pub use_grf_merging: bool, // false -> client directory, true -> GRF
    pub file_count: usize,
    mode: ThorMode,
    pub target_grf_name: String, // If empty (size == 0) -> default GRF
}

#[derive(Debug, PartialEq, Eq)]
enum ThorMode {
    SingleFile,
    MultipleFiles,
    Invalid,
}

#[derive(Debug, PartialEq, Eq)]
enum ThorTable {
    SingleFile(SingleFileTableDesc),
    MultipleFiles(MultipleFilesTableDesc),
}

#[derive(Debug, PartialEq, Eq)]
struct SingleFileTableDesc {
    file_table_offset: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct MultipleFilesTableDesc {
    file_table_compressed_size: usize,
    file_table_offset: u64,
}

#[derive(Debug, Clone, Eq)]
pub struct ThorFileEntry {
    pub size_compressed: usize,
    pub size: usize,
    pub relative_path: String,
    pub is_removed: bool,
    pub offset: u64,
}

impl ThorFileEntry {
    pub fn is_internal(&self) -> bool {
        self.relative_path == INTEGRITY_FILE_NAME
    }
}

impl Hash for ThorFileEntry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.relative_path.hash(state);
    }
}

impl PartialEq for ThorFileEntry {
    fn eq(&self, other: &ThorFileEntry) -> bool {
        self.relative_path == other.relative_path
    }
}

fn i16_to_thor_mode(i: i16) -> ThorMode {
    match i {
        33 => ThorMode::SingleFile,
        48 => ThorMode::MultipleFiles,
        _ => ThorMode::Invalid,
    }
}

/// Checks entries' flags
/// If LSB is 1, the entry indicates a file deletion
fn is_file_removed(flags: u8) -> bool {
    (flags & 0b1) == 1
}

named!(parse_thor_header<&[u8], ThorHeader>,
    do_parse!(
        tag!(HEADER_MAGIC)
            >> use_grf_merging: le_u8
            >> file_count: le_u32
            >> mode: le_i16
            >> target_grf_name_size: le_u8
            >> target_grf_name: take_str!(target_grf_name_size)
            >> (ThorHeader {
                use_grf_merging: use_grf_merging == 1,
                file_count: (file_count - 1) as usize,
                mode: i16_to_thor_mode(mode),
                target_grf_name: target_grf_name.to_string(),
            }
    )
));

named!(parse_single_file_table<&[u8], SingleFileTableDesc>,
    do_parse!(
        take!(1)
        >> (SingleFileTableDesc {
            file_table_offset: 0, // Offset in the 'data' field
        }
    )
));

named!(parse_multiple_files_table<&[u8], MultipleFilesTableDesc>,
    do_parse!(
        file_table_compressed_size: le_i32
        >> file_table_offset: le_i32
        >> (MultipleFilesTableDesc {
            file_table_compressed_size: file_table_compressed_size as usize,
            file_table_offset: file_table_offset as u64, // Offset in the 'data' field
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

macro_rules! take_string_ansi (
    ( $i:expr, $size:expr ) => (
        {
            let input: &[u8] = $i;
            map_res!(input, take!($size), string_from_win_1252)
        }
     );
);

named!(parse_single_file_entry<&[u8], ThorFileEntry>,
    do_parse!(
        size_compressed: le_i32
        >> size: le_i32
        >> relative_path_size: le_u8
        >> relative_path: take_string_ansi!(relative_path_size)
        >> (ThorFileEntry {
            size_compressed: size_compressed as usize,
            size: size as usize,
            relative_path,
            is_removed: false,
            offset: 0,
        }
    )
));

/// Uses the given parser only if the flag is as expected
/// This is used to avoid parsing unexisting fields for files marked for deletion
macro_rules! take_if_not_removed (
    ( $i:expr, $parser:expr, $flags:expr ) => (
        {
            let input: &[u8] = $i;
            if is_file_removed($flags) {
                value!(input, 0)
            } else {
                $parser(input)
            }
        }
        );
);

named!(parse_multiple_files_entry<&[u8], ThorFileEntry>,
    do_parse!(
        relative_path_size: le_u8
        >> relative_path: take_string_ansi!(relative_path_size)
        >> flags: le_u8
        >> offset: take_if_not_removed!(le_u32, flags)
        >> size_compressed: take_if_not_removed!(le_i32, flags)
        >> size: take_if_not_removed!(le_i32, flags)
        >> (ThorFileEntry {
            size_compressed: size_compressed as usize,
            size: size as usize,
            relative_path,
            is_removed: is_file_removed(flags),
            offset: offset as u64,
        }
    )
));

named!(parse_multiple_files_entries<&[u8], HashMap<String, ThorFileEntry>>,
    fold_many1!(parse_multiple_files_entry, HashMap::new(), |mut acc: HashMap<_, _>, item| {
        acc.insert(item.relative_path.clone(), item);
        acc
    })
);

pub fn parse_thor_patch(input: &[u8]) -> IResult<&[u8], ThorContainer> {
    let (output, header) = parse_thor_header(input)?;
    match header.mode {
        ThorMode::Invalid => Err(Err::Failure((input, ErrorKind::Switch))),
        ThorMode::SingleFile => {
            // Parse table
            let (output, table) = parse_single_file_table(output)?;
            // Parse the single entry
            let (output, entry) = parse_single_file_entry(output)?;
            Ok((
                output,
                ThorContainer {
                    header,
                    table: ThorTable::SingleFile(table),
                    entries: [(entry.relative_path.clone(), entry)]
                        .iter()
                        .cloned()
                        .collect(),
                },
            ))
        }
        ThorMode::MultipleFiles => {
            let (output, mut table) = parse_multiple_files_table(output)?;
            let consumed_bytes = output.as_ptr() as u64 - input.as_ptr() as u64;
            if table.file_table_offset < consumed_bytes {
                return Err(Err::Failure((input, ErrorKind::Switch)));
            }
            // Compute actual table offset inside of 'output'
            table.file_table_offset -= consumed_bytes;
            // Decompress the table with zlib
            let mut decoder = ZlibDecoder::new(&output[table.file_table_offset as usize..]);
            let mut decompressed_table = Vec::new();
            let _decompressed_size = match decoder.read_to_end(&mut decompressed_table) {
                Ok(v) => v,
                Err(_) => return Err(Err::Failure((input, ErrorKind::Switch))),
            };
            // Parse multiple entries
            let (_output, entries) =
                match parse_multiple_files_entries(decompressed_table.as_slice()) {
                    Ok(v) => v,
                    Err(_) => return Err(Err::Failure((input, ErrorKind::Many1))),
                };
            Ok((
                &[],
                ThorContainer {
                    header,
                    table: ThorTable::MultipleFiles(table),
                    entries,
                },
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_patch_list_from_string() {
        let plist_content = "//869 iteminfo_20170423.thor
870 iteminfo_20170423_.thor
871 sprites_20170427.thor
872 sprites_20170429.thor
623 2016-01_01.thor
//873 rodex_20170501.thor
//874 strings_20170501.thor
875 rodex_20170501_.thor";
        let expected_content: HashMap<usize, &str> = [
            (623, "2016-01_01.thor"),
            (870, "iteminfo_20170423_.thor"),
            (871, "sprites_20170427.thor"),
            (872, "sprites_20170429.thor"),
            (875, "rodex_20170501_.thor"),
        ]
        .iter()
        .cloned()
        .collect();
        //Empty patch list
        let empty_thor_patch_list = patch_list_from_string("");
        assert_eq!(empty_thor_patch_list.len(), 0);
        // TODO(LinkZ): Ensure patch list is ordered by patch index
        // Regular patch list
        let thor_patch_list = patch_list_from_string(plist_content);
        assert_eq!(thor_patch_list.len(), expected_content.len());
        for patch_info in thor_patch_list {
            assert!(expected_content.contains_key(&patch_info.index));
            assert_eq!(patch_info.file_name, expected_content[&patch_info.index]);
        }
    }

    #[test]
    fn test_open_thor_container() {
        let thor_dir_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/tests/thor");
        {
            let expected_content: HashMap<&str, usize> = [
                ("data.integrity", 63),
                (
                    "data\\texture\\\u{c0}\u{af}\u{c0}\u{fa}\u{c0}\u{ce}\u{c5}\u{cd}\u{c6}\u{e4}\u{c0}\u{cc}\u{bd}\u{ba}\\inventory\\icon_num.bmp",
                    560,
                ),
            ]
            .iter()
            .cloned()
            .collect();
            let check_tiny_thor_entries = |thor: &mut ThorArchive<File>| {
                let file_entries: Vec<ThorFileEntry> = thor.get_entries().cloned().collect();
                for file_entry in file_entries {
                    let file_path: &str = &file_entry.relative_path[..];
                    assert!(expected_content.contains_key(file_path));
                    let expected_size = expected_content[file_path];
                    assert_eq!(file_entry.size, expected_size);
                }
            };
            let thor_file_path = thor_dir_path.join("tiny.thor");
            let mut thor_archive = ThorArchive::open(&thor_file_path).unwrap();
            assert_eq!(thor_archive.file_count(), expected_content.len());
            assert_eq!(thor_archive.target_grf_name(), "");
            assert!(thor_archive.use_grf_merging());
            check_tiny_thor_entries(&mut thor_archive);
            assert!(thor_archive.is_valid());
        }

        {
            let thor_file_path = thor_dir_path.join("small.thor");
            let mut thor_archive = ThorArchive::open(&thor_file_path).unwrap();
            assert_eq!(thor_archive.file_count(), 16);
            assert_eq!(thor_archive.target_grf_name(), "data.grf");
            assert!(thor_archive.use_grf_merging());
            assert!(thor_archive.is_valid());
        }
    }
}
