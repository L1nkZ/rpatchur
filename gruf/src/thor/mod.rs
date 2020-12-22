pub mod builder;
pub mod reader;

pub use builder::ThorArchiveBuilder;
pub use reader::{
    patch_list_from_string, ThorArchive, ThorFileEntry, ThorPatchInfo, ThorPatchList,
};

use reader::THOR_HEADER_MAGIC;

const MULTIPLE_FILES_TABLE_DESC_SIZE: usize = 2 * std::mem::size_of::<i32>();
#[derive(Debug, PartialEq, Eq)]
enum ThorMode {
    SingleFile,
    MultipleFiles,
    Invalid,
}
