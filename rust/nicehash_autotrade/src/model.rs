use crate::schema::*;
use chrono::NaiveDateTime;

pub type IdType = i32;
pub type Amount = f32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderKind {
    Buy,
    Sell,
}

impl OrderKind {
    pub const fn is_buy(self) -> bool {
        match self {
            OrderKind::Buy => true,
            OrderKind::Sell => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "currency"]
pub struct Currency {
    pub currency_id: IdType,
    pub symbol: String,
    pub name: String,
}

impl Currency {
    pub fn new(currency_id: IdType, symbol: String, name: String) -> Self {
        Self {
            currency_id,
            symbol,
            name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "balance"]
pub struct Balance {
    pub balance_id: IdType,
    pub currency_id: IdType,
    pub stamp: NaiveDateTime,
    pub amount: Amount,
}

impl Balance {
    pub fn new(
        balance_id: IdType,
        currency_id: IdType,
        stamp: NaiveDateTime,
        amount: Amount,
    ) -> Self {
        Self {
            balance_id,
            currency_id,
            stamp,
            amount,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "market"]
pub struct Market {
    pub market_id: IdType,
    pub base_id: IdType,
    pub quote_id: IdType,
}

impl Market {
    pub fn new(market_id: IdType, base_id: IdType, quote_id: IdType) -> Self {
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
    pub price_id: IdType,
    pub market_id: IdType,
    pub stamp: NaiveDateTime,
    pub amount: Amount,
}

impl Price {
    pub fn new(price_id: IdType, market_id: IdType, stamp: NaiveDateTime, amount: Amount) -> Self {
        Self {
            price_id,
            market_id,
            stamp,
            amount,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "orderbook"]
pub struct Orderbook {
    pub orderbook_id: IdType,
    pub market_id: IdType,
    pub stamp: NaiveDateTime,
    pub is_buy: bool,
    pub price: Amount,
    pub volume: Amount,
}

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[table_name = "myorder"]
pub struct MyOrder {
    pub myorder_id: IdType,
    pub transaction_id: String,
    pub market_id: IdType,
    pub created: NaiveDateTime,
    pub modified: NaiveDateTime,
    pub price: Amount,
    pub base_quantity: Amount,
    pub quote_quantity: Amount,
    pub state: String,
}

// pub fn xxx(conn: &diesel::mysql::MysqlConnection) {
//     let currencies = (0..)
//         .map(|i| {
//             let currency_id = i;
//             let symbol = format!("SMB{}", i);
//             let name = format!("Currency-{}", i);
//             Currency {
//                 currency_id,
//                 symbol,
//                 name,
//             }
//         })
//         .take(5)
//         .collect::<Vec<_>>();

//     for c in currencies.into_iter() {
//         diesel::insert_into(currency::dsl::currency)
//             .values(c)
//             .execute(conn)
//             .unwrap();
//     }

//     // get one
//     let c = currency::dsl::currency.first::<Currency>(conn).unwrap();
//     println!("{:?}", c);
//     println!();

//     // get all
//     let cs = currency::dsl::currency.load::<Currency>(conn).unwrap();
//     for c in cs.iter() {
//         println!("{:?}", c);
//     }
//     println!();

//     diesel::update(currency::dsl::currency.filter(currency::currency_id.gt(3)))
//         .set(currency::symbol.eq("WoooHooo"))
//         .execute(conn)
//         .unwrap();

//     let cs = currency::dsl::currency.load::<Currency>(conn).unwrap();
//     for c in cs.iter() {
//         println!("{:?}", c);
//     }
//     println!();

//     diesel::delete(currency::dsl::currency)
//         .filter(currency::currency_id.eq(0))
//         .execute(conn)
//         .unwrap();

//     let cs = currency::dsl::currency.load::<Currency>(conn).unwrap();
//     for c in cs.iter() {
//         println!("{:?}", c);
//     }
//     println!();
// }
