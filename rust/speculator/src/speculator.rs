use std::usize;

use crate::rsi::RsiSequence;
use database::model::*;

#[derive(Debug, Clone, PartialEq)]
pub enum OrderRecommendation {
    Open {
        order_kind: OrderKind,
        base_quantity: Amount,
        quote_quantity: Amount,
        price: Amount,
    },
    Cancel(MyOrder),
}

#[derive(Debug, Clone)]
pub struct MarketState {
    pub stamp: Stamp,
    pub balance: Balance,
    pub price: Price,
    pub orderbooks: Vec<Orderbook>,
    pub myorders: Vec<MyOrder>,
}

#[derive(Debug)]
pub struct Speculator {
    market: Market,
    last_market_state: Option<MarketState>,
    rsi: RsiSequence,
}

impl Speculator {
    pub fn new(market: Market, trend_window_size: usize) -> Self {
        Self {
            market,
            last_market_state: None,
            rsi: RsiSequence::with_window_size(trend_window_size),
        }
    }

    pub fn market(&self) -> &Market {
        &self.market
    }

    pub fn update_market_state(&mut self, market_state: MarketState) {
        self.rsi.update_price(market_state.price.amount as f64);
        self.last_market_state.replace(market_state);
    }

    pub fn recommend(&self) -> Vec<(OrderRecommendation, String)> {
        match self.rsi.rsi_sequence() {
            Some(seq) => {
                let mut iter = seq.into_iter();
                let (last, last2) = (iter.next_back(), iter.next_back());

                let order = match (last, last2) {
                    (Some(last), Some(last2)) if last > 0.3 && last2 < 0.3 => {
                        // Buy
                        let order = OrderRecommendation::Open {
                            order_kind: OrderKind::Buy,
                            base_quantity: 0.,
                            quote_quantity: 0.,
                            price: 0.,
                        };
                        let reason = format!("Buy. RSI: {}", last,);
                        Some((order, reason))
                    }
                    (Some(last), Some(last2)) if last < 0.7 && last2 > 0.7 => {
                        // Sell
                        let order = OrderRecommendation::Open {
                            order_kind: OrderKind::Sell,
                            base_quantity: 0.,
                            quote_quantity: 0.,
                            price: 0.,
                        };
                        let reason = format!("Sell. RSI: {}", last,);
                        Some((order, reason))
                    }
                    _ => {
                        // Stay
                        None
                    }
                };

                order.into_iter().collect()
            }
            None => vec![],
        }
    }
}
