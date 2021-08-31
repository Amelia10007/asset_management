pub use crate::custom_sql_type::*;
use crate::schema::*;
pub use chrono::NaiveDateTime;

pub type Amount = f32;

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "currency"]
pub struct Currency {
    pub currency_id: CurrencyId,
    pub symbol: String,
    pub name: String,
}

impl Currency {
    pub fn new(currency_id: CurrencyId, symbol: String, name: String) -> Self {
        Self {
            currency_id,
            symbol,
            name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "stamp"]
pub struct Stamp {
    pub stamp_id: StampId,
    pub timestamp: NaiveDateTime,
}

impl Stamp {
    pub fn new(stamp_id: StampId, timestamp: NaiveDateTime) -> Self {
        Self {
            stamp_id,
            timestamp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "balance"]
pub struct Balance {
    pub balance_id: BalanceId,
    pub currency_id: CurrencyId,
    pub stamp_id: StampId,
    pub available: Amount,
    pub pending: Amount,
}

impl Balance {
    pub fn new(
        balance_id: BalanceId,
        currency_id: CurrencyId,
        stamp_id: StampId,
        available: Amount,
        pending: Amount,
    ) -> Self {
        Self {
            balance_id,
            currency_id,
            stamp_id,
            available,
            pending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "market"]
pub struct Market {
    pub market_id: MarketId,
    pub base_id: CurrencyId,
    pub quote_id: CurrencyId,
}

impl Market {
    pub fn new(market_id: MarketId, base_id: CurrencyId, quote_id: CurrencyId) -> Self {
        Self {
            market_id,
            base_id,
            quote_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "price"]
pub struct Price {
    pub price_id: PriceId,
    pub market_id: MarketId,
    pub stamp_id: StampId,
    pub amount: Amount,
}

impl Price {
    pub fn new(price_id: PriceId, market_id: MarketId, stamp_id: StampId, amount: Amount) -> Self {
        Self {
            price_id,
            market_id,
            stamp_id,
            amount,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "orderbook"]
pub struct Orderbook {
    pub orderbook_id: OrderbookId,
    pub market_id: MarketId,
    pub stamp_id: StampId,
    pub side: OrderSide,
    pub price: Amount,
    pub volume: Amount,
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "myorder"]
pub struct MyOrder {
    pub myorder_id: MyorderId,
    pub transaction_id: String,
    pub market_id: MarketId,
    pub created_stamp_id: StampId,
    pub modified_stamp_id: StampId,
    pub price: Amount,
    pub base_quantity: Amount,
    pub quote_quantity: Amount,
    pub order_type: OrderType,
    pub side: OrderSide,
    pub state: OrderState,
}
