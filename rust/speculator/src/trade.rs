use crate::rule::{MarketState, Recommendation, RecommendationType, Rule, RuleError};
use common::alias::BoxErr;
use database::custom_sql_type::{OrderSide, OrderType};
use database::model::{Amount, Balance, Market};
use itertools::Itertools;

#[derive(Debug, Clone, PartialEq)]
pub struct OrderRecommendation {
    pub side: OrderSide,
    pub order_type: OrderType,
    /// Always non-negative
    pub base_quantity: Amount,
    /// Always non-negative
    pub quote_quantity: Amount,
    pub price: Amount,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TradeParameter {
    /// Buy order if weighted average of rules is above this
    buy_trigger: f64,
    /// Sell order if (negative) weighted average of rules is above this
    sell_trigger: f64,
    /// Correction coefficient of buy quantity
    buy_quantity_ratio: f64,
    /// Correction coefficient of sell quantity
    sell_quantity_ratio: f64,
    /// Ratio of market order quantity by whole quantity
    market_ratio: f64,
    /// Ratio of limit order quantity by whole quantity
    limit_ratio: f64,
    buy_market_allowable_diff_ratio: f64,
    sell_market_allowable_diff_ratio: f64,
    buy_limit_diff_ratio: f64,
    sell_limit_diff_ratio: f64,
}

impl TradeParameter {
    /// # Params
    /// 1. `buy_trigger` Buy order if weighted average of rules is above this
    /// 1. `sell_trigger` Sell order if (negative) weighted average of rules is above this
    /// 1. `buy_quantity_ratio` Correction coefficient of buy quantity
    /// 1. `sell_quantity_ratio` Correction coefficient of sell quantity
    /// 1. `market_ratio` Ratio of market order quantity by whole quantity
    /// 1. `limit_ratio` Ratio of limit order quantity by whole quantity
    pub fn new(
        buy_trigger: f64,
        sell_trigger: f64,
        buy_quantity_ratio: f64,
        sell_quantity_ratio: f64,
        market_ratio: f64,
        limit_ratio: f64,
        buy_market_allowable_diff_ratio: f64,
        sell_market_allowable_diff_ratio: f64,
        buy_limit_diff_ratio: f64,
        sell_limit_diff_ratio: f64,
    ) -> Result<Self, BoxErr> {
        if !(0.0..=1.0).contains(&buy_trigger) {
            return Err(BoxErr::from("buy_trigger is out of range"));
        }
        if !(0.0..=1.0).contains(&sell_trigger) {
            return Err(BoxErr::from("sell_trigger is out of range"));
        }
        if !(0.0..=1.0).contains(&buy_quantity_ratio) {
            return Err(BoxErr::from("buy_quantity_ratio is out of range"));
        }
        if !(0.0..=1.0).contains(&sell_quantity_ratio) {
            return Err(BoxErr::from("sell_quantity_ratio is out of range"));
        }
        if !(0.0..=1.0).contains(&market_ratio) {
            return Err(BoxErr::from("market_ratio is out of range"));
        }
        if !(0.0..=1.0).contains(&limit_ratio) {
            return Err(BoxErr::from("limit_ratio is out of range"));
        }

        let p = Self {
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
        };

        Ok(p)
    }

    fn market_limit_ratio(&self) -> (f64, f64) {
        let sum = self.market_ratio + self.limit_ratio;
        (self.market_ratio / sum, self.limit_ratio / sum)
    }
}

pub struct WeightedRule {
    rule: Box<dyn Rule>,
    weight: f64,
}

impl WeightedRule {
    /// # Returns
    /// Returns `Some(rule)` if `weight` is not negative, otherwise `None`
    pub fn new<R: Rule + 'static>(rule: R, weight: f64) -> Option<Self> {
        if weight >= 0.0 {
            let weighted_rule = WeightedRule {
                rule: Box::from(rule),
                weight,
            };
            Some(weighted_rule)
        } else {
            None
        }
    }

    pub fn rule(&self) -> &Box<dyn Rule> {
        &self.rule
    }
}

pub struct TradeAggregation {
    market: Market,
    parameter: TradeParameter,
    weighted_rules: Vec<WeightedRule>,
    last_market_state: Option<MarketState>,
}

impl TradeAggregation {
    pub fn new(
        market: Market,
        parameter: TradeParameter,
        weighted_rules: Vec<WeightedRule>,
    ) -> Self {
        Self {
            market,
            parameter,
            weighted_rules,
            last_market_state: None,
        }
    }

    pub fn market(&self) -> &Market {
        &self.market
    }

    pub fn weighted_rules(&self) -> &[WeightedRule] {
        &self.weighted_rules
    }

    pub fn update_market_state(&mut self, market_state: MarketState) -> Result<(), Vec<RuleError>> {
        let errors = self
            .weighted_rules
            .iter_mut()
            .map(|weighted_rule| &mut weighted_rule.rule)
            .map(|rule| rule.update_market_state(market_state.clone()))
            .filter_map(Result::err)
            .collect_vec();

        self.last_market_state = Some(market_state);

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn recommend(&self) -> AggregatedRecommendation {
        let (mean, recommendations) = {
            let mut weight_sum = 0.0;
            let mut sum = 0.0;
            let mut recommendations = vec![];

            for WeightedRule { rule, weight } in self.weighted_rules.iter() {
                let recommendation = rule.recommend();
                let evaluation = match recommendation.recommendation_type() {
                    RecommendationType::Buy => Some(1.0),
                    RecommendationType::Sell => Some(-1.0),
                    RecommendationType::Pending => Some(0.0),
                    RecommendationType::Neutral => None,
                };

                if let Some(evaluation) = evaluation {
                    weight_sum += weight;
                    sum += evaluation * weight;
                    recommendations.push(recommendation);
                }
            }

            let mean = sum / weight_sum;
            (mean, recommendations)
        };

        let recommendation_type = match mean {
            m if m > self.parameter.buy_trigger => RecommendationType::Buy,
            m if m < -self.parameter.sell_trigger => RecommendationType::Sell,
            _ => RecommendationType::Pending,
        };
        let quantity_ratio = match recommendation_type {
            RecommendationType::Buy => mean.abs() * self.parameter.buy_quantity_ratio,
            RecommendationType::Sell => mean.abs() * self.parameter.sell_quantity_ratio,
            _ => 0.0,
        };

        AggregatedRecommendation {
            parameter: self.parameter,
            recommendation_type,
            quantity_ratio,
            source_recommendations: recommendations,
            last_market_state: self.last_market_state.clone(),
        }
    }
}

pub struct AggregatedRecommendation {
    parameter: TradeParameter,
    recommendation_type: RecommendationType,
    quantity_ratio: f64,
    source_recommendations: Vec<Box<dyn Recommendation>>,
    last_market_state: Option<MarketState>,
}

impl AggregatedRecommendation {
    pub fn recommendation_type(&self) -> RecommendationType {
        self.recommendation_type
    }

    pub fn recommend_orders(
        &self,
        base_balance: &Balance,
        quote_balance: &Balance,
    ) -> Vec<OrderRecommendation> {
        let market_state = match &self.last_market_state {
            Some(state) => state,
            None => return vec![],
        };
        let p = self.parameter;
        let (market_ratio, limit_ratio) = p.market_limit_ratio();
        match self.recommendation_type {
            RecommendationType::Buy => {
                let quote_quantity = quote_balance.available
                    * self.quantity_ratio as Amount
                    * p.buy_quantity_ratio as Amount;
                let market_quantity = quote_quantity * market_ratio as Amount;
                let limit_quantity = quote_quantity * limit_ratio as Amount;
                let market_order = market_buy_order(&self.parameter, market_state, market_quantity);
                let limit_order = limit_buy_order(&self.parameter, market_state, limit_quantity);
                vec![market_order, limit_order]
            }
            RecommendationType::Sell => {
                let base_quantity = base_balance.available
                    * self.quantity_ratio as Amount
                    * p.sell_quantity_ratio as Amount;
                let market_quantity = base_quantity * market_ratio as Amount;
                let limit_quantity = base_quantity * limit_ratio as Amount;
                let market_order =
                    market_sell_order(&self.parameter, market_state, market_quantity);
                let limit_order = limit_sell_order(&self.parameter, market_state, limit_quantity);
                vec![market_order, limit_order]
            }
            RecommendationType::Pending | RecommendationType::Neutral => vec![],
        }
    }

    pub fn source_recommendations(&self) -> &[Box<dyn Recommendation>] {
        &self.source_recommendations
    }
}

fn market_buy_order(
    parameter: &TradeParameter,
    market_state: &MarketState,
    quote_quantity: Amount,
) -> OrderRecommendation {
    let price = market_state.price.amount;
    let base_quantity =
        quote_quantity / price * parameter.buy_market_allowable_diff_ratio as Amount;

    OrderRecommendation {
        side: OrderSide::Buy,
        order_type: OrderType::Market,
        base_quantity,
        quote_quantity,
        price,
    }
}

fn market_sell_order(
    parameter: &TradeParameter,
    market_state: &MarketState,
    base_quantity: Amount,
) -> OrderRecommendation {
    let price = market_state.price.amount;
    let quote_quantity =
        base_quantity * price * parameter.sell_market_allowable_diff_ratio as Amount;

    OrderRecommendation {
        side: OrderSide::Sell,
        order_type: OrderType::Market,
        base_quantity,
        quote_quantity,
        price,
    }
}

fn limit_buy_order(
    parameter: &TradeParameter,
    market_state: &MarketState,
    quote_quantity: Amount,
) -> OrderRecommendation {
    let price = market_state.price.amount * parameter.buy_limit_diff_ratio as Amount;
    let base_quantity = quote_quantity / price;

    OrderRecommendation {
        side: OrderSide::Buy,
        order_type: OrderType::Limit,
        price,
        base_quantity,
        quote_quantity,
    }
}

fn limit_sell_order(
    parameter: &TradeParameter,
    market_state: &MarketState,
    base_quantity: Amount,
) -> OrderRecommendation {
    let price = market_state.price.amount * parameter.sell_limit_diff_ratio as Amount;
    let quote_quantity = base_quantity * price;

    OrderRecommendation {
        side: OrderSide::Sell,
        order_type: OrderType::Limit,
        price,
        base_quantity,
        quote_quantity,
    }
}
