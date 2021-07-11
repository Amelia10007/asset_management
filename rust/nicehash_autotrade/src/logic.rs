use crate::error::{Error, LogicError, Result};
use crate::model::*;
use crate::schema::*;
use apply::Apply;
use chrono::NaiveDateTime;
use diesel::expression::dsl::exists;
use diesel::prelude::*;

pub type Conn = diesel::mysql::MysqlConnection;

pub struct CurrencyCollection {
    currencies: Vec<Currency>,
}

impl CurrencyCollection {
    pub fn currencies(&self) -> impl Iterator<Item = &Currency> {
        self.currencies.iter()
    }

    pub fn by_id(&self, currency_id: IdType) -> Option<&Currency> {
        self.currencies
            .iter()
            .find(|c| c.currency_id == currency_id)
    }

    pub fn by_symbol<S: AsRef<str>>(&self, symbol: S) -> Option<&Currency> {
        self.currencies.iter().find(|c| c.symbol == symbol.as_ref())
    }
}

pub struct MarketCollection {
    markets: Vec<Market>,
}

impl MarketCollection {
    pub fn markets(&self) -> impl Iterator<Item = &Market> {
        self.markets.iter()
    }
}

pub fn list_currencies(conn: &Conn) -> Result<CurrencyCollection> {
    currency::dsl::currency
        .load(conn)
        .map(|currencies| CurrencyCollection { currencies })
        .map_err(Into::into)
}

pub fn add_currency(conn: &Conn, symbol: String, name: String) -> Result<Currency> {
    let already_exists = currency::dsl::currency
        .filter(currency::symbol.eq(&symbol))
        .filter(currency::name.eq(&name))
        .apply(exists)
        .apply(diesel::select)
        .get_result(conn)?;
    if already_exists {
        return Err(LogicError::DuplicatedCurrency.into());
    }

    let currency_id: IdType = next_id::dsl::next_id
        .select(next_id::currency)
        .first(conn)?;

    let currency = Currency::new(currency_id, symbol, name);
    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::dsl::next_id
            .apply(diesel::update)
            .set(next_id::currency.eq(next_id::currency + 1))
            .execute(conn)?;

        // Add currency
        currency::dsl::currency
            .apply(diesel::insert_into)
            .values(currency.clone())
            .execute(conn)?;

        Ok(())
    })?;

    Ok(currency)
}

pub fn add_balance(
    conn: &Conn,
    currency_id: IdType,
    stamp: NaiveDateTime,
    amount: Amount,
) -> Result<Balance> {
    let balance_id = next_id::dsl::next_id.select(next_id::balance).first(conn)?;

    let balance = Balance::new(balance_id, currency_id, stamp, amount);

    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::dsl::next_id
            .apply(diesel::update)
            .set(next_id::balance.eq(next_id::balance + 1))
            .execute(conn)?;

        // Add balance
        balance::dsl::balance
            .apply(diesel::insert_into)
            .values(balance.clone())
            .execute(conn)?;

        Ok(())
    })?;

    Ok(balance)
}

pub fn list_markets(conn: &Conn) -> Result<MarketCollection> {
    market::dsl::market
        .load(conn)
        .map(|markets| MarketCollection { markets })
        .map_err(Into::into)
}

pub fn search_or_add_market(
    conn: &Conn,
    base_currency_id: IdType,
    quote_currency_id: IdType,
) -> Result<Market> {
    match market::dsl::market
        .filter(market::base_id.eq(base_currency_id))
        .filter(market::quote_id.eq(quote_currency_id))
        .first(conn)
        .optional()
    {
        Ok(Some(market)) => return Ok(market),
        Ok(None) => {}
        Err(e) => return Err(e.into()),
    }

    let market_id = next_id::dsl::next_id.select(next_id::market).first(conn)?;

    let market = Market::new(market_id, base_currency_id, quote_currency_id);

    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::dsl::next_id
            .apply(diesel::update)
            .set(next_id::market.eq(next_id::market + 1))
            .execute(conn)?;

        // Add market
        market::dsl::market
            .apply(diesel::insert_into)
            .values(market.clone())
            .execute(conn)?;

        Ok(())
    })?;

    Ok(market)
}

pub fn add_price(
    conn: &Conn,
    market_id: IdType,
    stamp: NaiveDateTime,
    amount: Amount,
) -> Result<Price> {
    let price_id = next_id::dsl::next_id.select(next_id::price).first(conn)?;

    let price = Price::new(price_id, market_id, stamp, amount);

    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::dsl::next_id
            .apply(diesel::update)
            .set(next_id::price.eq(next_id::price + 1))
            .execute(conn)?;

        // Add price
        price::dsl::price
            .apply(diesel::insert_into)
            .values(price.clone())
            .execute(conn)?;

        Ok(())
    })?;

    Ok(price)
}

pub fn add_orderbook(
    conn: &Conn,
    market_id: IdType,
    stamp: NaiveDateTime,
    order_kind: OrderKind,
    price: Amount,
    volume: Amount,
) -> Result<Orderbook> {
    let orderbook_id = next_id::dsl::next_id
        .select(next_id::orderbook)
        .first(conn)?;
    let is_buy = order_kind.is_buy();

    let orderbook = Orderbook {
        orderbook_id,
        market_id,
        stamp,
        is_buy,
        price,
        volume,
    };

    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::dsl::next_id
            .apply(diesel::update)
            .set(next_id::orderbook.eq(next_id::orderbook + 1))
            .execute(conn)?;

        // Add orderbook
        orderbook::dsl::orderbook
            .apply(diesel::insert_into)
            .values(orderbook.clone())
            .execute(conn)?;

        Ok(())
    })?;

    Ok(orderbook)
}

pub fn add_or_update_myorder(
    conn: &Conn,
    transaction_id: String,
    market_id: IdType,
    now: NaiveDateTime,
    price: Amount,
    base_quantity: Amount,
    quote_quantity: Amount,
    state: String,
) -> Result<()> {
    if let Ok(1) = myorder::table
        .filter(myorder::transaction_id.eq(&transaction_id))
        .apply(diesel::update)
        .set((myorder::modified.eq(now), myorder::state.eq(&state)))
        .execute(conn)
    {
        return Ok(());
    }

    let myorder_id = next_id::dsl::next_id.select(next_id::myorder).first(conn)?;

    let myorder = MyOrder {
        myorder_id,
        transaction_id,
        market_id,
        created: now,
        modified: now,
        price,
        base_quantity,
        quote_quantity,
        state,
    };

    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::dsl::next_id
            .apply(diesel::update)
            .set(next_id::myorder.eq(next_id::myorder + 1))
            .execute(conn)?;

        // Add order
        myorder::dsl::myorder
            .apply(diesel::insert_into)
            .values(myorder.clone())
            .execute(conn)?;

        Ok(())
    })?;

    Ok(())
}
