use super::{Duration, Market, MarketState, Recommendation, RecommendationType, Rule, RuleError};
use database::custom_sql_type::OrderSide;

#[derive(Debug, Clone)]
pub struct FixedRule {
    market: Market,
    side: OrderSide,
}

impl FixedRule {
    pub fn new(market: Market, side: OrderSide) -> Self {
        Self { market, side }
    }
}

impl Rule for FixedRule {
    fn market(&self) -> Market {
        self.market.clone()
    }

    fn duration_requirement(&self) -> Option<Duration> {
        None
    }

    fn update_market_state(&mut self, market_state: MarketState) -> Result<(), RuleError> {
        if self.is_correct_market_state(&market_state) {
            Ok(())
        } else {
            Err(RuleError::MarketConstraint)
        }
    }

    fn recommend(&self) -> Box<dyn Recommendation> {
        Box::from(FixedRuleRecommendation(self.side))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedRuleRecommendation(OrderSide);

impl Recommendation for FixedRuleRecommendation {
    fn recommendation_type(&self) -> RecommendationType {
        match self.0 {
            OrderSide::Buy => RecommendationType::Buy,
            OrderSide::Sell => RecommendationType::Sell,
        }
    }

    fn reason(&self) -> String {
        String::from("Based on fixed trade rule")
    }
}
