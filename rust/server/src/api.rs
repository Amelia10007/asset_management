use std::collections::HashMap;
use std::str::FromStr;

use crate::exchange_graph::ExchangeGraph;
use anyhow::Result;
use apply::Apply;
use chrono::{Duration, NaiveDateTime};
use database::diesel::QueryDsl;
use database::diesel::*;
use database::logic::Conn;
use database::logic::*;
use database::model::*;
use database::schema;
use itertools::Itertools;
use json::JsonValue;
use qstring::QString;
use rayon::prelude::*;
use std::env;
use std::ops::Deref;
use std::rc::Rc;

pub fn api_balance_history(query: &QString) -> Result<JsonValue> {
    let (price_conn, balance_conn, _) = connect_db(&query)?;

    let timestamps = {
        let since = query
            .get("since")
            .and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ").ok());
        let until = query
            .get("until")
            .and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ").ok());
        let step = query
            .get("step")
            .and_then(|s| parse_query_step(s))
            .unwrap_or(Duration::days(1));

        get_target_timestamps(&price_conn, since, until, step)
    }?;

    let currency_collection = list_currencies(&price_conn)?;

    let fiat_symbol = query.get("fiat");
    let fiat_currency = fiat_symbol
        .as_ref()
        .and_then(|symbol| currency_collection.by_symbol(symbol));

    let timestamp_ids = timestamps
        .iter()
        .map(|stamp| stamp.stamp_id)
        .collect::<Vec<_>>();

    let balance_history = schema::balance::table
        .filter(schema::balance::stamp_id.eq_any(timestamp_ids))
        .load::<Balance>(&*balance_conn)?
        .into_iter()
        .group_by(|b| b.stamp_id)
        .into_iter()
        .map(|(stamp_id, balances)| (stamp_id, balances.collect_vec()))
        .collect::<HashMap<_, _>>();

    let history = match fiat_currency {
        Some(fiat_currency) => {
            let exchange_rate_history = timestamps
                .iter()
                .map(|stamp| construct_exchange_graph(&price_conn, stamp.stamp_id))
                .collect::<Vec<_>>();
            timestamps
                .into_par_iter()
                .zip(exchange_rate_history)
                .map(|(stamp, exchange_rate)| {
                    let balances = balance_history
                        .get(&stamp.stamp_id)
                        .cloned()
                        .unwrap_or(vec![]);
                    let rates = match exchange_rate {
                        Ok(exchange_rate) => balances
                            .iter()
                            .map(|b| {
                                exchange_rate.rate_between(b.currency_id, fiat_currency.currency_id)
                            })
                            .collect_vec(),
                        Err(_) => vec![None; balances.len()],
                    };
                    (stamp, balances, rates)
                })
                .collect::<Vec<_>>()
        }
        None => timestamps
            .into_par_iter()
            .map(|stamp| {
                let balances = balance_history
                    .get(&stamp.stamp_id)
                    .cloned()
                    .unwrap_or(vec![]);
                let rates = vec![None; balances.len()];
                (stamp, balances, rates)
            })
            .collect(),
    };

    let mut json = JsonValue::new_object();
    json["success"] = true.into();
    let mut history_array = JsonValue::new_array();
    for (stamp, balances, rates) in history {
        let mut history = JsonValue::new_object();
        history["stamp"] = stamp.timestamp.format("%Y-%m-%dT%H:%M").to_string().into();
        let mut currencies = JsonValue::new_array();
        for (balance, rate) in balances.into_iter().zip_eq(rates) {
            if let Some(currency) = currency_collection.by_id(balance.currency_id) {
                let mut currency_json = JsonValue::new_object();
                currency_json["name"] = currency.name.as_str().into();
                currency_json["symbol"] = currency.symbol.as_str().into();
                currency_json["available"] = balance.available.into();
                currency_json["pending"] = balance.pending.into();
                if let Some(rate) = rate {
                    currency_json["rate"] = rate.into();
                }
                currencies.push(currency_json).ok();
            }
        }
        history["currencies"] = currencies;
        history_array.push(history).ok();
    }
    json["history"] = history_array;

    Ok(json)
}

/// # Returns
/// `Ok(db_conn, balance_conn)` if successfully connected.
///
/// NOTE: If query specifies using simulation, `balance_conn` refers simulation DB.
fn connect_db(query: &QString) -> Result<(Rc<Conn>, Rc<Conn>, bool)> {
    let use_simulation_balance = matches!(query.get("sim"), Some("1"));

    let price_conn = env::var("DATABASE_URL")?
        .deref()
        .apply(Conn::establish)?
        .apply(Rc::new);
    let balance_conn = if use_simulation_balance {
        env::var("SIM_DATABASE_URL")?
            .deref()
            .apply(Conn::establish)?
            .apply(Rc::new)
    } else {
        price_conn.clone()
    };

    Ok((price_conn, balance_conn, use_simulation_balance))
}

fn parse_query_step(step_str: &str) -> Option<Duration> {
    let mut split = step_str.split('_');
    let num = split.next().and_then(|s| i64::from_str(s).ok())?;
    let unit = split.next()?;

    match unit {
        "day" => Some(Duration::days(num)),
        "hour" => Some(Duration::hours(num)),
        "minute" => Some(Duration::minutes(num)),
        _ => None,
    }
}

fn get_target_timestamps(
    conn: &Conn,
    since: Option<NaiveDateTime>,
    until: Option<NaiveDateTime>,
    step: Duration,
) -> Result<Vec<Stamp>> {
    let timestamps: Vec<Stamp> = match since {
        Some(since) => match until {
            Some(until) => schema::stamp::table
                .filter(schema::stamp::timestamp.between(since, until))
                .order(schema::stamp::timestamp)
                .load(conn)?,
            None => schema::stamp::table
                .filter(schema::stamp::timestamp.ge(since))
                .order(schema::stamp::timestamp)
                .limit(1)
                .load(conn)?,
        },
        None => match until {
            Some(until) => schema::stamp::table
                .filter(schema::stamp::timestamp.le(until))
                .order(schema::stamp::timestamp.desc())
                .limit(1)
                .load(conn)?,
            None => schema::stamp::table
                .order(schema::stamp::timestamp.desc())
                .limit(1)
                .load(conn)?,
        },
    };

    let mut last_valid_stamp: Option<Stamp> = None;
    let filtered_timestamps = timestamps
        .into_iter()
        .filter_map(|current| match last_valid_stamp.as_mut() {
            Some(stamp) => {
                if current.timestamp - stamp.timestamp >= step {
                    *stamp = current.clone();
                    Some(current)
                } else {
                    None
                }
            }
            None => {
                last_valid_stamp = Some(current.clone());
                Some(current)
            }
        })
        .collect();

    Ok(filtered_timestamps)
}

fn construct_exchange_graph(
    conn: &Conn,
    timestamp_id: StampId,
) -> Result<ExchangeGraph<CurrencyId>> {
    use schema::*;

    let prices = price::table
        .inner_join(market::table.on(price::market_id.eq(market::market_id)))
        .filter(price::stamp_id.eq(timestamp_id))
        .load::<(Price, Market)>(conn)?;

    prices
        .into_iter()
        .map(|(p, m)| (m.base_id, m.quote_id, p.amount as f64))
        .apply(ExchangeGraph::from_rates)
        .apply(Ok)
}
