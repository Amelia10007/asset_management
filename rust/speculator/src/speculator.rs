use crate::rsi::{Duration, TimespanRsiSequence};
use database::model::*;
use std::iter::once;

#[derive(Debug, Clone, PartialEq)]
pub struct IncompoleteMyOrder {
    pub market_id: IdType,
    pub price: Amount,
    pub base_quantity: Amount,
    pub quote_quantity: Amount,
    pub order_type: OrderType,
    pub side: OrderSide,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderRecommendation {
    Open(IncompoleteMyOrder, RecommendationDescription),
    Cancel(MyOrder, RecommendationDescription),
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
    pub balance: Balance,
    pub price: Price,
    pub orderbooks: Vec<Orderbook>,
    pub myorders: Vec<MyOrder>,
}

impl MarketState {
    pub fn new(
        stamp: Stamp,
        balance: Balance,
        price: Price,
        orderbooks: Vec<Orderbook>,
        myorders: Vec<MyOrder>,
    ) -> Self {
        Self {
            stamp,
            balance,
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

    fn recommend(&self) -> Vec<OrderRecommendation>;

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
    rsi_sequence_15m: TimespanRsiSequence,
    rsi_sequence_1h: TimespanRsiSequence,
    rsi_sequence_4h: TimespanRsiSequence,
}

impl MultipleRsiSpeculator {
    pub fn new(market: Market, rsi_window_size: usize) -> Self {
        Self {
            market,
            market_states: vec![],
            rsi_sequence_15m: TimespanRsiSequence::new(Duration::minutes(15), rsi_window_size),
            rsi_sequence_1h: TimespanRsiSequence::new(Duration::hours(1), rsi_window_size),
            rsi_sequence_4h: TimespanRsiSequence::new(Duration::hours(4), rsi_window_size),
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
        self.rsi_sequence_15m.update_price(timestamp, new_price);
        self.rsi_sequence_1h.update_price(timestamp, new_price);
        self.rsi_sequence_4h.update_price(timestamp, new_price);

        self.market_states.push(new_market_state);
    }

    fn recommend(&self) -> Vec<OrderRecommendation> {
        let rsis = once(&self.rsi_sequence_4h)
            .chain(once(&self.rsi_sequence_1h))
            .chain(once(&self.rsi_sequence_15m));

        match recommend_side_by_rsis(rsis) {
            Some((OrderSide::Buy, reason)) => {
                // Create buy order
                let last_state = self.market_states.last().unwrap();
                let spending = last_state.balance.available * 0.1;
                let price = last_state.price.amount * 1.001;
                let order = IncompoleteMyOrder {
                    market_id: self.market.market_id,
                    price,
                    base_quantity: spending,
                    quote_quantity: spending * price,
                    side: OrderSide::Buy,
                    order_type: OrderType::Limit,
                };

                let recommendation = OrderRecommendation::Open(order, reason);

                vec![recommendation]
            }
            Some((OrderSide::Sell, reason)) => {
                // Create sell order
                let last_state = self.market_states.last().unwrap();
                let spending = last_state.balance.available * 0.1;
                let price = last_state.price.amount * 0.999;
                let order = IncompoleteMyOrder {
                    market_id: self.market.market_id,
                    price,
                    base_quantity: spending,
                    quote_quantity: spending * price,
                    side: OrderSide::Sell,
                    order_type: OrderType::Limit,
                };

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
    rsis: impl IntoIterator<Item = &'a TimespanRsiSequence>,
) -> Option<(OrderSide, RecommendationDescription)> {
    for rsi_sequence in rsis.into_iter() {
        match recommend_side_by_rsi(rsi_sequence) {
            SideRecommendation::Buy(reason) => return Some((OrderSide::Buy, reason)),
            SideRecommendation::Sell(reason) => return Some((OrderSide::Sell, reason)),
            SideRecommendation::Pending => return None,
            SideRecommendation::Undetermined => continue,
        }
    }

    None
}

fn recommend_side_by_rsi(rsi_sequence: &TimespanRsiSequence) -> SideRecommendation {
    let buy_th = 30.0;
    let sell_th = 70.0;

    match rsi_sequence.rsi_sequence_opt() {
        Some(rsis) => {
            let mut iter = rsis.into_iter().rev();
            match (iter.next().flatten(), iter.next().flatten()) {
                (Some(latest), Some(prev)) => {
                    if latest.percent() > buy_th && prev.percent() < buy_th {
                        let description = RecommendationDescription {
                            reason: format!(
                                "Buy. RSI: {} (RSI chunk: {}m)",
                                latest.percent(),
                                rsi_sequence.duration_chunk().num_minutes()
                            ),
                        };
                        SideRecommendation::Buy(description)
                    } else if latest.percent() < sell_th && prev.percent() > sell_th {
                        let description = RecommendationDescription {
                            reason: format!(
                                "Sell. RSI: {} (RSI chunk: {}m)",
                                latest.percent(),
                                rsi_sequence.duration_chunk().num_minutes()
                            ),
                        };
                        SideRecommendation::Sell(description)
                    } else if latest.percent() < buy_th {
                        SideRecommendation::Pending
                    } else if latest.percent() > sell_th {
                        SideRecommendation::Pending
                    } else {
                        SideRecommendation::Undetermined
                    }
                }
                _ => SideRecommendation::Undetermined,
            }
        }
        None => SideRecommendation::Undetermined,
    }
}
