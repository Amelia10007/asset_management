pub use chrono::NaiveDateTime;
use diesel_derive_enum::DbEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, DbEnum)]
pub enum OrderKind {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, DbEnum)]
pub enum OrderState {
    Opened,
    Filled,
    Cancelled,
    Error,
}
