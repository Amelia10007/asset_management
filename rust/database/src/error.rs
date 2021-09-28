use thiserror::Error;

#[derive(Debug, Error)]
pub enum LogicError {
    #[error("Cannot add not latest timestamp")]
    NonLatestStamp,
    #[error("DuplicatedCurrency")]
    DuplicatedCurrency,
    #[error("DuplicatedMarket")]
    DuplicatedMarket,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("DB error: {0}")]
    Db(diesel::result::Error),
    #[error("Logic error: {0}")]
    Logic(LogicError),
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
