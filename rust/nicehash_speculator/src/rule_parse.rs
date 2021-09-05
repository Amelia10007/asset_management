use crate::LOGGER;
use apply::Apply;
use common::alias::Result;
use common::err::OkOpt;
use common::log::prelude::*;
use database::custom_sql_type::{MarketId, OrderSide};
use database::logic::{CurrencyCollection, MarketCollection};
use database::model::Market;
use itertools::Itertools;
use json::JsonValue;
use option_inspect::OptionInspectNone;
use speculator::indicator::rsi::Rsi;
use speculator::rule::fixed::FixedRule;
use speculator::rule::rsi_cross::{RsiCrossParameter, RsiCrossRule};
use speculator::rule::Duration;
use speculator::trade::WeightedRule;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub struct RuleSetting {
    rules: HashMap<MarketId, (Market, Vec<WeightedRule>)>,
}

impl RuleSetting {
    pub fn from_json(
        path: impl AsRef<Path>,
        currency_collection: &CurrencyCollection,
        market_collection: &MarketCollection,
    ) -> Result<Self> {
        let json = {
            let file = File::open(path)?;
            let mut reader = BufReader::new(file);
            let mut s = String::new();
            reader.read_to_string(&mut s)?;
            json::parse(&s)?
        };

        let mut rules = HashMap::new();

        for rule_json in json["rules"].members() {
            let algorithm = rule_json["algorithm"].as_str();
            let parsed_rules = match algorithm {
                Some("fixed") => {
                    parse_fixed_rule(rule_json, currency_collection, market_collection)
                }
                Some("rsiCross") => {
                    parse_rsi_cross_rule(rule_json, currency_collection, market_collection)
                }
                Some(s) => Err(format!("Unknown algorithm: {}", s).into()),
                None => Err("Unspecified algorithm".into()),
            };

            match parsed_rules {
                Ok(weighted_rules) => {
                    for weighted_rule in weighted_rules.into_iter() {
                        let market = weighted_rule.rule().market();
                        rules
                            .entry(market.market_id)
                            .or_insert((market, vec![]))
                            .1
                            .push(weighted_rule);
                    }
                }
                Err(e) => warn!(LOGGER, "{}", e),
            }
        }

        let rule_collection = Self { rules };
        Ok(rule_collection)
    }

    pub fn into_rules_per_market(self) -> impl Iterator<Item = (Market, Vec<WeightedRule>)> {
        self.rules.into_iter().map(|(_, value)| value)
    }
}

fn parse_rsi_cross_rule(
    json: &JsonValue,
    currency_collection: &CurrencyCollection,
    market_collection: &MarketCollection,
) -> Result<Vec<WeightedRule>> {
    let weight = json["weight"].as_f64().ok_opt("weight undefined")?;
    let parameter = {
        let candlestick_timespan = json["candlestickTimespanMin"]
            .as_i64()
            .ok_opt("Invalid candlestickTimespanMin")?
            .apply(Duration::minutes);
        let candlestick_required_count = json["candlestickCount"]
            .as_usize()
            .ok_opt("Invalid candlestickCount")?;
        let buy_trigger = json["buyTrigger"]
            .as_f64()
            .ok_opt("Invalid buyTrigger")?
            .apply(Rsi::from_percent);
        let sell_trigger = json["sellTrigger"]
            .as_f64()
            .ok_opt("Invalid sellTrigger")?
            .apply(Rsi::from_percent);
        let upper_pending_trigger = json["upperPendingTrigger"]
            .as_f64()
            .ok_opt("Invalid upperPendingTrigger")?
            .apply(Rsi::from_percent);
        let lower_pending_trigger = json["lowerPendingTrigger"]
            .as_f64()
            .ok_opt("Invalid lowerPendingTrigger")?
            .apply(Rsi::from_percent);
        RsiCrossParameter::new(
            candlestick_timespan,
            candlestick_required_count,
            buy_trigger,
            sell_trigger,
            upper_pending_trigger,
            lower_pending_trigger,
        )
    };

    parse_markets(&json["pairs"], currency_collection, market_collection)
        .cloned()
        .filter_map(move |market| {
            let rule = RsiCrossRule::new(market, parameter);
            WeightedRule::new(rule, weight)
                .inspect_none(|| warn!(LOGGER, "Invalid rule weight: {}", weight))
        })
        .collect_vec()
        .apply(Ok)
}

fn parse_fixed_rule(
    json: &JsonValue,
    currency_collection: &CurrencyCollection,
    market_collection: &MarketCollection,
) -> Result<Vec<WeightedRule>> {
    let weight = json["weight"].as_f64().ok_opt("weight undefined")?;
    let side = match json["side"].as_str() {
        Some("buy") => OrderSide::Buy,
        Some("sell") => OrderSide::Sell,
        Some(s) => return Err(format!("Undefined order side: {}", s).into()),
        None => return Err("side undefined".into()),
    };

    parse_markets(&json["pairs"], currency_collection, market_collection)
        .cloned()
        .filter_map(move |market| {
            let rule = FixedRule::new(market.clone(), side);
            WeightedRule::new(rule, weight)
                .inspect_none(|| warn!(LOGGER, "Invalid rule weight: {}", weight))
        })
        .collect_vec()
        .apply(Ok)
}

fn parse_markets<'a>(
    json: &'a JsonValue,
    currency_collection: &'a CurrencyCollection,
    market_collection: &'a MarketCollection,
) -> impl Iterator<Item = &'a Market> + 'a {
    json.members()
        .filter_map(|pair_json| pair_json.as_str())
        .filter_map(|str| {
            str.split('-')
                .collect_tuple::<(_, _)>()
                .inspect_none(|| warn!(LOGGER, "Invalid market pair: {}", str))
        })
        .filter_map(move |(base_symbol, quote_symbol)| {
            let base = currency_collection
                .by_symbol(base_symbol)
                .inspect_none(|| warn!(LOGGER, "Unknown currency symbol: {}", base_symbol))?;
            let quote = currency_collection
                .by_symbol(quote_symbol)
                .inspect_none(|| warn!(LOGGER, "Unknown currency symbol: {}", base_symbol))?;
            Some((base, quote))
        })
        .filter_map(move |(base, quote)| {
            market_collection
                .by_base_quote_id(base.currency_id, quote.currency_id)
                .inspect_none(|| {
                    warn!(
                        LOGGER,
                        "{}-{} does not exist in markets", base.symbol, quote.symbol
                    )
                })
        })
}
