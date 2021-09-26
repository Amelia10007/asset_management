use super::*;
use crate::indicator::*;
use anyhow::{ensure, Result};
use database::model::*;
use itertools::Itertools;
use ta::{indicators::RelativeStrengthIndex, Period};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RsiCrossParameter {
    candlestick_interval: Duration,
    candlestick_required_count: usize,
    buy: f64,
    sell: f64,
    upper_pending: f64,
    lower_pending: f64,
}

impl RsiCrossParameter {
    pub fn new(
        candlestick_interval: Duration,
        candlestick_required_count: usize,
        buy: f64,
        sell: f64,
        upper_pending: f64,
        lower_pending: f64,
    ) -> Result<Self> {
        ensure!(
            candlestick_interval > Duration::zero(),
            "candlestick_interval must be positive"
        );
        ensure!(
            candlestick_required_count > 0,
            "candlestick_required_count must be positive"
        );
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

#[derive(Debug, Clone)]
pub struct RsiCrossRule {
    market: Market,
    parameter: RsiCrossParameter,
    market_states: Vec<MarketState>,
    rsi_history: IndicatorHistory<RelativeStrengthIndex, f64>,
}

impl RsiCrossRule {
    pub fn new(market: Market, parameter: RsiCrossParameter) -> Self {
        // Parameter holds RsiHistory's constraint by RsiCrossParameter::new(),
        // so no panic occurs
        let indicator = RelativeStrengthIndex::new(parameter.candlestick_required_count).unwrap();
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

impl Rule for RsiCrossRule {
    fn market(&self) -> Market {
        self.market.clone()
    }

    fn duration_requirement(&self) -> Option<Duration> {
        let b = self.rsi_history.indicator_buffer();
        let d = b.interval() * (b.indicator().period() as i32 + 1);
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
            .next(price_stamp)
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

        //
        let (prev, current) = {
            let rsis = self.rsi_history.outputs().collect_vec();

            // Recommend only when candlestick is determined just now.
            // This condition prevents continuous recommendation by launch-by-launch this rule.
            if matches!(rsis.last(), Some(None)) {
                return Box::from(RsiCrossRecommendation::RsiUndetermined(p));
            }

            match rsis
                .into_iter()
                .flat_map(std::convert::identity)
                .copied()
                .tuple_windows()
                .last()
            {
                Some((prev, current)) => (prev, current),
                None => return Box::from(RsiCrossRecommendation::RsiUndetermined(p)),
            }
        };

        let recommendation = match (prev, current) {
            (_, current) if current > p.upper_pending => {
                RsiCrossRecommendation::Pending(current, p)
            }
            (_, current) if current < p.lower_pending => {
                RsiCrossRecommendation::Pending(current, p)
            }
            (prev, current) if prev < p.buy && current >= p.buy => {
                RsiCrossRecommendation::Buy(prev, current, p)
            }
            (prev, current) if prev > p.sell && current <= p.sell => {
                RsiCrossRecommendation::Sell(prev, current, p)
            }
            _ => RsiCrossRecommendation::Neutral(p),
        };

        Box::from(recommendation)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RsiCrossRecommendation {
    Buy(f64, f64, RsiCrossParameter),
    Sell(f64, f64, RsiCrossParameter),
    Pending(f64, RsiCrossParameter),
    Neutral(RsiCrossParameter),
    RsiUndetermined(RsiCrossParameter),
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

        let parameter = match self {
            Buy(_, _, p) | Sell(_, _, p) | Pending(_, p) | Neutral(p) | RsiUndetermined(p) => p,
        };
        let mut header = format!(
            "Rsi({}m {}x): ",
            parameter.candlestick_interval.num_minutes(),
            parameter.candlestick_required_count
        );

        let description = match self {
            Buy(prev, current, _) | Sell(prev, current, _) => {
                format!("{}->{}", prev, current)
            }
            Pending(current, _) => format!("{}", current),
            Neutral(_) => String::from("trigger condition is not satisfied"),
            RsiUndetermined(_) => String::from("undetermined RSI"),
        };

        header.push_str(&description);
        header
    }
}
