use apply::Apply;
use common::alias::BoxErr;
use common::alias::Result;
use common::log::prelude::*;
use database::logic::*;
use database::model::*;
use diesel::prelude::*;
use nicehash::api_common::ApiKey;
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

fn connect_db() -> Result<MysqlConnection> {
    let url = env::var("DATABASE_URL")?;
    diesel::mysql::MysqlConnection::establish(&url).map_err(Into::into)
}

fn get_orderbook_target_markets_from_env(
    currency_collection: &CurrencyCollection,
    known_markets: &MarketCollection,
) -> Result<Vec<(Currency, Currency, Market)>> {
    let market_symbol_source = env::var("FETCH_ORDERBOOK_TARGET_MARKETS")?;
    parse_market_symbols(&market_symbol_source, currency_collection, known_markets).apply(Ok)
}

fn get_myorder_target_markets_from_env(
    currency_collection: &CurrencyCollection,
    known_markets: &MarketCollection,
) -> Result<Vec<(Currency, Currency, Market)>> {
    let market_symbol_source = env::var("FETCH_MYORDER_TARGET_MARKETS")?;
    parse_market_symbols(&market_symbol_source, currency_collection, known_markets).apply(Ok)
}

fn parse_market_symbols(
    s: &str,
    currency_collection: &CurrencyCollection,
    known_markets: &MarketCollection,
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
            let market = known_markets.by_base_quote_id(base.currency_id, quote.currency_id)?;
            Some((base.clone(), quote.clone(), market.clone()))
        })
        .collect()
}

fn main() {
    // Load environment variables from file '.env' in currenct dir.
    dotenv::dotenv().ok();

    let api_key = {
        let organization_id = env::var("NICEHASH_ORGANIZATION_ID");
        let key = env::var("NICEHASH_API_KEY");
        let secret_key = env::var("NICEHASH_API_SECRET_KEY");

        match (organization_id, key, secret_key) {
            (Ok(id), Ok(key), Ok(skey)) => ApiKey::new(id, key, skey),
            _ => {
                error!(LOGGER, "Can't load api key from environment variable");
                return;
            }
        }
    };

    let now = chrono::Local::now();
    info!(LOGGER, "Nicehash scraper started at {}", now);

    let conn = match connect_db() {
        Ok(conn) => conn,
        Err(e) => {
            error!(LOGGER, "Can't connect database: {}", e);
            return;
        }
    };

    let stamp = match add_stamp(&conn, now.naive_utc()) {
        Ok(stamp) => stamp,
        Err(e) => {
            error!(LOGGER, "Can't add timestamp to local DB: {}", e);
            return;
        }
    };

    // Fetch currency info between remote server
    if let Ok("1") = env::var("FETCH_CURRENCY_FROM_REMOTE_SERVER").as_deref() {
        match nicehash::fetch_all_currencies() {
            Ok(currencies) => {
                for c in currencies.into_iter() {
                    match add_currency(&conn, c.symbol.clone(), c.name.clone()) {
                        Ok(_) => info!(LOGGER, "Add currency {}/{}", c.symbol, c.name),
                        Err(database::error::Error::Logic(
                            database::error::LogicError::DuplicatedCurrency,
                        )) => {}
                        Err(e) => {
                            warn!(LOGGER, "Can't add currency: {}", e)
                        }
                    }
                }
            }
            Err(e) => warn!(LOGGER, "Cat't fetch currencies: {}", e),
        }
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
    if let Ok("1") = env::var("FETCH_BALANCE_FROM_REMOTE_SERVER").as_deref() {
        match nicehash::fetch_all_balances(api_key.clone()) {
            Ok(balances) => balances
                .into_iter()
                .filter_map(|balance| {
                    let currency = currency_collection.by_symbol(&balance.symbol)?;
                    Some((currency.clone(), balance))
                })
                .for_each(|(currency, balance)| {
                    // Add balance info to local DB
                    match add_balance(
                        &conn,
                        currency.currency_id,
                        stamp.stamp_id,
                        balance.available,
                        balance.pending,
                    ) {
                        Ok(balance) => {
                            debug!(
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
        }
    }

    let known_symbols = currency_collection
        .currencies()
        .iter()
        .map(|c| &c.symbol)
        .collect::<Vec<_>>();

    // Fetch market info from remote server
    if let Ok("1") = env::var("FETCH_MARKET_AND_PRICE_FROM_REMOTE_SERVER").as_deref() {
        let known_markets = match list_markets(&conn) {
            Ok(markets) => markets,
            Err(e) => {
                error!(LOGGER, "Cant list markets from DB: {}", e);
                return;
            }
        };
        match nicehash::fetch_all_market_prices(&known_symbols) {
            Ok(market_prices) => market_prices
                .iter()
                .filter_map(|market_price| {
                    let base = currency_collection.by_symbol(&market_price.base_symbol)?;
                    let quote = currency_collection.by_symbol(&market_price.quote_symbol)?;

                    Some((base, quote, market_price.price))
                })
                .for_each(|(base, quote, price)| {
                    // Get market. Add market if necessary
                    let market =
                        match known_markets.by_base_quote_id(base.currency_id, quote.currency_id) {
                            Some(market) => market.clone(),
                            None => match add_market(&conn, base.currency_id, quote.currency_id) {
                                Ok(market) => {
                                    info!(LOGGER, "Add market: {}/{}", base.symbol, quote.symbol);
                                    market
                                }
                                Err(e) => {
                                    warn!(LOGGER, "Can't add currency: {}", e);
                                    return;
                                }
                            },
                        };
                    // Add price
                    match add_price(&conn, market.market_id, stamp.stamp_id, price) {
                        Ok(price) => {
                            debug!(LOGGER, "Add price: {}/{}", price.market_id, price.amount)
                        }
                        Err(e) => warn!(LOGGER, "Can't add price: {}", e),
                    }
                }),
            Err(e) => warn!(LOGGER, "Can't fetch markets and prices: {}", e),
        }
    }

    // List all markets after adding new markets to local DB
    let known_markets = match list_markets(&conn) {
        Ok(markets) => markets,
        Err(e) => {
            error!(LOGGER, "Cant list markets from DB: {}", e);
            return;
        }
    };

    // Add target markets' orderbooks
    match get_orderbook_target_markets_from_env(&currency_collection, &known_markets) {
        Ok(markets) => {
            match env::var("ORDERBOOK_FETCH_COUNT_PER_MARKET")
                .map_err(BoxErr::from)
                .and_then(|s| usize::from_str(&s).map_err(BoxErr::from))
            {
                Ok(0) => {}
                Ok(fetch_count) => {
                    for (base, quote, market) in markets.into_iter() {
                        match nicehash::fetch_orderbooks_of(base.symbol, quote.symbol, fetch_count)
                        {
                            Ok(orderbooks) => {
                                for orderbook in orderbooks.into_iter() {
                                    match add_orderbook(
                                        &conn,
                                        market.market_id,
                                        stamp.stamp_id,
                                        orderbook.side,
                                        orderbook.price,
                                        orderbook.volume,
                                    ) {
                                        Ok(o) => {
                                            debug!(LOGGER, "Add orderbook. id: {}", o.orderbook_id)
                                        }
                                        Err(e) => warn!(LOGGER, "Can't add orderbook: {}", e),
                                    }
                                }
                            }
                            Err(e) => warn!(LOGGER, "Can't fetch orderbook: {}", e),
                        }
                    }
                }

                Err(e) => warn!(LOGGER, "Can't load orderbook-fetch count: {}", e),
            }
        }
        Err(e) => warn!(LOGGER, "Can't list orderbook-fetch target markets: {}", e),
    }

    // Add target markets' my orders
    match get_myorder_target_markets_from_env(&currency_collection, &known_markets) {
        Ok(markets) => {
            match env::var("MYORDER_FETCH_COUNT_PER_MARKET")
                .map_err(BoxErr::from)
                .and_then(|s| usize::from_str(&s).map_err(BoxErr::from))
            {
                Ok(0) => {}
                Ok(fetch_count) => {
                    for (base, quote, market) in markets.into_iter() {
                        match nicehash::fetch_myorders(
                            &base.symbol,
                            &quote.symbol,
                            fetch_count,
                            api_key.clone(),
                        ) {
                            Ok(myorders) => {
                                for myorder in myorders.into_iter() {
                                    match add_or_update_myorder(
                                        &conn,
                                        myorder.transaction_id.clone(),
                                        market.market_id,
                                        stamp.stamp_id,
                                        myorder.price,
                                        myorder.base_quantity,
                                        myorder.quote_quantity,
                                        myorder.order_type,
                                        myorder.side,
                                        myorder.state,
                                    ) {
                                        Ok(_) => debug!(
                                            LOGGER,
                                            "Add or update myorder transaction: {}",
                                            myorder.transaction_id
                                        ),
                                        Err(e) => {
                                            warn!(LOGGER, "Can't add or update myorder: {}", e)
                                        }
                                    }
                                }
                            }
                            Err(e) => warn!(LOGGER, "Can't fetch myorder: {}", e),
                        }
                    }
                }
                Err(e) => warn!(LOGGER, "Can't load myorder-fetch count: {}", e),
            }
        }
        Err(e) => warn!(LOGGER, "Can't list myorder-fetch target markets: {}", e),
    }

    info!(
        LOGGER,
        "Nicehash scraper finished at {}",
        chrono::Local::now()
    );
}
