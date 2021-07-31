use apply::Apply;
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

fn get_sim_next_balance_id(sim_conn: &MysqlConnection) -> IdType {
    schema::balance::table
        .select(max(schema::balance::balance_id))
        .first::<Option<i32>>(sim_conn)
        .unwrap_or(None)
        .unwrap_or(0)
        + 1
}

fn sync_balance(conn: &MysqlConnection, sim_conn: &MysqlConnection, stamp: Stamp) -> Result<()> {
    let balances = schema::balance::table
        .filter(schema::balance::stamp_id.eq(stamp.stamp_id))
        .load::<Balance>(conn)?;

    debug!(LOGGER, "Sync: found {} balances", balances.len());

    sim_conn.transaction::<(), diesel::result::Error, _>(|| {
        for mut balance in balances.into_iter() {
            balance.balance_id = get_sim_next_balance_id(sim_conn);
            insert_into(schema::balance::table)
                .values(balance)
                .execute(sim_conn)?;
        }

        Ok(())
    })?;

    Ok(())
}

fn simulate_trade<'a>(
    speculators: impl IntoIterator<Item = &'a MultipleRsiSpeculator>,
    currency_collection: &CurrencyCollection,
    market_collection: &MarketCollection,
    current_balances: &[Balance],
    fee_ratio: Amount,
) -> Result<HashMap<IdType, Balance>> {
    let mut balances = current_balances
        .iter()
        .map(|balance| (balance.currency_id, balance.clone()))
        .collect::<HashMap<_, _>>();

    for speculator in speculators.into_iter() {
        let base_id = speculator.market().base_id;
        let quote_id = speculator.market().quote_id;
        let base_balance = match balances.get(&base_id) {
            Some(b) => b,
            None => {
                warn!(LOGGER, "Currency id {} is not found in balances", base_id);
                continue;
            }
        };
        let quote_balance = match balances.get(&quote_id) {
            Some(b) => b,
            None => {
                warn!(LOGGER, "Currency id {} is not found in balances", quote_id);
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

            let market = market_collection.by_id(order.market_id).unwrap();
            let base = currency_collection.by_id(market.base_id).unwrap();
            let quote = currency_collection.by_id(market.quote_id).unwrap();

            let base_diff = match order.side {
                OrderSide::Buy => order.base_quantity * (1.0 - fee_ratio),
                OrderSide::Sell => -order.base_quantity,
            };
            let quote_diff = match order.side {
                OrderSide::Buy => -order.quote_quantity,
                OrderSide::Sell => order.quote_quantity * (1.0 - fee_ratio),
            };

            balances.get_mut(&base.currency_id).unwrap().available += base_diff;
            balances.get_mut(&quote.currency_id).unwrap().available += quote_diff;

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

    Ok(balances)
}

fn batch() -> Result<()> {
    let url = env::var("DATABASE_URL")?;
    let conn = Conn::establish(&url)?;

    let rsi_window_size = env::var("RSI_WINDOW_SIZE")?.apply(|s| usize::from_str(&s))?;
    let rsi_timespans = env::var("RSI_CHUNK_TIME_MINUTES")?
        .apply_ref(|minutes_str| parse_rsi_timespans(minutes_str))?;

    let currency_collection = list_currencies(&conn)?;
    let market_collection = list_markets(&conn)?;

    let target_market_ids = {
        let speculator_target_markets = env::var("SPECULATOR_TARGET_MARKETS")?;
        let speculator_target_markets = parse_market_symbols(
            &speculator_target_markets,
            &currency_collection,
            &market_collection,
        );
        speculator_target_markets
            .iter()
            .map(|(_, _, market)| market.market_id)
            .collect::<Vec<_>>()
    };

    let records = {
        let oldest_stamp_in_rsi_window = schema::stamp::table
            .order(schema::stamp::stamp_id.desc())
            .limit(2 * 12 * 4 * 20 * target_market_ids.len() as i64)
            .load::<Stamp>(&conn)?
            .last()
            .cloned()
            .ok_opt("No timestamp exists")?;
        schema::price::table
            .inner_join(
                schema::market::table.on(schema::market::market_id.eq(schema::price::market_id)),
            )
            .inner_join(
                schema::stamp::table.on(schema::price::stamp_id.eq(schema::stamp::stamp_id)),
            )
            .filter(schema::market::market_id.eq_any(target_market_ids))
            .filter(schema::stamp::timestamp.ge(oldest_stamp_in_rsi_window.timestamp))
            .order(schema::stamp::stamp_id)
            .load::<(Price, Market, Stamp)>(&conn)?
    };

    let mut speculators = HashMap::<IdType, MultipleRsiSpeculator>::new();

    debug!(LOGGER, "Speculation source record count: {}", records.len());

    for (price, market, stamp) in records.into_iter() {
        let speculator = speculators
            .entry(market.market_id)
            .or_insert(MultipleRsiSpeculator::new(
                market.clone(),
                rsi_window_size,
                rsi_timespans.clone(),
            ));

        let market_state = MarketState {
            stamp,
            price,
            orderbooks: vec![],
            myorders: vec![],
        };

        speculator.update_market_state(market_state);
    }

    let latest_stamp = schema::stamp::table
        .order(schema::stamp::stamp_id.desc())
        .first::<Stamp>(&conn)?;

    let sim_conn = env::var("SIM_DATABASE_URL")?
        .deref()
        .apply_ref(Conn::establish)?;

    if let Ok("1") = env::var("SIM_SYNC_BALANCE").as_deref() {
        sync_balance(&conn, &sim_conn, latest_stamp.clone())?;
        info!(LOGGER, "Sync balance. timestamp: {:?}", latest_stamp);
        Ok(())
    } else {
        batch_sim(
            &sim_conn,
            &currency_collection,
            &market_collection,
            latest_stamp,
            speculators.values(),
        )
    }
}

fn batch_sim<'a>(
    sim_conn: &MysqlConnection,
    currency_collection: &CurrencyCollection,
    market_collection: &MarketCollection,
    latest_stamp: Stamp,
    speculators: impl IntoIterator<Item = &'a MultipleRsiSpeculator>,
) -> Result<()> {
    let previous_stamp_id = schema::balance::table
        .select(max(schema::balance::stamp_id))
        .first::<Option<IdType>>(sim_conn)?
        .expect("No balance exists in simulation DB");

    let current_balances = schema::balance::table
        .filter(schema::balance::stamp_id.eq(previous_stamp_id))
        .load::<Balance>(sim_conn)?;
    let sim_fee_ratio = env::var("SIM_FEE_RATIO")?.deref().apply(Amount::from_str)?;

    let new_balances = simulate_trade(
        speculators,
        &currency_collection,
        &market_collection,
        &current_balances,
        sim_fee_ratio,
    )?;

    for (
        currency_id,
        Balance {
            available, pending, ..
        },
    ) in new_balances.into_iter()
    {
        let balance_id = get_sim_next_balance_id(sim_conn);
        let balance = Balance::new(
            balance_id,
            currency_id,
            latest_stamp.stamp_id,
            available,
            pending,
        );
        insert_into(schema::balance::table)
            .values(balance)
            .execute(sim_conn)?;
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
