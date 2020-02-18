extern crate encoding;
extern crate flate2;
extern crate nom;

use std::borrow::Cow;
use std::io::Read;

use encoding::label::encoding_from_whatwg_label;
use encoding::DecoderTrap;
use flate2::read::ZlibDecoder;
use nom::error::ErrorKind;
use nom::number::streaming::{le_i16, le_i32, le_u32, le_u8};
use nom::IResult;
use nom::*;

const HEADER_MAGIC: &str = "ASSF (C) 2007 Aeomin DEV";

#[derive(Debug, PartialEq, Eq)]
pub struct ThorPatch<'a> {
    pub header: ThorHeader<'a>,
    pub table: ThorTable,
    pub entries: Vec<ThorEntry>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ThorHeader<'a> {
    pub use_grf_merging: u8, // 0 -> client directory, 1 -> GRF
    pub nb_of_files: i32,
    pub mode: ThorMode,
    pub target_grf_name_size: u8,
    pub target_grf_name: &'a str, // If empty (size == 0) -> default GRF
}

#[derive(Debug, PartialEq, Eq)]
pub enum ThorMode {
    SingleFile,
    MultipleFiles,
    Invalid,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ThorTable {
    SingleFile(SingleFileTableDesc),
    MultipleFiles(MultipleFilesTableDesc),
}

#[derive(Debug, PartialEq, Eq)]
pub struct SingleFileTableDesc {
    pub file_table_offset: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MultipleFilesTableDesc {
    pub file_table_compressed_length: usize,
    pub file_table_offset: usize,
    pub data_offset: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ThorEntry {
    pub size_compressed: usize,
    pub size_decompressed: usize,
    pub relative_path: String,
    pub is_removed: bool,
    pub offset: usize,
}

fn i16_to_mode(i: i16) -> ThorMode {
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
            >> nb_of_files: le_i32
            >> mode: le_i16
            >> target_grf_name_size: le_u8
            >> target_grf_name: take_str!(target_grf_name_size)
            >> (ThorHeader {
                use_grf_merging: use_grf_merging,
                nb_of_files: nb_of_files,
                mode: i16_to_mode(mode),
                target_grf_name_size: target_grf_name_size,
                target_grf_name: target_grf_name,
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
        file_table_compressed_length: le_i32 
        >> file_table_offset: le_i32
        >> (MultipleFilesTableDesc {
            file_table_compressed_length: file_table_compressed_length as usize,
            file_table_offset: file_table_offset as usize, // Offset in the 'data' field
            data_offset: 0, // Offset in the 'data' field
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

named!(parse_single_file_entry<&[u8], ThorEntry>,
    do_parse!(
        size_compressed: le_i32
        >> size_decompressed: le_i32
        >> relative_path_size: le_u8
        >> relative_path: take_string_ansi!(relative_path_size)
        >> (ThorEntry {
            size_compressed: size_compressed as usize,
            size_decompressed: size_decompressed as usize,
            relative_path: relative_path,
            is_removed: false,
            offset: 0,
        }
    )
));

/// Uses the given parser only if the flag is as expected
/// This is used to avoid parsing unexisting fields for files marked for deletion
macro_rules! parse_if_not_removed (
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

named!(parse_multiple_files_entry<&[u8], ThorEntry>,
    do_parse!(
        relative_path_size: le_u8
        >> relative_path: take_string_ansi!(relative_path_size)
        >> flags: le_u8
        >> offset: parse_if_not_removed!(le_u32, flags)
        >> size_compressed: parse_if_not_removed!(le_i32, flags)
        >> size_decompressed: parse_if_not_removed!(le_i32, flags)
        >> (ThorEntry {
            size_compressed: size_compressed as usize,
            size_decompressed: size_decompressed as usize,
            relative_path: relative_path,
            is_removed: is_file_removed(flags),
            offset: offset as usize,
        }
    )
));

named!(parse_multiple_files_entries<&[u8], Vec<ThorEntry>>, many1!(complete!(parse_multiple_files_entry)));

pub fn parse_thor_patch(input: &[u8]) -> IResult<&[u8], ThorPatch> {
    let (output, header) = match parse_thor_header(input) {
        Ok(v) => v,
        Err(error) => return Err(error),
    };
    match header.mode {
        ThorMode::Invalid => return Err(Err::Failure((input, ErrorKind::Switch))),
        ThorMode::SingleFile => {
            // Parse table
            let (output, table) = match parse_single_file_table(output) {
                Ok(v) => v,
                Err(error) => return Err(error),
            };
            // Parse the single entry
            let (output, entry) = match parse_single_file_entry(output) {
                Ok(v) => v,
                Err(error) => return Err(error),
            };
            return Ok((
                output,
                ThorPatch {
                    header: header,
                    table: ThorTable::SingleFile(table),
                    entries: vec![entry],
                },
            ));
        }
        ThorMode::MultipleFiles => {
            let (output, mut table) = match parse_multiple_files_table(output) {
                Ok(v) => v,
                Err(error) => return Err(error),
            };
            let consumed_bytes = output.as_ptr() as usize - input.as_ptr() as usize;
            if table.file_table_offset < consumed_bytes {
                return Err(Err::Failure((input, ErrorKind::Switch)));
            }
            // Compute actual table offset inside of 'output'
            table.file_table_offset -= consumed_bytes;
            // Decompress the table with zlib
            let mut decoder = ZlibDecoder::new(&output[table.file_table_offset..]);
            let mut decompressed_table = Vec::new();
            let _decompressed_size = match decoder.read_to_end(&mut decompressed_table) {
                Ok(v) => v,
                Err(_) => return Err(Err::Failure((input, ErrorKind::Switch))),
            };
            // Parse multiple entries
            let (_output, entries) =
                match parse_multiple_files_entries(decompressed_table.as_mut_slice()) {
                    Ok(v) => v,
                    Err(_) => return Err(Err::Failure((input, ErrorKind::Many1))),
                };
            return Ok((
                &input[0..0],
                ThorPatch {
                    header: header,
                    table: ThorTable::MultipleFiles(table),
                    entries: entries,
                },
            ));
        }
    }
}
