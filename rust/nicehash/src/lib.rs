pub mod api_common;

use api_common::*;
use apply::Apply;
use common::alias::Result;
use database::model::*;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct IncompleteCurrency {
    pub symbol: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct IncompleteBalance {
    pub symbol: String,
    pub pending: Amount,
    pub available: Amount,
}

#[derive(Debug, Clone)]
pub struct IncompleteMarketPrice {
    pub base_symbol: String,
    pub quote_symbol: String,
    pub price: Amount,
}

#[derive(Debug, Clone)]
pub struct IncompleteOrderbook {
    pub order_kind: OrderKind,
    pub price: Amount,
    pub volume: Amount,
}

#[derive(Debug, Clone)]
pub struct IncompleteMyorder {
    pub transaction_id: String,
    pub price: Amount,
    pub base_quantity: Amount,
    pub quote_quantity: Amount,
    pub state: OrderState,
}

pub fn fetch_all_currencies() -> Result<Vec<IncompleteCurrency>> {
    let json = ApiCallBuilder::new()
        .public_api()
        .method(Method::GET)
        .path("/main/api/v2/public/currencies")
        .query_empty()
        .call()?;

    json["currencies"]
        .members()
        .filter_map(|json| {
            let symbol = json["symbol"].as_str();
            let name = json["name"].as_str();
            match (symbol, name) {
                (Some(symbol), Some(name)) => IncompleteCurrency {
                    symbol: symbol.to_string(),
                    name: name.to_string(),
                }
                .apply(Some),
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .apply(Ok)
}

pub fn fetch_all_balances(api_key: ApiKey) -> Result<Vec<IncompleteBalance>> {
    let json = ApiCallBuilder::new()
        .private_api()
        .method(Method::GET)
        .path("/main/api/v2/accounting/accounts2")
        .query_empty()
        .api_key(api_key)
        .call()?;

    json["currencies"]
        .members()
        .filter(|j| j["active"].as_bool() == Some(true))
        .filter_map(|balance_json| {
            let symbol = balance_json["currency"].as_str()?.to_string();
            let available = balance_json["available"]
                .as_str()
                .and_then(|s| Amount::from_str(s).ok())?;
            let pending = balance_json["pending"]
                .as_str()
                .and_then(|s| Amount::from_str(s).ok())?;
            let balance = IncompleteBalance {
                symbol,
                available,
                pending,
            };

            Some(balance)
        })
        .collect::<Vec<_>>()
        .apply(Ok)
}

pub fn fetch_all_market_prices<S: AsRef<str>>(
    known_symbols: &[S],
) -> Result<Vec<IncompleteMarketPrice>> {
    let json = ApiCallBuilder::new()
        .public_api()
        .method(Method::GET)
        .path("/exchange/api/v2/info/prices")
        .query_empty()
        .call()?;

    json.entries()
        .filter_map(|(market, json_price)| {
            let base = known_symbols
                .iter()
                .find(|symbol| market.starts_with(symbol.as_ref()))?
                .as_ref();

            let remaining_market = &market[base.len()..];
            let quote = known_symbols
                .iter()
                .find(|symbol| remaining_market.starts_with(symbol.as_ref()))?
                .as_ref();

            let price = json_price.as_f32()?;

            let market_price = IncompleteMarketPrice {
                base_symbol: base.to_string(),
                quote_symbol: quote.to_string(),
                price,
            };
            Some(market_price)
        })
        .collect::<Vec<_>>()
        .apply(Ok)
}

pub fn fetch_orderbooks_of<SB, SQ>(
    base_symbol: SB,
    quote_symbol: SQ,
    fetch_count: usize,
) -> Result<Vec<IncompleteOrderbook>>
where
    SB: AsRef<str>,
    SQ: AsRef<str>,
{
    let market_symbol = get_market_symbol(base_symbol, quote_symbol);
    let query = vec![
        ("market", market_symbol),
        ("limit", fetch_count.to_string()),
    ];
    let json = ApiCallBuilder::new()
        .public_api()
        .method(Method::GET)
        .path("/exchange/api/v2/info/trades")
        .query(query)
        .call()?;

    json.members()
        .filter_map(|order_json| {
            let order_kind = match order_json["dir"].as_str() {
                Some("BUY") => Some(OrderKind::Buy),
                Some("SELL") => Some(OrderKind::Sell),
                _ => None,
            }?;
            let price = order_json["price"].as_f32()?;
            let volume = order_json["qty"].as_f32()?;

            let orderbook = IncompleteOrderbook {
                order_kind,
                price,
                volume,
            };

            Some(orderbook)
        })
        .collect::<Vec<_>>()
        .apply(Ok)
}

pub fn fetch_myorders<S: AsRef<str>>(
    base_symbol: S,
    quote_symbol: S,
    fetch_count: usize,
    api_key: ApiKey,
) -> Result<Vec<IncompleteMyorder>> {
    let market_symbol = get_market_symbol(base_symbol, quote_symbol);
    let query = vec![
        ("market", market_symbol),
        ("limit", fetch_count.to_string()),
    ];

    let json = ApiCallBuilder::new()
        .private_api()
        .method(Method::GET)
        .path("/exchange/api/v2/info/myOrders")
        .query(query)
        .api_key(api_key)
        .call()?;

    json.members()
        .filter_map(|myorder_json| {
            let transaction_id = myorder_json["orderId"].as_str()?;
            let price = myorder_json["price"].as_f32()?;
            let base_quantity = myorder_json["origQty"].as_f32()?;
            let quote_quantity = myorder_json["origSndQty"].as_f32()?;
            let state = myorder_json["state"].as_str().and_then(get_myorder_state)?;
            let order_kind = match myorder_json["side"].as_str() {
                Some("BUY") => Some(OrderKind::Buy),
                Some("SELL") => Some(OrderKind::Sell),
                _ => None,
            }?;

            let myorder = IncompleteMyorder {
                transaction_id: transaction_id.to_string(),
                price,
                base_quantity,
                quote_quantity,
                state,
            };
            Some(myorder)
        })
        .collect::<Vec<_>>()
        .apply(Ok)
}

pub fn get_market_symbol<SB: AsRef<str>, SQ: AsRef<str>>(
    base_symbol: SB,
    quote_symbol: SQ,
) -> String {
    format!("{}{}", base_symbol.as_ref(), quote_symbol.as_ref())
}

fn get_myorder_state<S: AsRef<str>>(s: S) -> Option<OrderState> {
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
