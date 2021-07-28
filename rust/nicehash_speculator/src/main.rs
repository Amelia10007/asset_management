use apply::Apply;
use common::alias::Result;
use common::err::OkOpt;
use common::log::prelude::*;
use database::logic::*;
use database::model::*;
use database::schema;
use diesel::prelude::*;
use once_cell::sync::Lazy;
use speculator::rsi::Duration;
use speculator::speculator::{MarketState, MultipleRsiSpeculator, OrderRecommendation, Speculator};
use std::collections::HashMap;
use std::env;
use std::io::{stdout, Stdout};
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

fn batch() -> Result<()> {
    let url = env::var("DATABASE_URL")?;
    let conn = Conn::establish(&url)?;

    let rsi_window_size = env::var("RSI_WINDOW_SIZE")?.apply(|s| usize::from_str(&s))?;
    let rsi_timespans = env::var("RSI_CHUNK_TIME_MINUTES")?
        .apply_ref(|minutes_str| parse_rsi_timespans(minutes_str))?;

    let currency_collection = list_currencies(&conn)?;
    let market_collection = list_markets(&conn)?;

    let speculator_target_markets = env::var("SPECULATOR_TARGET_MARKETS")?;
    let speculator_target_markets = parse_market_symbols(
        &speculator_target_markets,
        &currency_collection,
        &market_collection,
    );
    let target_market_ids = speculator_target_markets
        .iter()
        .map(|(_, _, market)| market.market_id)
        .collect::<Vec<_>>();

    let oldest_stamp_in_rsi_window = schema::stamp::table
        .order(schema::stamp::stamp_id.desc())
        .limit(2 * 12 * 4 * 20 * speculator_target_markets.len() as i64)
        .load::<Stamp>(&conn)?
        .last()
        .cloned()
        .ok_opt("No timestamp exists")?;

    let records = schema::price::table
        .inner_join(
            schema::market::table.on(schema::market::market_id.eq(schema::price::market_id)),
        )
        .inner_join(schema::stamp::table.on(schema::price::stamp_id.eq(schema::stamp::stamp_id)))
        .filter(schema::market::market_id.eq_any(target_market_ids))
        .filter(schema::stamp::timestamp.ge(oldest_stamp_in_rsi_window.timestamp))
        .order(schema::stamp::stamp_id)
        .load::<(Price, Market, Stamp)>(&conn)?;

    let mut speculators = HashMap::<IdType, MultipleRsiSpeculator>::new();

    debug!(LOGGER, "Speculation source record count: {}", records.len());

    for (price, market, stamp) in records.into_iter() {
        let speculator = speculators
            .entry(market.market_id)
            .or_insert(MultipleRsiSpeculator::new(
                market,
                rsi_window_size,
                rsi_timespans.clone(),
            ));

        let market_state = MarketState {
            stamp,
            price,
            balance: Balance::new(0, 0, 0, 0., 0.),
            orderbooks: vec![],
            myorders: vec![],
        };

        speculator.update_market_state(market_state);
    }

    for speculator in speculators.values() {
        let recommendations = speculator.recommend();

        debug!(
            LOGGER,
            "Recommendation count in market {}: {}",
            speculator.market().market_id,
            recommendations.len()
        );

        for recommend in recommendations.into_iter() {
            match recommend {
                OrderRecommendation::Open(order, description) => {
                    info!(LOGGER, "{}: {:?}", description.reason(), order)
                }
                OrderRecommendation::Cancel(order, description) => {
                    info!(LOGGER, "{}: {}", description.reason(), order.transaction_id)
                }
            }
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

    if let Err(e) = batch() {
        error!(LOGGER, "{}", e);
    }

    info!(LOGGER, "Nicehash speculator finished");
}
