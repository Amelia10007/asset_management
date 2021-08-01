use crate::exchange_graph::ExchangeGraph;
use crate::HttpQuery;
use apply::{Also, Apply};
use common::alias::Result;
use common::err::OkOpt;
use database::diesel::prelude::*;
use database::diesel::{self, QueryDsl, RunQueryDsl};
use database::logic::{list_currencies, Conn};
use database::model::*;
use database::schema;
use iter_vals::iter_vals;
use std::env;
use std::ops::Deref;
use std::str::FromStr;
use templar::*;

pub fn render_balance_current(query: HttpQuery<'_>) -> Result<Document> {
    let conn = env::var("DATABASE_URL")?.deref().apply(Conn::establish)?;

    let latest_timestamp = load_latest_timestamp(&conn)?;
    let exchange_graph = construct_exchange_graph(&conn, latest_timestamp.stamp_id)?;

    let fiat: Currency = {
        let base_symbol = query.get(&"fiat").unwrap_or(&"USDT");
        schema::currency::table
            .filter(schema::currency::symbol.eq(base_symbol))
            .first(&conn)?
    };

    let balances = schema::balance::table
        .inner_join(schema::currency::table)
        .filter(schema::balance::stamp_id.eq(latest_timestamp.stamp_id))
        .get_results::<(Balance, Currency)>(&conn)?;

    let mut data = Document::default();
    data["title"].set("Autotrader Dashboard");
    data["date"].set(latest_timestamp.timestamp.to_string());
    data["conversion"]["name"].set(&fiat.name);
    data["conversion"]["symbol"].set(&fiat.symbol);

    data["balances"] = Document::Seq(
        balances
            .into_iter()
            .map(|(b, c)| {
                let total_balance = b.available + b.pending;
                let rate = exchange_graph.rate_between(b.currency_id, fiat.currency_id);
                let rate_str = rate.map(|r| r.to_string()).unwrap_or(String::from("???"));
                let conversion_str = rate
                    .map(|r| r * total_balance as f64)
                    .map(|amount| amount.to_string())
                    .unwrap_or(String::from("???"));
                let map = iter_vals![
                    (Document::from("name"), Document::from(&c.name)),
                    (Document::from("available"), Document::from(b.available)),
                    (Document::from("pending"), Document::from(b.pending)),
                    (Document::from("total"), Document::from(total_balance)),
                    (Document::from("symbol"), Document::from(&c.symbol)),
                    (Document::from("rate"), Document::from(rate_str)),
                    (Document::from("conversion"), Document::from(conversion_str))
                ];
                Document::Map(map.collect())
            })
            .collect::<Vec<_>>(),
    );

    Ok(data)
}

pub fn render_balance_current_sim(query: HttpQuery<'_>) -> Result<Document> {
    let conn = env::var("DATABASE_URL")?.deref().apply(Conn::establish)?;
    let sim_conn = env::var("SIM_DATABASE_URL")?
        .deref()
        .apply(Conn::establish)?;

    let latest_timestamp = load_latest_timestamp(&conn)?;
    let exchange_graph = construct_exchange_graph(&conn, latest_timestamp.stamp_id)?;

    let fiat: Currency = {
        let base_symbol = query.get(&"fiat").unwrap_or(&"USDT");
        schema::currency::table
            .filter(schema::currency::symbol.eq(base_symbol))
            .first(&conn)?
    };

    let currency_collection = list_currencies(&conn)?;
    let balances = schema::balance::table
        .filter(schema::balance::stamp_id.eq(latest_timestamp.stamp_id))
        .load::<Balance>(&sim_conn)?
        .into_iter()
        .filter_map(|b| {
            let currency = currency_collection.by_id(b.currency_id)?.clone();
            Some((b, currency))
        })
        .collect::<Vec<_>>();

    let mut data = Document::default();
    data["title"] = "Autotrader Dashboard".into();
    data["date"] = latest_timestamp.timestamp.to_string().into();
    data["conversion"]["name"] = fiat.name.clone().into();
    data["conversion"]["symbol"] = fiat.symbol.clone().into();

    data["balances"] = Document::Seq(
        balances
            .into_iter()
            .map(|(b, c)| {
                let total_balance = b.available + b.pending;
                let rate = exchange_graph.rate_between(b.currency_id, fiat.currency_id);
                let rate_str = rate.map(|r| r.to_string()).unwrap_or(String::from("???"));
                let conversion_str = rate
                    .map(|r| r * total_balance as f64)
                    .map(|amount| amount.to_string())
                    .unwrap_or(String::from("???"));
                let map = iter_vals![
                    (Document::from("name"), Document::from(&c.name)),
                    (Document::from("available"), Document::from(b.available)),
                    (Document::from("pending"), Document::from(b.pending)),
                    (Document::from("total"), Document::from(total_balance)),
                    (Document::from("symbol"), Document::from(&c.symbol)),
                    (Document::from("rate"), Document::from(rate_str)),
                    (Document::from("conversion"), Document::from(conversion_str))
                ];
                Document::Map(map.collect())
            })
            .collect::<Vec<_>>(),
    );

    Ok(data)
}

pub fn render_balance_history_sim(query: HttpQuery<'_>) -> Result<Document> {
    let conn = env::var("DATABASE_URL")?.deref().apply(Conn::establish)?;
    let sim_conn = env::var("SIM_DATABASE_URL")?
        .deref()
        .apply(Conn::establish)?;

    let stamps = {
        let limit = query
            .get(&"limit")
            .and_then(|limit| i64::from_str(limit).ok())
            .unwrap_or(10);

        schema::stamp::table
            .order(schema::stamp::stamp_id.desc())
            .limit(limit)
            .load::<Stamp>(&conn)?
            .also(|v| v.reverse())
    };

    let fiat: Currency = {
        let base_symbol = query.get(&"fiat").unwrap_or(&"USDT");
        schema::currency::table
            .filter(schema::currency::symbol.eq(base_symbol))
            .first(&conn)?
    };

    let total_balance_history = stamps
        .into_iter()
        .filter_map(|stamp| {
            let exchange_graph = construct_exchange_graph(&conn, stamp.stamp_id).ok()?;
            let sim_balances = schema::balance::table
                .filter(schema::balance::stamp_id.eq(stamp.stamp_id))
                .load::<Balance>(&sim_conn)
                .ok()?;

            let sum = sim_balances
                .into_iter()
                .filter_map(|b| {
                    exchange_graph
                        .rate_between(b.currency_id, fiat.currency_id)
                        .map(|rate| rate * (b.available + b.pending) as f64)
                })
                .sum::<f64>();

            Some((stamp, sum))
        })
        .map(|(stamp, sum)| {
            let map = iter_vals![
                (
                    Document::from("stamp"),
                    Document::from(stamp.timestamp.to_string())
                ),
                (Document::from("balance"), Document::from(sum))
            ]
            .collect();
            Document::Map(map)
        })
        .collect::<Vec<_>>();

    let mut data = Document::default();
    data["title"] = "Balance history (simulation)".into();
    data["fiat"] = fiat.symbol.into();
    data["history"] = Document::Seq(total_balance_history);
    Ok(data)
}

fn load_latest_timestamp(conn: &Conn) -> Result<Stamp> {
    let id = schema::stamp::table
        .select(diesel::dsl::max(schema::stamp::stamp_id))
        .get_result::<Option<IdType>>(conn)?
        .ok_opt("No timestamp exists")?;

    let stamp = schema::stamp::table
        .filter(schema::stamp::stamp_id.eq(id))
        .first(conn)?;

    Ok(stamp)
}

fn construct_exchange_graph(conn: &Conn, timestamp_id: IdType) -> Result<ExchangeGraph<IdType>> {
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
