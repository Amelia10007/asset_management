use super::*;
use crate::indicator::*;
use anyhow::{ensure, Result};
use database::model::*;
use itertools::Itertools;
use std::ops::Range;
use ta::{indicators::RelativeStrengthIndex, Close, Period};

#[derive(Debug, Clone, PartialEq)]
pub struct RsiDivergenceParameter {
    pub candlestick_interval: Duration,
    pub candlestick_count: usize,
    pub candlestick_maxma_interval: Range<usize>,
    pub upper_divergence_trigger: f64,
    pub lower_divergence_trigger: f64,
}

impl RsiDivergenceParameter {
    pub fn new(
        candlestick_interval: Duration,
        candlestick_count: usize,
        candlestick_maxma_interval: Range<usize>,
        upper_divergence_trigger: f64,
        lower_divergence_trigger: f64,
    ) -> Result<Self> {
        ensure!(
            candlestick_interval > Duration::zero(),
            "candlestick_interval must be positive"
        );
        ensure!(candlestick_count > 0, "candlestick_count must be positive");
        let parameter = Self {
            candlestick_interval,
            candlestick_count,
            candlestick_maxma_interval,
            upper_divergence_trigger,
            lower_divergence_trigger,
        };
        Ok(parameter)
    }
}

#[derive(Debug, Clone)]
pub struct RsiDivergenceRule {
    market: Market,
    parameter: RsiDivergenceParameter,
    market_states: Vec<MarketState>,
    rsi_history: IndicatorHistory<RelativeStrengthIndex, f64>,
}

impl RsiDivergenceRule {
    pub fn new(market: Market, parameter: RsiDivergenceParameter) -> Self {
        // Parameter holds RsiHistory's constraint by RsiDivergenceParameter::new(),
        // so no panic occurs
        let indicator = RelativeStrengthIndex::new(parameter.candlestick_count).unwrap();
        let indicator_buffer = IndicatorBuffer::new(indicator, parameter.candlestick_interval);
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

            let take_count = self.parameter.candlestick_maxma_interval.end
                - self.parameter.candlestick_maxma_interval.start;

            // Take determined candlesticks and its RSI
            history
                .iter()
                .flat_map(std::convert::identity)
                .skip(self.parameter.candlestick_maxma_interval.start)
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
            parameter.candlestick_interval.num_minutes(),
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
