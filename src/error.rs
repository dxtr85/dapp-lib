use std::error::Error;
use std::fmt::{Display, Formatter, Result as FmtResult};

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
        }
    }
}
