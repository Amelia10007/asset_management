pub mod fixed;
pub mod rsi_cross;
pub mod rsi_divergence;

use crate::Duration;
use anyhow::Error;
pub use database::model::*;
use thiserror::Error as ThisError;

/// Market state at a time
#[derive(Debug, Clone)]
pub struct MarketState {
    pub stamp: Stamp,
    pub price: Price,
    pub orderbooks: Vec<Orderbook>,
    pub myorders: Vec<MyOrder>,
}

impl MarketState {
    /// # Panic
    /// Panics if `price` or any of `orderbooks` has different timestamp between `stamp`
    pub fn new(
        stamp: Stamp,
        price: Price,
        orderbooks: Vec<Orderbook>,
        myorders: Vec<MyOrder>,
    ) -> Self {
        assert_eq!(stamp.stamp_id, price.stamp_id);
        assert!(orderbooks.iter().all(|o| o.stamp_id == stamp.stamp_id));

        Self {
            stamp,
            price,
            orderbooks,
            myorders,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecommendationType {
    Buy,
    Sell,
    /// Should not do any trade
    Pending,
    /// Leave determination to other rules
    Neutral,
}

/// Trade recommendation by speculator rule
pub trait Recommendation {
    fn recommendation_type(&self) -> RecommendationType;

    fn reason(&self) -> String;
}

#[typetag::serde(tag = "algorithm")]
pub trait RuleParameter {
    fn create_rule(&self, market: Market) -> Box<dyn Rule>;
}

/// Speculator rule
pub trait Rule {
    /// Return target-market of this rule
    fn market(&self) -> Market;

    /// Return the shortest duration required to generate recommendation
    fn duration_requirement(&self) -> Option<Duration>;

    /// Push newer market state
    /// # Returns
    /// `Ok(())` if succeeds
    ///
    /// `Err(e)` if market/timestamp constraint fails
    fn update_market_state(&mut self, market_state: MarketState) -> Result<(), RuleError>;

    /// Gererate trade recommendation
    fn recommend(&self) -> Box<dyn Recommendation>;

    fn is_correct_market_state(&self, market_state: &MarketState) -> bool {
        let id = self.market().market_id;
        let price_cond = market_state.price.market_id == id;
        let orderbook_cond = market_state.orderbooks.iter().all(|o| o.market_id == id);
        let myorder_cond = market_state.myorders.iter().all(|m| m.market_id == id);

        price_cond && orderbook_cond && myorder_cond
    }
}

#[derive(Debug, ThisError)]
pub enum RuleError {
    #[error("Market constraint failure")]
    MarketConstraint,
    #[error("Timestamp constraint failure")]
    StampConstraint,
    #[error("{0}")]
    Other(Error),
}
