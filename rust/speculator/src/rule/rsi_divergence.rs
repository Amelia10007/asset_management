use std::ops::Range;

use super::*;
use crate::indicator::chart::PriceStamp;
use crate::indicator::rsi::{Rsi, RsiHistory};
use database::model::*;
use ordered_float::OrderedFloat;

#[derive(Debug, Clone, PartialEq)]
pub struct RsiDivergenceParameter {
    pub candlestick_interval: Duration,
    pub candlestick_count: usize,
    pub candlestick_maxma_interval: Range<usize>,
    pub upper_divergence_trigger: Rsi,
    pub lower_divergence_trigger: Rsi,
}

impl RsiDivergenceParameter {
    pub fn new(
        candlestick_interval: Duration,
        candlestick_count: usize,
        candlestick_maxma_interval: Range<usize>,
        upper_divergence_trigger: Rsi,
        lower_divergence_trigger: Rsi,
    ) -> Result<Self, BoxErr> {
        if candlestick_interval <= Duration::zero() {
            Err("candlestick_interval must be positive".into())
        } else if candlestick_count <= 0 {
            Err("candlestick_count must be positive".into())
        } else {
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
}

#[derive(Debug, Clone)]
pub struct RsiDivergenceRule {
    market: Market,
    parameter: RsiDivergenceParameter,
    market_states: Vec<MarketState>,
    rsi_history: RsiHistory,
}

impl RsiDivergenceRule {
    pub fn new(market: Market, parameter: RsiDivergenceParameter) -> Self {
        // Parameter holds RsiHistory's constraint by RsiDivergenceParameter::new(),
        // so no panic occurs
        let rsi_history =
            RsiHistory::new(parameter.candlestick_interval, parameter.candlestick_count);
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
        let h = &self.rsi_history;
        let count =
            h.candlestick_required_count() + self.parameter.candlestick_maxma_interval.end + 1;
        let d = h.candlestick_interval() * count as i32;
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
            .update(price_stamp)
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
        if !self.rsi_history.is_candlestick_determined_just_now() {
            return Box::from(RsiDivergenceRecommendation::Neutral);
        }

        let (last_rsi, last_price) = match (
            self.rsi_history.rsis().last(),
            self.rsi_history.candlesticks().last(),
        ) {
            (Some(Some(rsi)), Some(stick)) => (rsi, stick.close().price()),
            _ => return Box::from(RsiDivergenceRecommendation::Neutral),
        };

        let take_count = self.parameter.candlestick_maxma_interval.end
            - self.parameter.candlestick_maxma_interval.start;
        let peak_candidates = self
            .rsi_history
            .rsis()
            .iter()
            .zip(self.rsi_history.candlesticks())
            .skip(self.parameter.candlestick_maxma_interval.start)
            .take(take_count)
            .filter_map(|(rsi, stick)| match rsi {
                Some(rsi) => Some((rsi, stick)),
                None => None,
            });

        let rsi_upper_peak = peak_candidates
            .clone()
            .max_by_key(|(rsi, ..)| OrderedFloat(rsi.rsi().percent()));
        if let Some((peak_rsi, peak_price)) = rsi_upper_peak {
            let peak_price = peak_price.close().price();
            let rsi_cond = self.parameter.upper_divergence_trigger < last_rsi.rsi()
                && last_rsi.rsi() < peak_rsi.rsi();
            let price_cond = last_price > peak_price;
            if rsi_cond && price_cond {
                return Box::from(RsiDivergenceRecommendation::Sell(
                    peak_rsi.rsi(),
                    peak_price,
                    last_rsi.rsi(),
                    last_price,
                ));
            }
        }

        let rsi_lower_peak =
            peak_candidates.min_by_key(|(rsi, ..)| OrderedFloat(rsi.rsi().percent()));
        if let Some((peak_rsi, peak_price)) = rsi_lower_peak {
            let peak_price = peak_price.close().price();
            let rsi_cond = self.parameter.lower_divergence_trigger > last_rsi.rsi()
                && last_rsi.rsi() > peak_rsi.rsi();
            let price_cond = last_price < peak_price;
            if rsi_cond && price_cond {
                return Box::from(RsiDivergenceRecommendation::Buy(
                    peak_rsi.rsi(),
                    peak_price,
                    last_rsi.rsi(),
                    last_price,
                ));
            }
        }

        Box::from(RsiDivergenceRecommendation::Neutral)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RsiDivergenceRecommendation {
    Buy(Rsi, f64, Rsi, f64),
    Sell(Rsi, f64, Rsi, f64),
    Neutral,
}

impl Recommendation for RsiDivergenceRecommendation {
    fn recommendation_type(&self) -> RecommendationType {
        use RsiDivergenceRecommendation::*;

        match self {
            Buy(..) => RecommendationType::Buy,
            Sell(..) => RecommendationType::Sell,
            Neutral => RecommendationType::Neutral,
        }
    }

    fn reason(&self) -> String {
        use RsiDivergenceRecommendation::*;

        match self {
            Buy(prev_rsi, prev_price, cur_rsi, cur_price)
            | Sell(prev_rsi, prev_price, cur_rsi, cur_price) => format!(
                "Rsi: {}->{}, Price: {}->{}",
                prev_rsi.percent(),
                cur_rsi.percent(),
                prev_price,
                cur_price
            )
            .into(),
            Neutral => String::from("trigger condition is not satisfied"),
        }
    }
}
