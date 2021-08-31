use crate::error::{Error, LogicError, Result};
use crate::model::*;
use crate::schema::*;
use apply::Apply;
use chrono::NaiveDateTime;
use diesel::expression::dsl::exists;
use diesel::prelude::*;

pub type Conn = diesel::mysql::MysqlConnection;

#[derive(Debug, Clone)]
pub struct CurrencyCollection {
    currencies: Vec<Currency>,
}

impl CurrencyCollection {
    pub fn currencies(&self) -> &[Currency] {
        self.currencies.as_slice()
    }

    pub fn by_id(&self, currency_id: CurrencyId) -> Option<&Currency> {
        self.currencies
            .iter()
            .find(|c| c.currency_id == currency_id)
    }

    pub fn by_symbol<S: AsRef<str>>(&self, symbol: S) -> Option<&Currency> {
        self.currencies.iter().find(|c| c.symbol == symbol.as_ref())
    }
}

#[derive(Debug, Clone)]
pub struct MarketCollection {
    markets: Vec<Market>,
}

impl MarketCollection {
    pub fn markets(&self) -> &[Market] {
        self.markets.as_slice()
    }

    pub fn by_id(&self, market_id: MarketId) -> Option<&Market> {
        self.markets.iter().find(|m| m.market_id == market_id)
    }

    pub fn by_base_quote_id(
        &self,
        base_currency_id: CurrencyId,
        quote_currency_id: CurrencyId,
    ) -> Option<&Market> {
        self.markets()
            .iter()
            .filter(|m| m.base_id == base_currency_id)
            .filter(|m| m.quote_id == quote_currency_id)
            .next()
    }
}

pub fn list_currencies(conn: &Conn) -> Result<CurrencyCollection> {
    currency::table
        .load(conn)
        .map(|currencies| CurrencyCollection { currencies })
        .map_err(Into::into)
}

pub fn add_currency(conn: &Conn, symbol: String, name: String) -> Result<Currency> {
    let already_exists = currency::table
        .filter(currency::symbol.eq(&symbol))
        .filter(currency::name.eq(&name))
        .apply(exists)
        .apply(diesel::select)
        .get_result(conn)?;
    if already_exists {
        return Err(LogicError::DuplicatedCurrency.into());
    }

    let currency_id: CurrencyId = next_id::table.select(next_id::currency).first(conn)?;

    let currency = Currency::new(currency_id, symbol, name);
    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::table
            .apply(diesel::update)
            .set(next_id::currency.eq(next_id::currency + 1))
            .execute(conn)?;

        // Add currency
        currency::table
            .apply(diesel::insert_into)
            .values(&currency)
            .execute(conn)?;

        Ok(())
    })?;

    Ok(currency)
}

pub fn add_stamp(conn: &Conn, timestamp: NaiveDateTime) -> Result<Stamp> {
    let stamp_id = next_id::table.select(next_id::stamp).first(conn)?;
    let stamp = Stamp::new(stamp_id, timestamp);

    conn.transaction::<(), Error, _>(|| {
        next_id::table
            .apply(diesel::update)
            .set(next_id::stamp.eq(next_id::stamp + 1))
            .execute(conn)?;

        stamp::table
            .apply(diesel::insert_into)
            .values(&stamp)
            .execute(conn)?;

        Ok(())
    })?;

    Ok(stamp)
}

pub fn add_balance(
    conn: &Conn,
    currency_id: CurrencyId,
    stamp_id: StampId,
    available: Amount,
    pending: Amount,
) -> Result<Balance> {
    let balance_id = next_id::table.select(next_id::balance).first(conn)?;

    let balance = Balance::new(balance_id, currency_id, stamp_id, available, pending);

    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::table
            .apply(diesel::update)
            .set(next_id::balance.eq(next_id::balance + 1))
            .execute(conn)?;

        // Add balance
        balance::table
            .apply(diesel::insert_into)
            .values(&balance)
            .execute(conn)?;

        Ok(())
    })?;

    Ok(balance)
}

pub fn list_markets(conn: &Conn) -> Result<MarketCollection> {
    market::table
        .load(conn)
        .map(|markets| MarketCollection { markets })
        .map_err(Into::into)
}

pub fn add_market(
    conn: &Conn,
    base_currency_id: CurrencyId,
    quote_currency_id: CurrencyId,
) -> Result<Market> {
    if market::table
        .filter(market::base_id.eq(base_currency_id))
        .filter(market::quote_id.eq(quote_currency_id))
        .apply(exists)
        .apply(diesel::select)
        .get_result(conn)?
    {
        return Err(LogicError::DuplicatedMarket.into());
    }

    let market_id = next_id::table.select(next_id::market).first(conn)?;

    let market = Market::new(market_id, base_currency_id, quote_currency_id);

    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::table
            .apply(diesel::update)
            .set(next_id::market.eq(next_id::market + 1))
            .execute(conn)?;

        // Add market
        market::table
            .apply(diesel::insert_into)
            .values(&market)
            .execute(conn)?;

        Ok(())
    })?;

    Ok(market)
}

pub fn add_price(
    conn: &Conn,
    market_id: MarketId,
    stamp_id: StampId,
    amount: Amount,
) -> Result<Price> {
    let price_id = next_id::table.select(next_id::price).first(conn)?;

    let price = Price::new(price_id, market_id, stamp_id, amount);

    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::table
            .apply(diesel::update)
            .set(next_id::price.eq(next_id::price + 1))
            .execute(conn)?;

        // Add price
        price::table
            .apply(diesel::insert_into)
            .values(&price)
            .execute(conn)?;

        Ok(())
    })?;

    Ok(price)
}

pub fn add_orderbook(
    conn: &Conn,
    market_id: MarketId,
    stamp_id: StampId,
    side: OrderSide,
    price: Amount,
    volume: Amount,
) -> Result<Orderbook> {
    let orderbook_id = next_id::table.select(next_id::orderbook).first(conn)?;

    let orderbook = Orderbook {
        orderbook_id,
        market_id,
        stamp_id,
        side,
        price,
        volume,
    };

    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::table
            .apply(diesel::update)
            .set(next_id::orderbook.eq(next_id::orderbook + 1))
            .execute(conn)?;

        // Add orderbook
        orderbook::table
            .apply(diesel::insert_into)
            .values(&orderbook)
            .execute(conn)?;

        Ok(())
    })?;

    Ok(orderbook)
}

pub fn add_or_update_myorder(
    conn: &Conn,
    transaction_id: String,
    market_id: MarketId,
    now_stamp_id: StampId,
    price: Amount,
    base_quantity: Amount,
    quote_quantity: Amount,
    order_type: OrderType,
    side: OrderSide,
    state: OrderState,
) -> Result<()> {
    let already_exists = myorder::table
        .filter(myorder::transaction_id.eq(&transaction_id))
        .apply(exists)
        .apply(diesel::select)
        .get_result(conn)?;

    if already_exists {
        // If order state changed, update order state.
        // Otherwise, nothing executed.
        return myorder::table
            .filter(myorder::transaction_id.eq(&transaction_id))
            .filter(myorder::state.ne(state))
            .apply(diesel::update)
            .set((
                myorder::modified_stamp_id.eq(now_stamp_id),
                myorder::state.eq(&state),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(Into::into);
    }

    let myorder_id = next_id::table.select(next_id::myorder).first(conn)?;

    let myorder = MyOrder {
        myorder_id,
        transaction_id,
        market_id,
        created_stamp_id: now_stamp_id,
        modified_stamp_id: now_stamp_id,
        price,
        base_quantity,
        quote_quantity,
        order_type,
        side,
        state,
    };

    conn.transaction::<(), Error, _>(|| {
        // Update next id
        next_id::table
            .apply(diesel::update)
            .set(next_id::myorder.eq(next_id::myorder + 1))
            .execute(conn)?;

        // Add order
        myorder::table
            .apply(diesel::insert_into)
            .values(&myorder)
            .execute(conn)?;

        Ok(())
    })?;

    Ok(())
}
