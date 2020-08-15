pub mod builder;
pub mod reader;
pub use builder::GrfArchiveBuilder;
pub use reader::GrfArchive;

mod crypto;
use reader::{GrfFileEntry, GRF_HEADER_MAGIC, GRF_HEADER_SIZE};
