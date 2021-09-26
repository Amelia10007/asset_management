use super::*;
use crate::indicator::*;
use anyhow::Result;
use database::model::*;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::ops::Range;
use ta::{indicators::RelativeStrengthIndex, Close, Period};
use validator::{Validate, ValidationError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RsiDivergenceParameter {
    #[validate(range(min = 1))]
    candlestick_interval_min: i64,
    #[validate(range(min = 1))]
    candlestick_count: usize,
    #[validate(custom = "validate_range")]
    candlestick_maxima_interval: Range<usize>,
    #[validate(range(min = 0, max = 100))]
    upper_divergence_trigger: f64,
    #[validate(range(min = 0, max = 100))]
    lower_divergence_trigger: f64,
}

impl RsiDivergenceParameter {
    fn candlestick_interval(&self) -> Duration {
        Duration::minutes(self.candlestick_interval_min)
    }
}

#[typetag::serde(name = "rsiDivergence")]
impl RuleParameter for RsiDivergenceParameter {
    fn create_rule(&self, market: Market) -> Box<dyn Rule> {
        Box::from(RsiDivergenceRule::new(market, self.clone()))
    }
}

fn validate_range(range: &Range<usize>) -> Result<(), ValidationError> {
    if range.start > 0 && range.start < range.end {
        Ok(())
    } else {
        Err(ValidationError::new("Invalid range"))
    }
}

#[derive(Debug, Clone)]
struct RsiDivergenceRule {
    market: Market,
    parameter: RsiDivergenceParameter,
    market_states: Vec<MarketState>,
    rsi_history: IndicatorHistory<RelativeStrengthIndex, f64>,
}

impl RsiDivergenceRule {
    fn new(market: Market, parameter: RsiDivergenceParameter) -> Self {
        // Parameter holds RsiHistory's constraint by RsiDivergenceParameter::new(),
        // so no panic occurs
        let indicator = RelativeStrengthIndex::new(parameter.candlestick_count).unwrap();
        let indicator_buffer = IndicatorBuffer::new(indicator, parameter.candlestick_interval());
        let rsi_history = IndicatorHistory::new(indicator_buffer);
        Self {
            market,
            parameter,
            market_states: vec![],
            rsi_history,
        }
    }
}

impl Rule for RsiDivergenceRule {
    fn market(&self) -> Market {
        self.market.clone()
    }

    /// Return the shortest duration required to generate recommendation
    fn duration_requirement(&self) -> Option<Duration> {
        let b = self.rsi_history.indicator_buffer();
        let d = b.interval() * (b.indicator().period() as i32 + 1);
        Some(d)
    }

    /// Push newer market state
    /// # Returns
    /// `Ok(())` if succeeds
    ///
    /// `Err(e)` if market/timestamp constraint fails
    fn update_market_state(&mut self, mut market_state: MarketState) -> Result<(), RuleError> {
        if !self.is_correct_market_state(&market_state) {
            return Err(RuleError::MarketConstraint);
        }

        // Deny older timestamp data
        if let Some(last_state) = self.market_states.last() {
            if last_state.stamp.timestamp >= market_state.stamp.timestamp {
                return Err(RuleError::StampConstraint);
            }
        }

        let price_stamp = PriceStamp::new(
            market_state.stamp.timestamp,
            market_state.price.amount as f64,
        );

        self.rsi_history
            .next(price_stamp)
            .map_err(RuleError::Other)?;

        // Drop needless myorder data for RSI-based speculation
        market_state
            .myorders
            .retain(|m| m.state == OrderState::Opened);

        self.market_states.push(market_state);

        Ok(())
    }

    /// Gererate trade recommendation
    fn recommend(&self) -> Box<dyn Recommendation> {
        use RsiDivergenceRecommendation::*;

        let peak_candidates = {
            let history = self.rsi_history.history();

            // Recommend only when candlestick is determined just now.
            // This condition prevents continuous recommendation by launch-by-launch this rule.
            if matches!(history.last(), Some(None)) {
                return Box::from(Neutral(self.parameter.clone()));
            }

            let take_count = self.parameter.candlestick_maxima_interval.end
                - self.parameter.candlestick_maxima_interval.start;

            // Take determined candlesticks and its RSI
            history
                .iter()
                .flat_map(std::convert::identity)
                .skip(self.parameter.candlestick_maxima_interval.start)
                .take(take_count)
                .cloned()
        };

        let (last_price, last_rsi) = match peak_candidates.clone().last() {
            Some((data, rsi)) => (data.close(), rsi),
            None => return Box::from(Neutral(self.parameter.clone())),
        };

        let (rsi_lower_peak, rsi_upper_peak) =
            match peak_candidates.minmax_by_key(|(_, rsi)| *rsi).into_option() {
                Some(opt) => opt,
                None => return Box::from(Neutral(self.parameter.clone())),
            };

        // Check upper peak condition. It can make sell order recommendation
        {
            let upper_peak_price = rsi_upper_peak.0.close();
            let peak_rsi = rsi_upper_peak.1;
            let rsi_cond =
                self.parameter.upper_divergence_trigger < last_rsi && last_rsi < peak_rsi;
            let price_cond = last_price > upper_peak_price;
            if rsi_cond && price_cond {
                return Box::from(Sell(
                    self.parameter.clone(),
                    peak_rsi,
                    upper_peak_price,
                    last_rsi,
                    last_price,
                ));
            }
        }

        // Check lower peak condition. It can make buy order recommendation
        {
            let lower_peak_price = rsi_lower_peak.0.close();
            let peak_rsi = rsi_lower_peak.1;
            let rsi_cond =
                self.parameter.lower_divergence_trigger > last_rsi && last_rsi > peak_rsi;
            let price_cond = last_price < lower_peak_price;
            if rsi_cond && price_cond {
                return Box::from(Buy(
                    self.parameter.clone(),
                    peak_rsi,
                    lower_peak_price,
                    last_rsi,
                    last_price,
                ));
            }
        }

        // No buy/sell signal detected
        Box::from(Neutral(self.parameter.clone()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RsiDivergenceRecommendation {
    /// peak_rsi, peak_price, last_rsi, last_price
    Buy(RsiDivergenceParameter, f64, f64, f64, f64),
    /// peak_rsi, peak_price, last_rsi, last_price
    Sell(RsiDivergenceParameter, f64, f64, f64, f64),
    Neutral(RsiDivergenceParameter),
}

impl Recommendation for RsiDivergenceRecommendation {
    fn recommendation_type(&self) -> RecommendationType {
        use RsiDivergenceRecommendation::*;

        match self {
            Buy(..) => RecommendationType::Buy,
            Sell(..) => RecommendationType::Sell,
            Neutral(..) => RecommendationType::Neutral,
        }
    }

    fn reason(&self) -> String {
        use RsiDivergenceRecommendation::*;

        let parameter = match self {
            Buy(p, ..) | Sell(p, ..) | Neutral(p) => p,
        };
        let mut header = format!(
            "Rsi divergence({}m {}x): ",
            parameter.candlestick_interval().num_minutes(),
            parameter.candlestick_count
        );

        let description = match self {
            Buy(_, prev_rsi, prev_price, cur_rsi, cur_price)
            | Sell(_, prev_rsi, prev_price, cur_rsi, cur_price) => format!(
                "Rsi: {}->{}, Price: {}->{}",
                prev_rsi, cur_rsi, prev_price, cur_price
            )
            .into(),
            Neutral(_) => String::from("trigger condition is not satisfied"),
        };

        header.push_str(&description);
        header
    }
}
