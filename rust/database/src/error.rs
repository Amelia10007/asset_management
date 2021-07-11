use std::fmt::{self, Display, Formatter};

#[derive(Debug)]
pub enum LogicError {
    DuplicatedCurrency,
    DuplicatedMarket,
}

impl Display for LogicError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for LogicError {}

#[derive(Debug)]
pub enum Error {
    Db(diesel::result::Error),
    Logic(LogicError),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::Db(e) => e.fmt(f),
            Error::Logic(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Db(e) => Some(e),
            Error::Logic(e) => Some(e),
        }
    }
}

impl From<diesel::result::Error> for Error {
    fn from(e: diesel::result::Error) -> Self {
        Error::Db(e)
    }
}

impl From<LogicError> for Error {
    fn from(e: LogicError) -> Self {
        Error::Logic(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
