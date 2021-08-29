use apply::Also;
use apply::Apply;
use common::alias::Result;
use common::err::OkOpt;
use common::log::prelude::*;
use database::logic::*;
use database::model::*;
use database::schema;
use diesel::prelude::*;
use diesel::update;
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
    let rsi_candlestick_count =
        env::var("RSI_CANDLESTICK_COUNT")?.apply(|s| usize::from_str(&s))?;
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
        let rsi_oldest_timestamp =
            latest_main_stamp.timestamp - longest_timespan * rsi_candlestick_count as i32 * 2;
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
                rsi_candlestick_count,
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

fn load_latest_balances(
    conn: &Conn,
    latest_stamp: Stamp,
    currency_collection: &CurrencyCollection,
) -> Result<HashMap<IdType, Balance>> {
    let balances = schema::balance::table
        .filter(schema::balance::stamp_id.eq(latest_stamp.stamp_id))
        .load::<Balance>(conn)?;

    currency_collection
        .currencies()
        .iter()
        .filter_map(
            |c| match balances.iter().find(|b| b.currency_id == c.currency_id) {
                Some(b) => Some(b.clone()),
                None => {
                    debug!(LOGGER, "Currency {} is not found in balances", c.name);
                    None
                }
            },
        )
        .map(|b| (b.currency_id, b))
        .collect::<HashMap<_, _>>()
        .apply(Ok)
}

fn simulate_trade() -> Result<()> {
    let sim_url = env::var("SIM_DATABASE_URL")?;
    let conn = Conn::establish(&sim_url)?;

    let latest_stamp = schema::stamp::table
        .order(schema::stamp::stamp_id.desc())
        .first::<Stamp>(&conn)?;
    let currency_collection = list_currencies(&conn)?;
    let market_collection = list_markets(&conn)?;

    let speculators = construct_speculators(
        &conn,
        &currency_collection,
        &market_collection,
        latest_stamp.clone(),
    )?;

    let mut current_balances = load_latest_balances(&conn, latest_stamp, &currency_collection)?;

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

    // Update balances after trade
    for (_, updated_balance) in current_balances.into_iter() {
        let target_row = schema::balance::table
            .filter(schema::balance::balance_id.eq(updated_balance.balance_id));
        if let Err(e) = update(target_row)
            .set(schema::balance::available.eq(updated_balance.available))
            .execute(&conn)
        {
            warn!(LOGGER, "Can't update balance: {}", e);
        }
    }

    Ok(())
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

    if let Err(e) = simulate_trade() {
        error!(LOGGER, "{}", e);
    }

    info!(LOGGER, "Nicehash speculator finished");
}
