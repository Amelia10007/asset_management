use std::borrow::Cow;

use crate::rsi::{Duration, RsiHistory};
use apply::Apply;
use database::model::*;

#[derive(Debug, Clone, PartialEq)]
pub struct IncompleteMyorder {
    pub market_id: IdType,
    pub price: Amount,
    pub base_quantity: Amount,
    pub quote_quantity: Amount,
    pub order_type: OrderType,
    pub side: OrderSide,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderRecommendation {
    Open(IncompleteMyorder, RecommendationDescription),
    Cancel(MyOrder, RecommendationDescription),
}

impl OrderRecommendation {
    pub fn incomplete_myorder(&self) -> Cow<IncompleteMyorder> {
        match self {
            OrderRecommendation::Open(o, _) => Cow::Borrowed(o),
            OrderRecommendation::Cancel(o, _) => IncompleteMyorder {
                market_id: o.market_id,
                price: o.price,
                base_quantity: o.base_quantity,
                quote_quantity: o.quote_quantity,
                order_type: o.order_type,
                side: o.side,
            }
            .apply(Cow::Owned),
        }
    }

    pub fn description(&self) -> &RecommendationDescription {
        match self {
            OrderRecommendation::Open(_, d) => d,
            OrderRecommendation::Cancel(_, d) => d,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecommendationDescription {
    reason: String,
}

impl RecommendationDescription {
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone)]
pub struct MarketState {
    pub stamp: Stamp,
    pub price: Price,
    pub orderbooks: Vec<Orderbook>,
    pub myorders: Vec<MyOrder>,
}

impl MarketState {
    pub fn new(
        stamp: Stamp,
        price: Price,
        orderbooks: Vec<Orderbook>,
        myorders: Vec<MyOrder>,
    ) -> Self {
        Self {
            stamp,
            price,
            orderbooks,
            myorders,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum SideRecommendation {
    Buy(RecommendationDescription),
    Sell(RecommendationDescription),
    Pending,
    Undetermined,
}

pub trait Speculator {
    fn market(&self) -> Market;

    fn update_market_state(&mut self, new_market_state: MarketState);

    fn recommend(
        &self,
        base_balance: &Balance,
        quote_balance: &Balance,
    ) -> Vec<OrderRecommendation>;

    fn filter_orderbooks(&self, orderbooks: &mut Vec<Orderbook>) {
        orderbooks.retain(|o| o.market_id == self.market().market_id)
    }

    fn filter_myorders(&self, myorders: &mut Vec<MyOrder>) {
        myorders.retain(|m| m.market_id == self.market().market_id)
    }
}

#[derive(Debug, Clone)]
pub struct MultipleRsiSpeculator {
    market: Market,
    market_states: Vec<MarketState>,
    // RSI ordered by timespan descending
    rsi_histories: Vec<RsiHistory>,
    spend_balance_ratio: Amount,
}

impl MultipleRsiSpeculator {
    pub fn new(market: Market, rsi_timespans: Vec<Duration>, spend_balance_ratio: Amount) -> Self {
        let rsi_histories = rsi_timespans
            .into_iter()
            .map(|span| RsiHistory::new(span))
            .collect();

        Self {
            market,
            market_states: vec![],
            rsi_histories,
            spend_balance_ratio,
        }
    }
}

impl Speculator for MultipleRsiSpeculator {
    fn market(&self) -> Market {
        self.market.clone()
    }

    fn update_market_state(&mut self, new_market_state: MarketState) {
        let timestamp = new_market_state.stamp.timestamp;
        let new_price = new_market_state.price.amount as f64;

        for rsi_history in self.rsi_histories.iter_mut() {
            rsi_history.update_price(timestamp, new_price);
        }

        self.market_states.push(new_market_state);
    }

    fn recommend(
        &self,
        base_balance: &Balance,
        quote_balance: &Balance,
    ) -> Vec<OrderRecommendation> {
        match recommend_side_by_rsis(&self.rsi_histories) {
            Some((OrderSide::Buy, reason)) => {
                // Create buy order
                let last_state = self.market_states.last().unwrap();
                let order = make_limit_buy_order(
                    &self.market,
                    last_state,
                    quote_balance,
                    self.spend_balance_ratio,
                );
                let recommendation = OrderRecommendation::Open(order, reason);
                vec![recommendation]
            }
            Some((OrderSide::Sell, reason)) => {
                // Create sell order
                let last_state = self.market_states.last().unwrap();
                let order = make_limit_sell_order(
                    &self.market,
                    last_state,
                    base_balance,
                    self.spend_balance_ratio,
                );
                let recommendation = OrderRecommendation::Open(order, reason);
                vec![recommendation]
            }
            None => {
                vec![]
            }
        }
    }
}

fn recommend_side_by_rsis<'a>(
    rsi_histories: impl IntoIterator<Item = &'a RsiHistory>,
) -> Option<(OrderSide, RecommendationDescription)> {
    for rsi_history in rsi_histories.into_iter() {
        match recommend_side_by_rsi(rsi_history) {
            SideRecommendation::Buy(reason) => return Some((OrderSide::Buy, reason)),
            SideRecommendation::Sell(reason) => return Some((OrderSide::Sell, reason)),
            SideRecommendation::Pending => return None,
            SideRecommendation::Undetermined => continue,
        }
    }

    None
}

fn recommend_side_by_rsi(rsi_history: &RsiHistory) -> SideRecommendation {
    let buy_th = 30.0;
    let sell_th = 70.0;

    let mut rsis = rsi_history.rsis();
    let last = rsis.next_back().copied().flatten().map(|rsi| rsi.percent());
    let last2 = rsis.next_back().copied().flatten().map(|rsi| rsi.percent());

    match (last2, last) {
        (Some(last2), Some(last)) if last2 < buy_th && last >= buy_th => {
            let reason = format!(
                "RSI: {}, Timespan: {}m",
                last,
                rsi_history.timespan().num_minutes()
            );
            let description = RecommendationDescription { reason };
            SideRecommendation::Buy(description)
        }
        (Some(last2), Some(last)) if last2 > sell_th && last <= sell_th => {
            let reason = format!(
                "RSI: {}, Timespan: {}m",
                last,
                rsi_history.timespan().num_minutes()
            );
            let description = RecommendationDescription { reason };
            SideRecommendation::Sell(description)
        }
        (_, Some(last)) if last < buy_th => SideRecommendation::Pending,
        (_, Some(last)) if last > sell_th => SideRecommendation::Pending,
        _ => SideRecommendation::Undetermined,
    }
}

fn make_limit_buy_order(
    market: &Market,
    market_state: &MarketState,
    quote_balance: &Balance,
    spend_balance_ratio: Amount,
) -> IncompleteMyorder {
    let quote_quantity = quote_balance.available * spend_balance_ratio;
    let price = market_state.price.amount * 1.001;
    let base_quantity = quote_quantity / price;

    let order = IncompleteMyorder {
        market_id: market.market_id,
        price,
        base_quantity,
        quote_quantity,
        side: OrderSide::Buy,
        order_type: OrderType::Limit,
    };

    order
}

fn make_limit_sell_order(
    market: &Market,
    market_state: &MarketState,
    base_balance: &Balance,
    spend_balance_ratio: Amount,
) -> IncompleteMyorder {
    let base_quantity = base_balance.available * spend_balance_ratio;
    let price = market_state.price.amount * 0.999;
    let quote_quantity = base_quantity * price;

    let order = IncompleteMyorder {
        market_id: market.market_id,
        price,
        base_quantity,
        quote_quantity,
        side: OrderSide::Sell,
        order_type: OrderType::Limit,
    };

    order
}
