pub use chrono::NaiveDateTime;
use diesel_derive_enum::DbEnum;
use std::fmt::{self, Display, Formatter};

macro_rules! id_type {
    ($wrapper:tt, $inner:tt) => {
        #[derive(DieselNewType, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $wrapper($inner);

        impl $wrapper {
            pub const fn new(inner: $inner) -> Self {
                Self(inner)
            }

            pub const fn inner(self) -> $inner {
                self.0
            }
        }

        impl Display for $wrapper {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(CurrencyId, i32);
id_type!(StampId, i32);
id_type!(BalanceId, i32);
id_type!(MarketId, i32);
id_type!(PriceId, i32);
id_type!(OrderbookId, i32);
id_type!(MyorderId, i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, DbEnum)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, DbEnum)]
pub enum OrderType {
    Limit,
    Market,
    StopLimit,
    StopMarket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, DbEnum)]
pub enum OrderState {
    Opened,
    Filled,
    Cancelled,
    Error,
}
