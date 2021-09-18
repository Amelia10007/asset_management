use super::*;
use crate::indicator::chart::PriceStamp;
use crate::indicator::rsi::{Rsi, RsiHistory, RsiStamp};
use database::model::*;
use itertools::Itertools;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RsiCrossParameter {
    candlestick_interval: Duration,
    candlestick_required_count: usize,
    buy: Rsi,
    sell: Rsi,
    upper_pending: Rsi,
    lower_pending: Rsi,
}

impl RsiCrossParameter {
    pub fn new(
        candlestick_interval: Duration,
        candlestick_required_count: usize,
        buy: Rsi,
        sell: Rsi,
        upper_pending: Rsi,
        lower_pending: Rsi,
    ) -> Result<Self, BoxErr> {
        if candlestick_interval <= Duration::zero() {
            Err("candlestick_interval must be positive".into())
        } else if candlestick_required_count <= 0 {
            Err("candlestick_required_count must be positive".into())
        } else {
            let parameter = Self {
                candlestick_interval,
                candlestick_required_count,
                buy,
                sell,
                upper_pending,
                lower_pending,
            };
            Ok(parameter)
        }
    }
}

#[derive(Debug, Clone)]
pub struct RsiCrossRule {
    market: Market,
    parameter: RsiCrossParameter,
    market_states: Vec<MarketState>,
    rsi_history: RsiHistory,
}

impl RsiCrossRule {
    pub fn new(market: Market, parameter: RsiCrossParameter) -> Self {
        // Parameter holds RsiHistory's constraint by RsiCrossParameter::new(),
        // so no panic occurs
        let rsi_history = RsiHistory::new(
            parameter.candlestick_interval,
            parameter.candlestick_required_count,
        );
        Self {
            market,
            parameter,
            market_states: vec![],
            rsi_history,
        }
    }
}

impl Rule for RsiCrossRule {
    fn market(&self) -> Market {
        self.market.clone()
    }

    fn duration_requirement(&self) -> Option<Duration> {
        let h = &self.rsi_history;
        let d = h.candlestick_interval() * (h.candlestick_required_count() as i32 + 1);
        Some(d)
    }

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

    fn recommend(&self) -> Box<dyn Recommendation> {
        let p = self.parameter;

        // Recommend only when 2 or more candlesticks have been determined
        let (prev, current) = match self.rsi_history.rsis().iter().tuple_windows().last() {
            Some((prev, current)) => (
                prev.as_ref().map(RsiStamp::rsi),
                current.as_ref().map(RsiStamp::rsi),
            ),
            None => return Box::from(RsiCrossRecommendation::RsiUndetermined(p)),
        };

        // Recommend only when candlestick is determined just now.
        // This condition prevents continuous recommendation by launch-by-launch this rule.
        if !self.rsi_history.is_candlestick_determined_just_now() {
            return Box::from(RsiCrossRecommendation::RsiUndetermined(p));
        }

        let recommendation = match (prev, current) {
            (Some(prev), Some(current)) if prev < p.buy && current >= p.buy => {
                RsiCrossRecommendation::Buy(prev, current, p)
            }
            (Some(prev), Some(current)) if prev > p.sell && current <= p.sell => {
                RsiCrossRecommendation::Sell(prev, current, p)
            }
            (_, Some(current)) if current > p.upper_pending => {
                RsiCrossRecommendation::Pending(current, p)
            }
            (_, Some(current)) if current < p.lower_pending => {
                RsiCrossRecommendation::Pending(current, p)
            }
            _ => RsiCrossRecommendation::Neutral(p),
        };

        Box::from(recommendation)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RsiCrossRecommendation {
    Buy(Rsi, Rsi, RsiCrossParameter),
    Sell(Rsi, Rsi, RsiCrossParameter),
    Pending(Rsi, RsiCrossParameter),
    Neutral(RsiCrossParameter),
    RsiUndetermined(RsiCrossParameter),
}

impl RsiCrossRecommendation {
    fn reason_header(&self) -> String {
        use RsiCrossRecommendation::*;

        let parameter = match self {
            Buy(_, _, p) | Sell(_, _, p) | Pending(_, p) | Neutral(p) | RsiUndetermined(p) => p,
        };

        format!(
            "Rsi({}m {}x): ",
            parameter.candlestick_interval.num_minutes(),
            parameter.candlestick_required_count
        )
    }
}

impl Recommendation for RsiCrossRecommendation {
    fn recommendation_type(&self) -> RecommendationType {
        use RsiCrossRecommendation::*;

        match self {
            Buy(..) => RecommendationType::Buy,
            Sell(..) => RecommendationType::Sell,
            Pending(..) => RecommendationType::Pending,
            Neutral(..) | RsiUndetermined(..) => RecommendationType::Neutral,
        }
    }

    fn reason(&self) -> String {
        use RsiCrossRecommendation::*;

        let mut header = self.reason_header();

        let description = match self {
            Buy(prev, current, _) | Sell(prev, current, _) => {
                format!("{}->{}", prev.percent(), current.percent()).into()
            }
            Pending(current, _) => format!("{}", current.percent()).into(),
            Neutral(_) => String::from("trigger condition is not satisfied"),
            RsiUndetermined(_) => String::from("undetermined RSI"),
        };

        header.push_str(&description);
        header
    }
}
