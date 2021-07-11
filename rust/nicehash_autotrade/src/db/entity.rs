use apply::Apply;
use common::id_type;
use mysql::prelude::{ConvIr, FromValue};
use mysql::{FromValueError, Value};
use mysql_common::value::convert::ParseIr;
use std::borrow::Cow;
use std::ops::{Add, Sub};
use std::time::{Duration, SystemTime};

pub trait IntoId {
    type Id;

    fn into_id(self) -> Self::Id;
}

macro_rules! id_type_ext {
    ($t: tt) => {
        id_type!($t, i32);

        impl IntoId for $t {
            type Id = $t;

            fn into_id(self) -> Self::Id {
                self
            }
        }

        impl Into<Value> for $t {
            fn into(self) -> Value {
                self.into_id().into()
            }
        }
    };
}

id_type_ext!(CurrencyId);
id_type_ext!(BalanceId);
id_type_ext!(MarketId);
id_type_ext!(PriceId);
id_type_ext!(OrderbookId);
id_type_ext!(OrderId);

pub type Amount = f64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderKind {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderStateTag {
    Created,
    Reserved,
    ReservedError,
    Inserted,
    InsertedError,
    Released,
    ReleasedError,
    Partial,
    Entered,
    Full,
    ProcessedError,
    CancelRequest,
    Cancelled,
    CancelledError,
    Rejected,
    Unknown,
}

use OrderStateTag::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrderState {
    pub tag: OrderStateTag,
    pub s: String,
}

impl OrderState {
    pub fn from_str<'a, S: Into<Cow<'a, str>>>(s: S) -> Self {
        let s = s.into();

        let tag = match s.as_ref() {
            "CREATED" => Created,
            "RESERVED" => Reserved,
            "RESERVED_ERROR" => ReservedError,
            "INSERTED" => Inserted,
            "INSERTED_ERROR" => InsertedError,
            "RELEASED" => Released,
            "RELEASED_ERROR" => ReleasedError,
            "PARTIAL" => Partial,
            "ENTERED" => Entered,
            "FULL" => Full,
            "PROCESSED_ERROR" => ProcessedError,
            "CANCEL_REQUEST" => CancelRequest,
            "CANCELLED" => Cancelled,
            "CANCELLED_ERROR" => CancelledError,
            "REJECTED" => Rejected,
            _ => Unknown,
        };

        Self {
            tag,
            s: s.into_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Currency {
    pub id: CurrencyId,
    pub symbol: String,
    pub name: String,
}

impl Currency {
    pub fn new<'a, 'b, S1, S2>(id: CurrencyId, symbol: S1, name: S2) -> Self
    where
        S1: Into<Cow<'a, str>>,
        S2: Into<Cow<'b, str>>,
    {
        Self {
            id,
            symbol: symbol.into().into_owned(),
            name: name.into().into_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    system_time: SystemTime,
}

impl Timestamp {
    pub fn since_unix_epoch(duration: Duration) -> Self {
        let system_time = SystemTime::UNIX_EPOCH + duration;
        Self { system_time }
    }

    pub fn now() -> Self {
        Self {
            system_time: SystemTime::now(),
        }
    }

    pub fn to_unix_epoch(self) -> Duration {
        self.system_time
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
    }
}

impl Add<Duration> for Timestamp {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        let system_time = self.system_time + rhs;
        Self { system_time }
    }
}

impl Sub<Duration> for Timestamp {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        let system_time = self.system_time - rhs;
        Self { system_time }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Balance {
    pub id: BalanceId,
    pub currency_id: CurrencyId,
    pub timestamp: Timestamp,
    pub balance: Amount,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Market {
    pub id: MarketId,
    pub base: CurrencyId,
    pub quote: CurrencyId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Price {
    pub id: PriceId,
    pub market: MarketId,
    pub timestamp: Timestamp,
    pub price: Amount,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Orderbook {
    pub id: OrderbookId,
    pub market: MarketId,
    pub timestamp: Timestamp,
    pub kind: OrderKind,
    pub price: Amount,
    pub volume: Amount,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub id: OrderId,
    pub transaction_id: String,
    pub market: MarketId,
    pub created: Timestamp,
    pub modified: Timestamp,
    pub price: Amount,
    pub base_quantity: Amount,
    pub quote_quantity: Amount,
    pub state: OrderState,
}
