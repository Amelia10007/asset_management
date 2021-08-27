use apply::Also;
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
use std::hash::Hash;
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

fn group_by<V, K, F>(iter: impl IntoIterator<Item = V>, mut f: F) -> HashMap<K, Vec<V>>
where
    K: Eq + Hash,
    V: Clone,
    F: FnMut(&V) -> K,
{
    let mut map = HashMap::new();
    for v in iter.into_iter() {
        let key = f(&v);
        map.entry(key)
            .and_modify(|vec: &mut Vec<V>| vec.push(v.clone()))
            .or_insert_with(|| vec![v]);
    }

    map
}

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
    let spend_buy_ratio = env::var("SIM_SPEND_BUY_RATIO")?.apply(|s| Amount::from_str(&s))?;
    let spend_sell_ratio = env::var("SIM_SPEND_SELL_RATIO")?.apply(|s| Amount::from_str(&s))?;

    let market_symbols = env::var("SPECULATOR_TARGET_MARKETS")?;
    let target_markets =
        parse_market_symbols(&market_symbols, currency_collection, market_collection);

    // Load RSI-target timestamps
    let stamps = {
        let longest_timespan = rsi_timespans
            .iter()
            .max()
            .copied()
            .ok_opt("At least one rsi-timespan is required")?;
        // Twice timespan is required to obtain RSIs in the specified timespan
        let rsi_oldest_timestamp = latest_main_stamp.timestamp - longest_timespan * 2;
        schema::stamp::table
            .filter(schema::stamp::timestamp.ge(rsi_oldest_timestamp))
            .order_by(schema::stamp::timestamp.asc())
            .load::<Stamp>(conn)?
    };
    let oldest_stamp = stamps.first().cloned().ok_opt("No stamp exists")?;
    debug!(LOGGER, "Oldest stamp in RSI: {}", oldest_stamp.timestamp);

    // Load prices/orderbooks of all markets within RSI timespan, then sort them by timestamp
    let mut price_group = schema::price::table
        .filter(schema::price::stamp_id.ge(oldest_stamp.stamp_id))
        .load::<Price>(conn)?
        .apply(|prices| group_by(prices, |p| p.market_id))
        .also(|group| {
            group
                .values_mut()
                .for_each(|g| g.sort_by_key(|p| p.stamp_id))
        });
    let mut orderbook_group = schema::orderbook::table
        .filter(schema::orderbook::stamp_id.ge(oldest_stamp.stamp_id))
        .load::<Orderbook>(conn)?
        .apply(|orderbooks| group_by(orderbooks, |o| o.market_id))
        .also(|group| {
            group
                .values_mut()
                .for_each(|g| g.sort_by_key(|p| p.stamp_id))
        });

    // Speculator for each market
    let speculators = target_markets
        .into_iter()
        .map(|(base, quote, market)| {
            // Get price/orderbook of speculator's market
            let prices = price_group
                .remove(&market.market_id)
                .unwrap_or(vec![])
                .into_iter()
                .map(|p| (p.stamp_id, p))
                .collect::<HashMap<_, _>>();
            let orderbooks = orderbook_group.remove(&market.market_id).unwrap_or(vec![]);
            debug!(
                LOGGER,
                "Market {}-{}: price count:{},orderbook count:{}",
                base.symbol,
                quote.symbol,
                prices.len(),
                orderbooks.len()
            );

            let mut orderbook_collections = group_by(orderbooks, |o| o.stamp_id);

            let mut speculator = MultipleRsiSpeculator::new(
                market,
                rsi_timespans.clone(),
                spend_buy_ratio,
                spend_sell_ratio,
            );

            // Push market-state sequence
            for stamp in stamps.iter().cloned() {
                let price = prices.get(&stamp.stamp_id);
                let orderbooks = orderbook_collections
                    .remove(&stamp.stamp_id)
                    .unwrap_or(vec![]);
                let myorders = vec![]; // Omit myorder because it is unnecessary yet
                if let Some(price) = price.cloned() {
                    let market_state = MarketState {
                        stamp,
                        price,
                        orderbooks,
                        myorders,
                    };
                    speculator.update_market_state(market_state);
                }
            }

            (base, quote, speculator)
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
