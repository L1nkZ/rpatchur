pub mod builder;
pub mod reader;

pub use builder::GrfArchiveBuilder;
pub use reader::{GrfArchive, GrfFileEntry};

mod crypto;
mod dyn_alloc;

use reader::{GRF_HEADER_MAGIC, GRF_HEADER_SIZE};
