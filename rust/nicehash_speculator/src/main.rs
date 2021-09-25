mod market_parse;
mod rule_parse;
mod trade_parse;

use anyhow::{anyhow, Result};
use apply::Apply;
use database::logic::*;
use database::model::*;
use database::schema;
use diesel::dsl::max;
use diesel::insert_into;
use diesel::prelude::*;
use speculator::rule::MarketState;
use speculator::rule::RecommendationType;
use speculator::trade::TradeAggregation;
use std::collections::HashMap;
use std::env;
use std::hash::Hash;
#[macro_use]
extern crate log;

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

    info!("Sync: found {} balances in main DB", balances.len());

    for mut balance in balances.into_iter() {
        balance.balance_id = get_sim_next_balance_id(balance_sim_conn);
        insert_into(schema::balance::table)
            .values(balance)
            .execute(balance_sim_conn)?;
    }

    info!("Synced balances with main DB");

    Ok(())
}

fn get_latest_stamp(conn: &Conn) -> Result<Stamp> {
    schema::stamp::table
        .order(schema::stamp::stamp_id.desc())
        .first(conn)
        .map_err(Into::into)
}

pub fn construct_speculators(
    currency_collection: &CurrencyCollection,
    market_collection: &MarketCollection,
) -> Result<HashMap<MarketId, TradeAggregation>> {
    let rule_setting = env::var("RULE_JSON")?.apply(|path| {
        rule_parse::RuleSetting::from_json(path, currency_collection, market_collection)
    })?;
    let trade_setting = env::var("TRADE_JSON")?.apply(trade_parse::TradeSetting::from_json)?;

    rule_setting
        .into_rules_per_market()
        .map(|(market, weighted_rules)| {
            TradeAggregation::new(market, trade_setting.trade_parameter, weighted_rules)
        })
        .map(|aggregation| (aggregation.market().market_id, aggregation))
        .collect::<HashMap<_, _>>()
        .apply(Ok)
}

pub fn load_market_states(
    conn: &Conn,
    latest_main_stamp: Stamp,
    aggregations: &mut HashMap<MarketId, TradeAggregation>,
) -> Result<()> {
    let required_duration = match aggregations
        .values()
        .flat_map(|a| a.weighted_rules())
        .flat_map(|weighted_rule| weighted_rule.rule().duration_requirement())
        .max()
    {
        Some(d) => d,
        None => return Ok(()),
    };

    // Load necessary timestamps
    let stamps = {
        let rsi_oldest_timestamp = latest_main_stamp.timestamp - required_duration;
        schema::stamp::table
            .filter(schema::stamp::timestamp.ge(rsi_oldest_timestamp))
            .order_by(schema::stamp::timestamp.asc())
            .load::<Stamp>(conn)?
    };
    let oldest_stamp = match stamps.first().cloned() {
        Some(stamp) => stamp,
        None => return Ok(()),
    };

    // Load prices/orderbooks of all markets within timespan
    let price_group = schema::price::table
        .filter(schema::price::stamp_id.ge(oldest_stamp.stamp_id))
        .load::<Price>(conn)?
        .into_iter()
        .map(|p| ((p.market_id, p.stamp_id), p))
        .collect::<HashMap<_, _>>();
    let orderbook_group = schema::orderbook::table
        .filter(schema::orderbook::stamp_id.ge(oldest_stamp.stamp_id))
        .load::<Orderbook>(conn)?
        .apply(|orderbooks| group_by(orderbooks, |o| (o.market_id, o.stamp_id)));

    // Push market states
    for (&market_id, aggregation) in aggregations.iter_mut() {
        for stamp in stamps.iter().cloned() {
            let price = price_group.get(&(market_id, stamp.stamp_id));
            let orderbooks = orderbook_group
                .get(&(market_id, stamp.stamp_id))
                .cloned()
                .unwrap_or_default();

            if let Some(price) = price.cloned() {
                let market_state = MarketState {
                    stamp,
                    price,
                    orderbooks,
                    myorders: vec![], // Omit myorder because it is unnecessary yet
                };
                if let Err(errors) = aggregation.update_market_state(market_state) {
                    for e in errors.into_iter() {
                        warn!("{}", e);
                    }
                }
            }
        }
    }

    Ok(())
}

fn load_latest_sim_balances(
    balance_sim_conn: &Conn,
    currency_collection: &CurrencyCollection,
) -> Result<HashMap<CurrencyId, Balance>> {
    let latest_balance_stamp_id = schema::balance::table
        .select(max(schema::balance::stamp_id))
        .first::<Option<StampId>>(balance_sim_conn)?
        .ok_or(anyhow!("No balance exists in simulation DB"))?;
    currency_collection
        .currencies()
        .into_iter()
        .filter_map(|c| {
            let latest_balance = schema::balance::table
                .filter(schema::balance::currency_id.eq(c.currency_id))
                .filter(schema::balance::stamp_id.eq(latest_balance_stamp_id))
                .first(balance_sim_conn)
                .optional();
            match latest_balance {
                Ok(Some(balance)) => Some(balance),
                Ok(None) => {
                    debug!(
                        "Currency {} is not found in simulation balances. Its balance is assumed 0",
                        c.name
                    );
                    let balance_id = get_sim_next_balance_id(balance_sim_conn);
                    let balance =
                        Balance::new(balance_id, c.currency_id, latest_balance_stamp_id, 0.0, 0.0);
                    Some(balance)
                }
                Err(e) => {
                    warn!("Can't fetch balance of currency {}: {}", c.name, e);
                    None
                }
            }
        })
        .map(|b| (b.currency_id, b))
        .collect::<HashMap<_, _>>()
        .apply(Ok)
}

fn get_sim_next_balance_id(balance_sim_conn: &Conn) -> BalanceId {
    let id = schema::balance::table
        .select(max(schema::balance::balance_id))
        .first::<Option<BalanceId>>(balance_sim_conn)
        .unwrap_or(None)
        .unwrap_or(BalanceId::new(0));
    let next_id = (id.inner() + 1).apply(BalanceId::new);
    next_id
}

fn simulate_trade(conn: &Conn, balance_sim_conn: &Conn, latest_main_stamp: Stamp) -> Result<()> {
    let currency_collection = list_currencies(&conn)?;
    let market_collection = list_markets(&conn)?;

    let mut speculators = construct_speculators(&currency_collection, &market_collection)?;
    load_market_states(conn, latest_main_stamp.clone(), &mut speculators)?;

    let market_setting = env::var("MARKET_JSON")?.apply(market_parse::MarketSetting::from_json)?;
    let fee_ratio = market_setting.fee_ratio;

    let mut current_balances = load_latest_sim_balances(&balance_sim_conn, &currency_collection)?;

    for (_, speculator) in speculators.into_iter() {
        let market = speculator.market();
        let base = match currency_collection.by_id(market.base_id) {
            Some(base) => base,
            None => {
                warn!("Unknown base id");
                continue;
            }
        };
        let quote = match currency_collection.by_id(market.quote_id) {
            Some(quote) => quote,
            None => {
                warn!("Unknown quote id");
                continue;
            }
        };
        let base_balance = match current_balances.get(&market.base_id).cloned() {
            Some(b) => b,
            None => {
                warn!("Currency {} is not found in balances", base.name);
                continue;
            }
        };
        let quote_balance = match current_balances.get(&market.quote_id).cloned() {
            Some(b) => b,
            None => {
                warn!("Currency {} is not found in balances", quote.name);
                continue;
            }
        };

        let recommendation = speculator.recommend();

        for order in recommendation
            .recommend_orders(&base_balance, &quote_balance)
            .iter()
        {
            let base_diff = match order.side {
                OrderSide::Buy => order.base_quantity * (1.0 - fee_ratio) as Amount,
                OrderSide::Sell => -order.base_quantity,
            };
            let quote_diff = match order.side {
                OrderSide::Buy => -order.quote_quantity,
                OrderSide::Sell => order.quote_quantity * (1.0 - fee_ratio) as Amount,
            };

            // Balance must no be negative
            let base_available = base_balance.available;
            if base_available + base_diff < 0.0 {
                warn!(
                    "Too much sell. available: {}, order: {:?}",
                    base_available, order
                );
                continue;
            }
            let quote_available = quote_balance.available;
            if quote_available + quote_diff < 0.0 {
                warn!(
                    "Too much buy. available: {}, order: {:?}",
                    quote_available, order
                );
                continue;
            }

            // Update
            current_balances
                .get_mut(&base.currency_id)
                .unwrap()
                .available += base_diff;
            current_balances
                .get_mut(&quote.currency_id)
                .unwrap()
                .available += quote_diff;

            info!(
                "Market:{}-{} Order:{:?}-{:?} price: {}, base_diff:{}, quote_diff:{}",
                base.symbol,
                quote.symbol,
                order.order_type,
                order.side,
                order.price,
                base_diff,
                quote_diff,
            );
        }

        let recommendation_type = recommendation.recommendation_type();
        match recommendation_type {
            RecommendationType::Buy | RecommendationType::Sell => {
                info!("{:?} reasons:", recommendation_type);
                for r in recommendation.source_recommendations() {
                    info!("{}", r.reason());
                }
            }
            RecommendationType::Pending | RecommendationType::Neutral => {
                debug!("{:?} reasons:", recommendation_type);
                for r in recommendation.source_recommendations() {
                    debug!("{}", r.reason());
                }
            }
        }
    }

    for (
        currency_id,
        Balance {
            available, pending, ..
        },
    ) in current_balances.into_iter()
    {
        // Omit
        if available == 0.0 && pending == 0.0 {
            continue;
        }

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
            warn!("Can't add new balance: {}", e);
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
        .first::<Option<StampId>>(&balance_sim_conn)?;
    let latest_main_stamp = get_latest_stamp(&conn)?;

    match last_sim_stamp_id {
        Some(id) if id == latest_main_stamp.stamp_id => {
            Err(anyhow!("No new timestamp exists in main DB"))
        }
        Some(_) => simulate_trade(&conn, &balance_sim_conn, latest_main_stamp),
        None => sync_balance(&conn, &balance_sim_conn, latest_main_stamp),
    }
}

fn main() {
    dotenv::dotenv().ok();

    env_logger::init();

    info!("Nicehash speculator started at {}", chrono::Local::now());

    if let Err(e) = batch() {
        error!("{}", e);
    }

    info!("Nicehash speculator finished at {}", chrono::Local::now());
}
