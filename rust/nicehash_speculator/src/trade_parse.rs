use anyhow::{anyhow, Result};
use json::JsonValue;
use speculator::trade::TradeParameter;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

#[derive(Debug)]
pub struct TradeSetting {
    pub trade_parameter: TradeParameter,
}

impl TradeSetting {
    pub fn from_json(path: impl AsRef<Path>) -> Result<Self> {
        let json = {
            let file = File::open(path)?;
            let mut reader = BufReader::new(file);
            let mut s = String::new();
            reader.read_to_string(&mut s)?;
            json::parse(&s)?
        };

        let buy_trigger = parse_param_f64(&json, "buyTrigger")?;
        let sell_trigger = parse_param_f64(&json, "sellTrigger")?;
        let buy_quantity_ratio = parse_param_f64(&json, "buyQuantityRatio")?;
        let sell_quantity_ratio = parse_param_f64(&json, "sellQuantityRatio")?;
        let market_ratio = parse_param_f64(&json, "marketRatio")?;
        let limit_ratio = parse_param_f64(&json, "limitRatio")?;
        let buy_market_allowable_diff_ratio =
            parse_param_f64(&json, "buyMarketAllowableDiffRatio")?;
        let sell_market_allowable_diff_ratio =
            parse_param_f64(&json, "sellMarketAllowableDiffRatio")?;
        let buy_limit_diff_ratio = parse_param_f64(&json, "buyLimitDiffRatio")?;
        let sell_limit_diff_ratio = parse_param_f64(&json, "sellLimitDiffRatio")?;

        let trade_parameter = TradeParameter::new(
            buy_trigger,
            sell_trigger,
            buy_quantity_ratio,
            sell_quantity_ratio,
            market_ratio,
            limit_ratio,
            buy_market_allowable_diff_ratio,
            sell_market_allowable_diff_ratio,
            buy_limit_diff_ratio,
            sell_limit_diff_ratio,
        )?;

        Ok(Self { trade_parameter })
    }
}

fn parse_param_f64(json: &JsonValue, key: &str) -> Result<f64> {
    json[key]
        .as_f64()
        .ok_or(anyhow!("Trade json: invalid {}", key))
        .map_err(Into::into)
}
