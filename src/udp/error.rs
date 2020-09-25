use std::fmt::{Formatter, Result};
use std::io;

#[derive(Debug)]
pub enum Error {
    Logic(String),
    IOError(io::Error),
    Other(Box<dyn std::error::Error>),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Error::Logic(msg) => write!(f, "logic error:{}", msg),
            Error::IOError(err) => write!(f, "io error:{}", err),
            Error::Other(err) => write!(f, "{}", err),
        }
    }
}

impl std::error::Error for Error {}
