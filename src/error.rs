use std::error::Error;
use std::fmt::{Display, Formatter, Result as FmtResult};

use crate::Data;

#[derive(Debug)]
pub enum AppError {
    Smthg,
    IndexingError,
    HashMismatch,
    ContentEmpty,
    ContentFull,
    LinkNonTransformative,
    DatatypeMismatch,
    DatastoreFull,
    DatastoreInsertCalledOnFilled,
    AppDataNotSynced,
}
impl Error for AppError {}
impl Display for AppError {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            Self::Smthg => write!(f, "Smthg"),
            Self::IndexingError => write!(f, "IndexingError"),
            Self::HashMismatch => write!(f, "HashMismatch"),
            Self::ContentEmpty => write!(f, "ContentEmpty"),
            Self::ContentFull => write!(f, "ContentFull"),
            Self::LinkNonTransformative => write!(f, "LinkNonTransformative"),
            Self::DatatypeMismatch => write!(f, "DatatypeMismatch"),
            Self::DatastoreFull => write!(f, "DatastoreFull"),
            Self::DatastoreInsertCalledOnFilled => write!(f, "DatastoreInsertCalledOnFilled"),
            Self::AppDataNotSynced => write!(f, "AppDataNotSynced"),
        }
    }
}
#[derive(Debug)]
pub enum SubtreeError {
    Empty,
    DatatypeMismatch,
    RightLeafEmpty(Data),
    BothLeavesEmpty(Data),
}
impl Error for SubtreeError {}
impl Display for SubtreeError {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            Self::RightLeafEmpty(_data) => write!(f, "RightLeafEmpty"),
            Self::BothLeavesEmpty(_data) => write!(f, "BothLeavesEmpty"),
            Self::Empty => write!(f, "Empty"),
            Self::DatatypeMismatch => write!(f, "DataTypeMismatch"),
        }
    }
}
