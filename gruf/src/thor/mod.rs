pub mod builder;
pub mod reader;

pub use builder::ThorArchiveBuilder;
pub use reader::{
    patch_list_from_string, ThorArchive, ThorFileEntry, ThorPatchInfo, ThorPatchList,
};

const THOR_HEADER_MAGIC: &[u8; 24] = b"ASSF (C) 2007 Aeomin DEV";
const INTEGRITY_FILE_NAME: &str = "data.integrity";
const MULTIPLE_FILES_TABLE_DESC_SIZE: usize = 2 * std::mem::size_of::<i32>();
#[derive(Debug, PartialEq, Eq)]
enum ThorMode {
    SingleFile,
    MultipleFiles,
    Invalid,
}
