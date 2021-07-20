use apply::Apply;
use common::alias::Result;
use common::err::OkOpt;
use common::http_query::HttpQuery;
use common::log::prelude::*;
use database::logic::*;
use database::model::*;
use database::schema;
use diesel::prelude::*;
use json::JsonValue;
use once_cell::sync::Lazy;
use speculator::speculator::{MarketState, OrderRecommendation, Speculator};
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

fn call_public_api(api_path: &str, query_collection: &HttpQuery<&str, &str>) -> Result<JsonValue> {
    let url = format!("https://api2.nicehash.com{}", api_path);
    let client = reqwest::blocking::ClientBuilder::default().build()?;

    let req = client
        .request(reqwest::Method::GET, url)
        .query(query_collection.as_slice())
        .build()?;

    // Get reponse
    let res = client.execute(req)?;
    let res = res.text()?;

    let json = json::parse(&res)?;

    Ok(json)
}

fn fetch_server_time() -> Result<NaiveDateTime> {
    let api_path = "/api/v2/time";
    let query = HttpQuery::empty();
    let json = call_public_api(api_path, &query)?;
    let millis = json["serverTime"].as_u64().ok_opt("Invalid serverTime")?;
    let secs = millis / 1000;
    let nsecs = millis % 1000 * 1_000_000;
    let time = NaiveDateTime::from_timestamp(secs as i64, nsecs as u32);
    Ok(time)
}

fn batch() -> Result<()> {
    let url = env::var("DATABASE_URL")?;
    let conn = Conn::establish(&url)?;

    let rsi_window_size = env::var("RSI_WINDOW_SIZE")?.apply(|s| usize::from_str(&s))?;

    let oldest_stamp_in_rsi_window = schema::stamp::table
        .order(schema::stamp::stamp_id.desc())
        .limit(rsi_window_size as i64 * 2)
        .load::<Stamp>(&conn)?
        .last()
        .cloned()
        .ok_opt("No timestamp exists")?;

    let records = schema::price::table
        .inner_join(
            schema::market::table.on(schema::market::market_id.eq(schema::price::market_id)),
        )
        .inner_join(schema::stamp::table.on(schema::price::stamp_id.eq(schema::stamp::stamp_id)))
        .filter(schema::stamp::timestamp.ge(oldest_stamp_in_rsi_window.timestamp))
        .order(schema::stamp::stamp_id)
        .load::<(Price, Market, Stamp)>(&conn)?;

    let mut speculators = HashMap::<IdType, Speculator>::new();

    info!(LOGGER, "Speculation source record count: {}", records.len());

    for (price, market, stamp) in records.into_iter() {
        let speculator = speculators
            .entry(market.market_id)
            .or_insert(Speculator::new(market, rsi_window_size));

        let market_state = MarketState {
            stamp,
            price,
            balance: Balance::new(0, 0, 0, 0., 0.),
            orderbooks: vec![],
            myorders: vec![],
        };

        speculator.update_market_state(market_state);
    }

    let currencies = schema::currency::table.load::<Currency>(&conn)?;

    for speculator in speculators.values() {
        let market = speculator.market();
        let base_currency = currencies
            .iter()
            .find(|c| c.currency_id == market.base_id)
            .ok_opt("Currency does not exist")?;
        let quote_currency = currencies
            .iter()
            .find(|c| c.currency_id == market.quote_id)
            .ok_opt("Currency does not exist")?;
        let market_symbol = format!("{}-{}", base_currency.symbol, quote_currency.symbol);
        let recommendations = speculator.recommend();

        info!(LOGGER, "Recommendation count: {}", recommendations.len());

        for (recommend, reason) in recommendations {
            let mut message = match recommend {
                OrderRecommendation::Open { order_kind, .. } => {
                    format!("Recommend to {:?} in {}", order_kind, market_symbol)
                }
                OrderRecommendation::Cancel(order) => format!(
                    "Recommend to cancel order {} in {}",
                    order.transaction_id, market_symbol
                ),
            };
            message.push(' ');
            message.push_str(&reason);

            notify_recommendation(&message)?;
        }
    }

    Ok(())
}

fn notify_recommendation(s: &str) -> Result<()> {
    warn!(LOGGER, "{}", s);

    Ok(())
}

fn main() {
    dotenv::dotenv().ok();

    let now = fetch_server_time().unwrap();
    warn!(LOGGER, "Nicehash speculator started at {}", now);

    // Load environment variables from file '.env' in currenct dir.
    if let Err(e) = dotenv::dotenv() {
        error!(LOGGER, "{}", e);
    }

    if let Err(e) = batch() {
        error!(LOGGER, "{}", e);
    }

    let now = fetch_server_time().unwrap();
    warn!(LOGGER, "Nicehash speculator finished at {}", now);
}
