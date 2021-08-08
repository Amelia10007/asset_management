use apply::Apply;
use common::alias::BoxErr;
use common::alias::Result;
use common::err::OkOpt;
use common::log::prelude::*;
use database::logic::*;
use database::model::*;
use database::schema;
use diesel::dsl::max;
use diesel::insert_into;
use diesel::prelude::*;
use once_cell::sync::Lazy;
use speculator::rsi::Duration;
use speculator::speculator::{MarketState, MultipleRsiSpeculator, Speculator};
use std::collections::HashMap;
use std::env;
use std::io::{stdout, Stdout};
use std::ops::Deref;
use std::str::FromStr;

static LOGGER: Lazy<Logger<Stdout>> = Lazy::new(|| {
    let level = match env::var("SPECULATOR_LOGGER_LEVEL")
        .map(|s| s.to_lowercase())
        .as_deref()
    {
        Ok("error") => LogLevel::Error,
        Ok("warn") => LogLevel::Warning,
        Ok("info") => LogLevel::Info,
        Ok("debug") => LogLevel::Debug,
        _ => LogLevel::Debug,
    };
    Logger::new(stdout(), level)
});

fn sync_balance(conn: &Conn, balance_sim_conn: &Conn, latest_main_stamp: Stamp) -> Result<()> {
    let balances = schema::balance::table
        .filter(schema::balance::stamp_id.eq(latest_main_stamp.stamp_id))
        .load::<Balance>(conn)?;

    info!(LOGGER, "Sync: found {} balances in main DB", balances.len());

    for mut balance in balances.into_iter() {
        balance.balance_id = get_sim_next_balance_id(balance_sim_conn);
        insert_into(schema::balance::table)
            .values(balance)
            .execute(balance_sim_conn)?;
    }

    info!(LOGGER, "Synced balances with main DB");

    Ok(())
}

fn get_latest_stamp(conn: &Conn) -> Result<Stamp> {
    schema::stamp::table
        .order(schema::stamp::stamp_id.desc())
        .first(conn)
        .map_err(Into::into)
}

fn parse_rsi_timespans(minutes_str: &str) -> Result<Vec<Duration>> {
    minutes_str
        .split(':')
        .map(|minutes_str| {
            i64::from_str(minutes_str)
                .map(Duration::minutes)
                .map_err(Into::into)
        })
        .collect()
}

fn parse_market_symbols(
    s: &str,
    currency_collection: &CurrencyCollection,
    market_collection: &MarketCollection,
) -> Vec<(Currency, Currency, Market)> {
    s.split(':')
        .map(|symbol_pair| symbol_pair.split('-'))
        .filter_map(|mut iter| match (iter.next(), iter.next()) {
            (Some(base), Some(quote)) => Some((base, quote)),
            _ => None,
        })
        .filter_map(|(base_symbol, quote_symbol)| {
            let base = currency_collection.by_symbol(base_symbol)?;
            let quote = currency_collection.by_symbol(quote_symbol)?;
            let market = market_collection.by_base_quote_id(base.currency_id, quote.currency_id)?;
            Some((base.clone(), quote.clone(), market.clone()))
        })
        .collect()
}

/// # Returns
/// Vec of `(base currency, quote currency, rsi)` if succeeds
fn construct_speculators(
    conn: &Conn,
    currency_collection: &CurrencyCollection,
    market_collection: &MarketCollection,
    latest_main_stamp: Stamp,
) -> Result<Vec<(Currency, Currency, MultipleRsiSpeculator)>> {
    let rsi_timespans = env::var("RSI_TIMESPAN_MINUTES")?
        .apply_ref(|minutes_str| parse_rsi_timespans(minutes_str))?;
    let spend_balance_ratio =
        env::var("SIM_SPEND_BALANCE_RATIO")?.apply(|s| Amount::from_str(&s))?;

    let longest_timespan = rsi_timespans
        .iter()
        .max()
        .copied()
        .ok_opt("At least one rsi-timespan is required")?;
    // Twice timespan is required to obtain RSIs in the specified timespan
    let rsi_oldest_stamp = latest_main_stamp.timestamp - longest_timespan * 2;

    let market_symbols = env::var("SPECULATOR_TARGET_MARKETS")?;
    let target_markets =
        parse_market_symbols(&market_symbols, currency_collection, market_collection);
    debug!(
        LOGGER,
        "Oldest stamp within RSI window: {}", rsi_oldest_stamp
    );

    let speculators = target_markets
        .into_iter()
        .filter_map(|(base, quote, market)| {
            let market_id = market.market_id;
            let price_stamps = match schema::price::table
                .inner_join(
                    schema::stamp::table.on(schema::price::stamp_id.eq(schema::stamp::stamp_id)),
                )
                .filter(schema::price::market_id.eq(market_id))
                .filter(schema::stamp::timestamp.ge(rsi_oldest_stamp))
                .load::<(Price, Stamp)>(conn)
            {
                Ok(price_stamps) => Some(price_stamps),
                Err(e) => {
                    warn!(LOGGER, "Can't fetch prices for market {}: {}", market_id, e);
                    None
                }
            }?;
            debug!(LOGGER, "Use {} price-stamp pairs", price_stamps.len());

            let mut speculator =
                MultipleRsiSpeculator::new(market, rsi_timespans.clone(), spend_balance_ratio);
            for (price, stamp) in price_stamps.into_iter() {
                let orderbooks = schema::orderbook::table
                    .filter(schema::orderbook::stamp_id.eq(stamp.stamp_id))
                    .filter(schema::orderbook::market_id.eq(market_id))
                    .load(conn)
                    .unwrap_or(vec![]);
                let myorders = schema::myorder::table
                    .filter(schema::myorder::market_id.eq(market_id))
                    .filter(schema::myorder::modified_stamp_id.eq(stamp.stamp_id))
                    .load(conn)
                    .unwrap_or(vec![]);
                let market_state = MarketState {
                    stamp,
                    price,
                    orderbooks,
                    myorders,
                };
                speculator.update_market_state(market_state);
            }
            Some((base, quote, speculator))
        })
        .collect();

    Ok(speculators)
}

fn load_latest_sim_balances(
    balance_sim_conn: &Conn,
    currency_collection: &CurrencyCollection,
) -> Result<HashMap<IdType, Balance>> {
    let latest_balance_stamp_id = schema::balance::table
        .select(max(schema::balance::stamp_id))
        .first::<Option<IdType>>(balance_sim_conn)?
        .ok_opt("No balance exists in simulation DB")?;
    currency_collection
        .currencies()
        .into_iter()
        .filter_map(|c| {
            let latest_balance = schema::balance::table
                .filter(schema::balance::currency_id.eq(c.currency_id))
                .order_by(schema::balance::stamp_id.desc())
                .first(balance_sim_conn)
                .optional();
            match latest_balance {
                Ok(Some(balance)) => Some(balance),
                Ok(None) => {
                    info!(
                        LOGGER,
                        "Currency {} is not found in simulation balances. Its balance is assumed 0",
                        c.name
                    );
                    let balance_id = get_sim_next_balance_id(balance_sim_conn);
                    let balance =
                        Balance::new(balance_id, c.currency_id, latest_balance_stamp_id, 0.0, 0.0);
                    Some(balance)
                }
                Err(e) => {
                    warn!(LOGGER, "Can't fetch balance of currency {}: {}", c.name, e);
                    None
                }
            }
        })
        .map(|b| (b.currency_id, b))
        .collect::<HashMap<_, _>>()
        .apply(Ok)
}

fn get_sim_next_balance_id(balance_sim_conn: &Conn) -> IdType {
    schema::balance::table
        .select(max(schema::balance::balance_id))
        .first::<Option<i32>>(balance_sim_conn)
        .unwrap_or(None)
        .unwrap_or(0)
        + 1
}

fn simulate_trade(conn: &Conn, balance_sim_conn: &Conn, latest_main_stamp: Stamp) -> Result<()> {
    let currency_collection = list_currencies(&conn)?;
    let market_collection = list_markets(&conn)?;

    let speculators = construct_speculators(
        conn,
        &currency_collection,
        &market_collection,
        latest_main_stamp.clone(),
    )?;

    let mut current_balances = load_latest_sim_balances(&balance_sim_conn, &currency_collection)?;

    let fee_ratio = env::var("SIM_FEE_RATIO")?.deref().apply(Amount::from_str)?;

    for (base, quote, speculator) in speculators.into_iter() {
        let base_balance = match current_balances.get(&base.currency_id) {
            Some(b) => b,
            None => {
                warn!(LOGGER, "Currency {} is not found in balances", base.name);
                continue;
            }
        };
        let quote_balance = match current_balances.get(&quote.currency_id) {
            Some(b) => b,
            None => {
                warn!(LOGGER, "Currency {} is not found in balances", quote.name);
                continue;
            }
        };

        let recommendations = speculator.recommend(&base_balance, &quote_balance);
        debug!(
            LOGGER,
            "Speculator recommendation: {}, Market: {:?}",
            recommendations.len(),
            speculator.market()
        );

        for recommend in recommendations.into_iter() {
            let order = recommend.incomplete_myorder();

            let base_diff = match order.side {
                OrderSide::Buy => order.base_quantity * (1.0 - fee_ratio),
                OrderSide::Sell => -order.base_quantity,
            };
            let quote_diff = match order.side {
                OrderSide::Buy => -order.quote_quantity,
                OrderSide::Sell => order.quote_quantity * (1.0 - fee_ratio),
            };

            current_balances
                .get_mut(&base.currency_id)
                .unwrap()
                .available += base_diff;
            current_balances
                .get_mut(&quote.currency_id)
                .unwrap()
                .available += quote_diff;

            info!(
                LOGGER,
                "Market:{}-{} Order:{:?}-{:?} price: {}, base_diff:{} quote_diff:{} Reason:{}",
                base.symbol,
                quote.symbol,
                order.order_type,
                order.side,
                order.price,
                base_diff,
                quote_diff,
                recommend.description().reason()
            );
        }
    }

    for (
        currency_id,
        Balance {
            available, pending, ..
        },
    ) in current_balances.into_iter()
    {
        let balance_id = get_sim_next_balance_id(&balance_sim_conn);
        let balance = Balance::new(
            balance_id,
            currency_id,
            latest_main_stamp.stamp_id,
            available,
            pending,
        );
        if let Err(e) = insert_into(schema::balance::table)
            .values(balance)
            .execute(balance_sim_conn)
        {
            warn!(LOGGER, "Can't add new balance: {}", e);
        }
    }

    Ok(())
}

fn batch() -> Result<()> {
    let url = env::var("DATABASE_URL")?;
    let conn = Conn::establish(&url)?;
    let sim_url = env::var("SIM_DATABASE_URL")?;
    let balance_sim_conn = Conn::establish(&sim_url)?;

    let last_sim_stamp_id = schema::balance::table
        .select(max(schema::balance::stamp_id))
        .first::<Option<IdType>>(&balance_sim_conn)?;
    let latest_main_stamp = get_latest_stamp(&conn)?;

    match last_sim_stamp_id {
        Some(id) if id == latest_main_stamp.stamp_id => {
            Err(BoxErr::from("No new timestamp exists in main DB"))
        }
        Some(_) => simulate_trade(&conn, &balance_sim_conn, latest_main_stamp),
        None => sync_balance(&conn, &balance_sim_conn, latest_main_stamp),
    }
}

fn main() {
    dotenv::dotenv().ok();

    let now = match nicehash::api_common::fetch_server_time() {
        Ok(now) => now,
        Err(e) => {
            error!(LOGGER, "Can't fetch nicehash server time: {}", e);
            return;
        }
    };
    info!(LOGGER, "Nicehash speculator started at {}", now);

    if let Err(e) = batch() {
        error!(LOGGER, "{}", e);
    }

    info!(LOGGER, "Nicehash speculator finished");
}
