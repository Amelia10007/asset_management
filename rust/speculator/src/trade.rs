use crate::rule::*;
use anyhow::{bail, Result};
use chrono::Duration;
use database::custom_sql_type::{MarketId, OrderSide, OrderType};
use database::model::{Amount, Balance, Market};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use validator::Validate;

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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TradeParameter {
    /// Buy order if weighted average of rules is above this
    #[validate(range(min = 0, max = 1.0))]
    buy_trigger: f64,
    /// Sell order if (negative) weighted average of rules is above this
    #[validate(range(min = 0, max = 1.0))]
    sell_trigger: f64,
    /// Correction coefficient of buy quantity
    #[validate(range(min = 0, max = 1.0))]
    buy_quantity_ratio: f64,
    /// Correction coefficient of sell quantity
    #[validate(range(min = 0, max = 1.0))]
    sell_quantity_ratio: f64,
    /// Ratio of market order quantity by whole quantity
    #[validate(range(min = 0, max = 1.0))]
    market_ratio: f64,
    /// Ratio of limit order quantity by whole quantity
    #[validate(range(min = 0, max = 1.0))]
    limit_ratio: f64,
    buy_market_allowable_diff_ratio: f64,
    sell_market_allowable_diff_ratio: f64,
    buy_limit_diff_ratio: f64,
    sell_limit_diff_ratio: f64,
}

impl TradeParameter {
    fn market_limit_ratio(&self) -> (f64, f64) {
        let sum = self.market_ratio + self.limit_ratio;
        (self.market_ratio / sum, self.limit_ratio / sum)
    }
}

struct WeightedRule {
    rule: Box<dyn Rule>,
    weight: f64,
}

#[derive(Serialize, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
struct RuleComponent {
    rule: Box<dyn RuleParameter>,
    #[validate(range(min = 0))]
    weight: f64,
    #[serde(default)]
    markets: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeAggregationParameter {
    rules: Vec<RuleComponent>,
    #[serde(default)]
    default_markets: Vec<String>,
}

impl TradeAggregationParameter {
    pub fn finalize<F>(
        self,
        trade_parameter: TradeParameter,
        mut f: F,
    ) -> Result<HashMap<MarketId, TradeAggregation>>
    where
        F: FnMut(&str) -> Option<Market>,
    {
        let mut market_map = HashMap::new();
        let mut map = HashMap::new();

        for rule_component in self.rules.into_iter() {
            let market_strs = if rule_component.markets.is_empty() {
                &self.default_markets
            } else {
                &rule_component.markets
            };
            for market_str in market_strs.iter() {
                let market = match f(&market_str) {
                    Some(market) => market,
                    None => bail!("{} is invalid market", market_str),
                };
                market_map.entry(market.market_id).or_insert(market.clone());

                let rule = rule_component.rule.create_rule(market.clone());
                let weight = rule_component.weight;
                let weighted_rule = WeightedRule { rule, weight };
                map.entry(market.market_id)
                    .or_insert(vec![])
                    .push(weighted_rule);
            }
        }

        let mut aggregation_map = HashMap::new();
        for (market_id, weighted_rules) in map.into_iter() {
            let market = market_map[&market_id].clone();
            let aggregation = TradeAggregation::new(market, trade_parameter, weighted_rules);
            let ret = aggregation_map.insert(market_id, aggregation);
            assert!(ret.is_none());
        }

        Ok(aggregation_map)
    }
}

pub struct TradeAggregation {
    market: Market,
    parameter: TradeParameter,
    weighted_rules: Vec<WeightedRule>,
    last_market_state: Option<MarketState>,
}

impl TradeAggregation {
    fn new(market: Market, parameter: TradeParameter, weighted_rules: Vec<WeightedRule>) -> Self {
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

    pub fn duration_requirement(&self) -> Option<Duration> {
        self.weighted_rules
            .iter()
            .flat_map(|weighted_rule| weighted_rule.rule.duration_requirement())
            .max()
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
                }
                recommendations.push(recommendation);
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
