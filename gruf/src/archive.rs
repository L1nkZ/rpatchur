use std::io::Write;

use crate::{GrufError, Result};
use encoding::label::encoding_from_whatwg_label;
use encoding::EncoderTrap;

pub struct GenericFileEntry {
    pub offset: u64,
    // Note(LinkZ): u32 limited by the GRF and THOR file formats
    pub size: u32,
    pub size_compressed: u32,
}

/// Serializes string into a NULL-terminated list of win1252 chars and write it
/// into writer.
///
/// Used in GRF archives
pub fn serialize_as_win1252_cstr_into<W: Write>(mut writer: W, string: &str) -> Result<()> {
    let mut vec = serialize_to_win1252(string)?;
    vec.push(0); // NUL char terminator
    writer.write_all(vec.as_slice())?;
    Ok(())
}

/// Serializes string into a list of win1252 chars and write it into writer.
///
// Used in THOR archives
pub fn serialize_as_win1252_str_into<W: Write>(mut writer: W, string: &str) -> Result<()> {
    let vec = serialize_to_win1252(string)?;
    writer.write_all(vec.as_slice())?;
    Ok(())
}

pub fn serialize_to_win1252(string: &str) -> Result<Vec<u8>> {
    let decoder = encoding_from_whatwg_label("windows-1252")
        .ok_or_else(|| GrufError::serialization_error("Encoder unavailable"))?;
    decoder
        .encode(string, EncoderTrap::Strict)
        .map_err(|_| GrufError::serialization_error("Encoding failed"))
}
