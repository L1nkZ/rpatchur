use std::io;
use std::num;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, GrufError>;

#[derive(Error, Debug)]
pub enum GrufError {
    #[error("io error: {0}")]
    IoError(#[from] io::Error),
    #[error("bincode error: {0}")]
    BincodeError(#[from] bincode::Error),
    #[error("int conversion error: {0}")]
    TryFromIntError(#[from] num::TryFromIntError),
    #[error("failed to parse archive: {0}")]
    ParsingError(String),
    #[error("failed to find file entry")]
    EntryNotFound,
    #[error("failed to read content: {0}")]
    InvalidContent(String),
    #[error("failed to serialize data: {0}")]
    SerializationError(String),
    #[error("dyn_alloc error")]
    DynAllocError,
}

impl GrufError {
    pub fn parsing_error(msg: impl Into<String>) -> Self {
        Self::ParsingError(msg.into())
    }

    pub fn invalid_content(msg: impl Into<String>) -> Self {
        Self::InvalidContent(msg.into())
    }

    pub fn serialization_error(msg: impl Into<String>) -> Self {
        Self::SerializationError(msg.into())
    }
}
