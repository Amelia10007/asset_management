use apply::Apply;
use chrono::NaiveDateTime;
use common::alias::Result;
use common::err::OkOpt;
use common::http_query::HttpQuery;
use common::log::prelude::*;
use database::logic::*;
use database::model::*;
use diesel::prelude::*;
use json::JsonValue;
use once_cell::sync::Lazy;
use std::env;
use std::io::{stdout, Stdout};
use std::str::FromStr;

static LOGGER: Lazy<Logger<Stdout>> = Lazy::new(|| {
    let level = match env::var("SCRAPER_LOGGER_LEVEL")
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

fn call_private_api(api_path: &str, query_collection: &HttpQuery<&str, &str>) -> Result<JsonValue> {
    let organization_id = env::var("NICEHASH_ORGANIZATION_ID")?;
    let api_key = env::var("NICEHASH_API_KEY")?;
    let api_secret_key = env::var("NICEHASH_API_SECRET_KEY")?;
    // Fetch timestamp
    let server_timestamp_millis = fetch_server_time()?.timestamp_millis();

    // Onetime phrase
    let nonce = uuid::Uuid::new_v4();
    let request_id = uuid::Uuid::new_v4();

    //
    let method = "GET";
    let query = query_collection.build_query();

    // Digital signing
    let auth = {
        let input = format!(
            "{}\0{}\0{}\0\0{}\0\0{}\0{}\0{}",
            api_key, server_timestamp_millis, nonce, organization_id, method, api_path, query
        );
        let signature = hmac_sha256::HMAC::mac(input.as_bytes(), api_secret_key.as_bytes())
            .iter()
            .map(|b| format!("{:02x}", b))
            .fold(String::new(), |acc, cur| acc + &cur);
        format!("{}:{}", api_key, signature)
    };

    //
    let url = format!("https://api2.nicehash.com{}", api_path);
    let client = reqwest::blocking::ClientBuilder::default().build()?;

    let req = client
        .request(reqwest::Method::GET, url)
        .header("X-Time", server_timestamp_millis)
        .header("X-Nonce", nonce.to_string())
        .header("X-Organization-Id", organization_id)
        .header("X-Request-Id", request_id.to_string())
        .header("X-Auth", auth)
        .query(query_collection.as_slice())
        .build()?;

    // Get reponse
    let res = client.execute(req)?;
    let res = res.text()?;
    let json = json::parse(&res)?;

    Ok(json)
}

fn fetch_currencies() -> Result<Vec<(String, String)>> {
    if let Ok("0") = env::var("FETCH_CURRENCY_FROM_REMOTE_SERVER").as_deref() {
        return Ok(vec![]);
    }

    let json = call_public_api("/main/api/v2/public/currencies", &HttpQuery::empty())?;

    json["currencies"]
        .members()
        .filter_map(|currency_json| {
            let symbol = currency_json["symbol"].as_str();
            let name = currency_json["name"].as_str();
            if let (Some(symbol), Some(name)) = (symbol, name) {
                Some((symbol.to_string(), name.to_string()))
            } else {
                warn!(
                    LOGGER,
                    "Can't extract currency info. symbol: {:?}, name: {:?}", symbol, &name
                );
                None
            }
        })
        .collect::<Vec<_>>()
        .apply(Ok)
}

fn fetch_balances() -> Result<Vec<(String, Amount, Amount)>> {
    let json = call_private_api("/main/api/v2/accounting/accounts2", &HttpQuery::empty())?;

    json["currencies"]
        .members()
        .filter(|j| j["active"].as_bool() == Some(true))
        .filter_map(|balance_json| {
            let symbol = balance_json["currency"].as_str()?;
            let available = balance_json["available"]
                .as_str()
                .map(Amount::from_str)
                .and_then(|x| x.ok())?;
            let pending = balance_json["pending"]
                .as_str()
                .map(Amount::from_str)
                .and_then(|x| x.ok())?;

            Some((symbol.to_string(), available, pending))
        })
        .collect::<Vec<_>>()
        .apply(Ok)
}

fn fetch_market_prices<S: AsRef<str>>(symbols: &[S]) -> Result<Vec<(String, String, Amount)>> {
    let json = call_public_api("/exchange/api/v2/info/prices", &HttpQuery::empty())?;

    json.entries()
        .filter_map(|(market, json_price)| {
            let base = symbols
                .iter()
                .find(|symbol| market.starts_with(symbol.as_ref()))?
                .as_ref();

            let remaining_market = &market[base.len()..];
            let quote = symbols
                .iter()
                .find(|symbol| remaining_market.starts_with(symbol.as_ref()))?
                .as_ref();

            let price = json_price.as_f32()?;

            Some((base.to_string(), quote.to_string(), price))
        })
        .collect::<Vec<_>>()
        .apply(Ok)
}

fn fetch_orderbooks<S: AsRef<str>>(base: S, quote: S) -> Result<Vec<(OrderKind, Amount, Amount)>> {
    if let Ok("0") = env::var("FETCH_ORDERBOOK_FROM_REMOTE_SERVER").as_deref() {
        return Ok(vec![]);
    }

    let market = format!("{}{}", base.as_ref(), quote.as_ref());
    let limit = env::var("ORDERBOOK_FETCH_LIMIT_PER_MARKET")?;
    let query = [("market", market.as_str()), ("limit", limit.as_str())]
        .iter()
        .copied()
        .collect();
    let json = call_public_api("/exchange/api/v2/info/trades", &query)?;

    json.members()
        .filter_map(|order_json| {
            let kind = match order_json["dir"].as_str() {
                Some("BUY") => Some(OrderKind::Buy),
                Some("SELL") => Some(OrderKind::Sell),
                _ => None,
            }?;
            let price = order_json["price"].as_f32()?;
            let volume = order_json["qty"].as_f32()?;

            Some((kind, price, volume))
        })
        .collect::<Vec<_>>()
        .apply(Ok)
}

fn fetch_myorders<S: AsRef<str>>(
    base: S,
    quote: S,
) -> Result<Vec<(String, Amount, Amount, Amount, String)>> {
    if let Ok("0") = env::var("FETCH_MYORDER_FROM_REMOTE_SERVER").as_deref() {
        return Ok(vec![]);
    }

    let market = format!("{}{}", base.as_ref(), quote.as_ref());
    let limit = env::var("MYORDER_FETCH_LIMIT_PER_MARKET")?;
    let query = [("market", market.as_str()), ("limit", limit.as_str())]
        .iter()
        .copied()
        .collect();
    let json = call_private_api("/exchange/api/v2/info/myOrders", &query)?;

    json.members()
        .filter_map(|myorder_json| {
            let transaction_id = myorder_json["orderId"].as_str()?;
            let price = myorder_json["price"].as_f32()?;
            let base_quantity = myorder_json["origQty"].as_f32()?;
            let quote_quantity = myorder_json["origSndQty"].as_f32()?;
            let state = myorder_json["state"].as_str()?;

            Some((
                transaction_id.to_string(),
                price,
                base_quantity,
                quote_quantity,
                state.to_string(),
            ))
        })
        .collect::<Vec<_>>()
        .apply(Ok)
}

fn connect_db() -> Result<MysqlConnection> {
    let url = env::var("DATABASE_URL")?;
    diesel::mysql::MysqlConnection::establish(&url).map_err(Into::into)
}

fn to_myorder_state<S: AsRef<str>>(s: S) -> Option<OrderState> {
    match s.as_ref() {
        "CREATED" | "PARTIAL" | "RESERVED" | "INSERTED" | "ENTERED" | "RELEASED"
        | "CANCEL_REQUEST" => Some(OrderState::Opened),
        "FULL" => Some(OrderState::Filled),
        "CANCELLED" => Some(OrderState::Cancelled),
        "RESERVATION_ERROR" | "INSERTED_ERROR" | "RELEASED_ERROR" | "PROCESSED_ERROR"
        | "CANCELLED_ERROR" | "REJECTED" => Some(OrderState::Error),
        _ => None,
    }
}

fn main() {
    // Load environment variables from file '.env' in currenct dir.
    dotenv::dotenv().ok();

    let now = fetch_server_time().unwrap();
    warn!(LOGGER, "Nicehash scraper started at {}", now);

    let conn = match connect_db() {
        Ok(conn) => conn,
        Err(e) => {
            error!(LOGGER, "Can't connect database: {}", e);
            return;
        }
    };

    let stamp = match add_stamp(&conn, now) {
        Ok(stamp) => stamp,
        Err(e) => {
            error!(LOGGER, "Can't add timestamp to local DB: {}", e);
            return;
        }
    };

    // Fetch currency info between remote server
    match fetch_currencies() {
        Ok(currencies) => {
            for (symbol, name) in currencies.into_iter() {
                match add_currency(&conn, symbol.clone(), name.clone()) {
                    Ok(_) => info!(LOGGER, "Add currency {}/{}", symbol, name),
                    Err(database::error::Error::Db(e)) => {
                        warn!(LOGGER, "Can't add currency: {}", e)
                    }
                    Err(database::error::Error::Logic(_)) => {}
                }
            }
        }
        Err(e) => warn!(LOGGER, "Cat't fetch currencies: {}", e),
    }

    // Load currencies from local DB
    let currency_collection = match list_currencies(&conn) {
        Ok(cs) => cs,
        Err(e) => {
            error!(LOGGER, "Can't list currencies from database: {}", e);
            return;
        }
    };

    // Fetch balance info from remote server
    match fetch_balances() {
        Ok(balances) => balances
            .into_iter()
            .filter_map(|(symbol, available, pending)| {
                let currency = currency_collection.by_symbol(&symbol)?;
                Some((currency, available, pending))
            })
            .for_each(|(currency, available, pending)| {
                // Add balance info to local DB
                match add_balance(
                    &conn,
                    currency.currency_id,
                    stamp.stamp_id,
                    available,
                    pending,
                ) {
                    Ok(balance) => {
                        info!(
                            LOGGER,
                            "Add balance: {}/{} {}",
                            balance.available,
                            balance.pending,
                            currency.symbol
                        )
                    }
                    Err(e) => warn!(LOGGER, "Can't add balance: {}", e),
                }
            }),
        Err(e) => warn!(LOGGER, "Can't fetch balance: {}", e),
    };

    // Fetch market info from remote server
    let symbols = currency_collection
        .currencies()
        .map(|c| &c.symbol)
        .collect::<Vec<_>>();
    let markets = match fetch_market_prices(&symbols) {
        Ok(markets) => markets,
        Err(e) => {
            error!(LOGGER, "Can't fetch markets: {}", e);
            return;
        }
    };

    let local_markets = match list_markets(&conn) {
        Ok(markets) => markets,
        Err(e) => {
            error!(LOGGER, "Can't load markets from local DB: {}", e);
            return;
        }
    };

    markets
        .into_iter()
        // Add market info to local DB
        .filter_map(|(base, quote, price)| {
            let base = currency_collection.by_symbol(&base)?;
            let quote = currency_collection.by_symbol(&quote)?;

            let market = match local_markets
                .by_base_quote_id(base.currency_id, quote.currency_id)
                .cloned()
            {
                Some(market) => Some(market),
                None => match add_market(&conn, base.currency_id, quote.currency_id) {
                    Ok(market) => Some(market),
                    Err(e) => {
                        warn!(LOGGER, "Can't find or add market: {}", e);
                        None
                    }
                },
            }?;

            Some((market, price))
        })
        // Add price info to local DB
        .for_each(|(market, price)| {
            let market_id = market.market_id;
            match add_price(&conn, market_id, stamp.stamp_id, price) {
                Ok(price) => info!(LOGGER, "Add price: {}/{}", price.market_id, price.amount),
                Err(e) => warn!(LOGGER, "Can't add price: {}", e),
            }
        });

    let markets = match list_markets(&conn) {
        Ok(markets) => markets,
        Err(e) => {
            error!(LOGGER, "Can't load markets from DB: {}", e);
            return;
        }
    };

    for market in markets.markets() {
        let base = currency_collection.by_id(market.base_id).unwrap();
        let quote = currency_collection.by_id(market.quote_id).unwrap();
        // Fetch orderbook from remote server
        let orderbooks = match fetch_orderbooks(&base.symbol, &quote.symbol) {
            Ok(orderbooks) => orderbooks,
            Err(e) => {
                warn!(LOGGER, "Can't fetch orderbook: {}", e);
                continue;
            }
        };
        // Add orderbook info to local DB
        for (kind, price, volume) in orderbooks.into_iter() {
            match add_orderbook(&conn, market.market_id, stamp.stamp_id, kind, price, volume) {
                Ok(orderbook) => info!(LOGGER, "Add orderbook. id: {}", orderbook.orderbook_id),
                Err(e) => warn!(LOGGER, "Can't add orderbook: {}", e),
            }
        }
    }

    for market in markets.markets() {
        let base = currency_collection.by_id(market.base_id).unwrap();
        let quote = currency_collection.by_id(market.quote_id).unwrap();
        // Fetch myorder from remote server
        let myorders = match fetch_myorders(&base.symbol, &quote.symbol) {
            Ok(myorders) => myorders,
            Err(e) => {
                warn!(LOGGER, "Can't fetch myorder: {}", e);
                continue;
            }
        };
        // Add myorder info to local DB
        for (transaction_id, price, base_quantity, quote_quantity, state) in myorders.into_iter() {
            let state = to_myorder_state(state).unwrap_or(OrderState::Error);
            match add_or_update_myorder(
                &conn,
                transaction_id.clone(),
                market.market_id,
                stamp.stamp_id,
                price,
                base_quantity,
                quote_quantity,
                state,
            ) {
                Ok(_) => info!(LOGGER, "Update myorder transaction: {}", transaction_id),
                Err(e) => warn!(LOGGER, "Can't update myorder: {}", e),
            }
        }
    }

    let now = fetch_server_time().unwrap();
    warn!(LOGGER, "Nicehash scraper finished at {}", now);
}
