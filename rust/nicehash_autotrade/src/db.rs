// use apply::Apply;
// use common::err::{ErrSuger, OkOpt};
// pub use mysql;
// pub use mysql::time::Date;
// use mysql::TxOpts;
// use mysql::{prelude::*, Statement};
// use mysql::{Conn, Transaction};

// pub mod entity;
// pub mod error;

// use entity::*;
// use error::*;

// type Result<T> = std::result::Result<T, error::Error>;

// pub fn connect_db() -> Result<Conn> {
//     Conn::new("mysql://nicehash:nicehash@localhost:3307/nicehash").map_err(Into::into)
// }

// pub fn search_currency_by_symbol(conn: &mut Conn, symbol: &str) -> Result<Currency> {
//     match conn.exec_first::<(i32, String, String), _, _>(
//         "SELECT id, symbol, name FROM currency
//     WHERE symbol=?",
//         vec![symbol],
//     )? {
//         Some((id, symbol, name)) => {
//             let id = CurrencyId::new(id);
//             Currency::new(id, symbol, name).apply(Ok)
//         }
//         None => Err(LogicError::NotFound.into()),
//     }
// }

// pub fn list_currencies(conn: &mut Conn) -> Result<Vec<Currency>> {
//     conn.query_map(
//         "SELECT id, symbol, name FROM currency",
//         |(id, symbol, name)| {
//             let id = CurrencyId::new(id);
//             Currency::new::<String, String>(id, symbol, name)
//         },
//     )
//     .map_err(From::from)
// }

// pub fn search_market_by_symbols(
//     conn: &mut Conn,
//     base_symbol: &str,
//     quote_symbol: &str,
// ) -> Result<Market> where
// {
//     unimplemented!()
// }

// pub fn add_currency(conn: &mut Conn, symbol: &str, name: &str) -> Result<Currency> {
//     // Prevent duplicated record
//     if let Ok(_) = search_currency_by_symbol(conn, symbol) {
//         Err(LogicError::Duplicated)?;
//     }

//     let next_id = conn
//         .query_first("SELECT currency FROM next_id")?
//         .unwrap_or(0);

//     let mut tx = conn.start_transaction(TxOpts::default())?;

//     tx.exec_drop(
//         "INSERT INTO currency (id, symbol, name) VALUES (?, ?, ?)",
//         (next_id, symbol, name),
//     )?;
//     tx.query_drop("UPDATE next_id SET currency=currency+1")?;

//     tx.commit()?;

//     let id = CurrencyId::new(next_id);
//     let currency = Currency::new(id, symbol, name);

//     Ok(currency)
// }

// pub fn add_market<S1, S2>(conn: &mut Conn, base: &str, quote: &str) -> Result<()> {
//     unimplemented!()
// }

// pub fn add_balance(
//     conn: &mut Conn,
//     symbol: &str,
//     timestamp: Timestamp,
//     balance: Amount,
// ) -> Result<()> {
//     let mut tx = conn.start_transaction(TxOpts::default())?;

//     tx.exec_drop(
//         "INSERT INTO balance (id, currency, stamp, balance)
//     SELECT next_id.balance, currency.id, ?, ?
//     FROM next_id INNER JOIN currency
//     WHERE currency.symbol=?",
//         (timestamp.to_unix_epoch(), balance, symbol),
//     )?;
//     tx.query_drop("UPDATE next_id SET balance=balance+1")?;

//     tx.commit()?;

//     Ok(())
// }
