use std::fmt::{self, Display, Formatter};

#[derive(Debug)]
pub enum LogicError {
    Duplicated,
    NotFound,
}

impl Display for LogicError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for LogicError {}

#[derive(Debug)]
pub enum Error {
    Sql(mysql::Error),
    Logic(LogicError),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sql(e) => e.fmt(f),
            Error::Logic(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Sql(e) => Some(e),
            Error::Logic(e) => Some(e),
        }
    }
}

impl From<mysql::Error> for Error {
    fn from(e: mysql::Error) -> Self {
        Error::Sql(e)
    }
}

impl From<LogicError> for Error {
    fn from(e: LogicError) -> Self {
        Error::Logic(e)
    }
}
