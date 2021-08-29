use std::borrow::Cow;

use crate::chart::PriceStamp;
use crate::rsi::{Duration, RsiHistory};
use apply::Apply;
pub use chrono::{DateTime, Utc};
use database::model::*;
use itertools::Itertools;

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

    fn is_valid_market_state(&self, market_state: &MarketState) -> bool {
        let market_id = self.market().market_id;
        let price_cond = market_state.price.market_id == market_id;
        let orderbooks_cond = market_state
            .orderbooks
            .iter()
            .all(|o| o.market_id == market_id);
        let myorders_cond = market_state
            .myorders
            .iter()
            .all(|o| o.market_id == market_id);

        price_cond && orderbooks_cond && myorders_cond
    }
}

#[derive(Debug, Clone)]
pub struct MultipleRsiSpeculator {
    market: Market,
    market_states: Vec<MarketState>,
    // RSI ordered by timespan descending
    rsi_histories: Vec<RsiHistory<DateTime<Utc>>>,
    spend_buy_ratio: Amount,
    spend_sell_ratio: Amount,
}

impl MultipleRsiSpeculator {
    pub fn new(
        market: Market,
        rsi_timespans: Vec<Duration>,
        rsi_candlestick_count: usize,
        spend_buy_ratio: Amount,
        spend_sell_ratio: Amount,
    ) -> Self {
        let rsi_histories = rsi_timespans
            .into_iter()
            .map(|span| RsiHistory::new(span, rsi_candlestick_count))
            .collect();

        Self {
            market,
            market_states: vec![],
            rsi_histories,
            spend_buy_ratio,
            spend_sell_ratio,
        }
    }
}

impl Speculator for MultipleRsiSpeculator {
    fn market(&self) -> Market {
        self.market.clone()
    }

    fn update_market_state(&mut self, mut new_market_state: MarketState) {
        // Deny other market's data
        assert!(self.is_valid_market_state(&new_market_state));

        // Deny older timestamp data
        if let Some(last_state) = self.market_states.last() {
            assert!(new_market_state.stamp.timestamp > last_state.stamp.timestamp);
        }

        let price_stamp = PriceStamp::new(
            DateTime::from_utc(new_market_state.stamp.timestamp, Utc),
            new_market_state.price.amount as f64,
        );

        for rsi_history in self.rsi_histories.iter_mut() {
            rsi_history.update(price_stamp).ok();
        }

        // Drop needless myorder data for RSI-based speculation
        new_market_state
            .myorders
            .retain(|order| order.state == OrderState::Opened);

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
                let quote_quantity = quote_balance.available * self.spend_buy_ratio / 2.0; // Seperate into limit and market
                let limit_order = limit_buy_order(&self.market, last_state, quote_quantity);
                let market_order = market_buy_order(&self.market, last_state, quote_quantity);
                let opens = std::iter::once(limit_order)
                    .chain(market_order)
                    .map(|order| OrderRecommendation::Open(order, reason.clone()));

                // Cancel sell order
                let closes = last_state
                    .myorders
                    .iter()
                    .filter(|myorder| myorder.side == OrderSide::Sell)
                    .filter(|myorder| myorder.state == OrderState::Opened)
                    .cloned()
                    .map(|myorder| OrderRecommendation::Cancel(myorder, reason.clone()));

                opens.chain(closes).collect()
            }
            Some((OrderSide::Sell, reason)) => {
                // Create sell order
                let last_state = self.market_states.last().unwrap();
                let base_quantity = base_balance.available * self.spend_sell_ratio / 2.0; // Seperate into limit and market
                let limit_order = limit_sell_order(&self.market, last_state, base_quantity);
                let market_order = market_sell_order(&self.market, last_state, base_quantity);
                let opens = std::iter::once(limit_order)
                    .chain(market_order)
                    .map(|order| OrderRecommendation::Open(order, reason.clone()));

                // Cancel buy order
                let closes = last_state
                    .myorders
                    .iter()
                    .filter(|myorder| myorder.side == OrderSide::Buy)
                    .filter(|myorder| myorder.state == OrderState::Opened)
                    .cloned()
                    .map(|myorder| OrderRecommendation::Cancel(myorder, reason.clone()));

                opens.chain(closes).collect()
            }
            None => {
                vec![]
            }
        }
    }
}

fn recommend_side_by_rsis<'a>(
    rsi_histories: impl IntoIterator<Item = &'a RsiHistory<DateTime<Utc>>>,
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

fn recommend_side_by_rsi(rsi_history: &RsiHistory<DateTime<Utc>>) -> SideRecommendation {
    let buy_th = 30.0;
    let sell_th = 70.0;

    let (last2, last) = match rsi_history.rsis().tuple_windows().last() {
        Some((Some(last2), Some(last))) => (last2.rsi(), last.rsi()),
        _ => return SideRecommendation::Undetermined,
    };

    if last2.percent() < buy_th && last.percent() >= buy_th {
        let reason = format!(
            "RSI: {}, Timespan: {}m",
            last.percent(),
            rsi_history.candlestick_span().num_minutes()
        );
        let description = RecommendationDescription { reason };
        SideRecommendation::Buy(description)
    } else if last2.percent() > sell_th && last.percent() <= sell_th {
        let reason = format!(
            "RSI: {}, Timespan: {}m",
            last.percent(),
            rsi_history.candlestick_span().num_minutes()
        );
        let description = RecommendationDescription { reason };
        SideRecommendation::Sell(description)
    } else if last.percent() < buy_th {
        SideRecommendation::Pending
    } else if last.percent() > sell_th {
        SideRecommendation::Pending
    } else {
        SideRecommendation::Undetermined
    }
}

fn market_buy_order(
    market: &Market,
    market_state: &MarketState,
    quote_quantity: Amount,
) -> Option<IncompleteMyorder> {
    let average_price = {
        let sell_books = market_state
            .orderbooks
            .iter()
            .filter(|book| book.side == OrderSide::Sell)
            .filter(|book| !book.price.is_nan())
            .sorted_by(|b1, b2| b1.price.partial_cmp(&b2.price).unwrap());
        let mut baught_quantity = 0.0;
        let mut remaining_quantity = quote_quantity;
        let mut weighted_price_sum = 0.0;
        for Orderbook { price, volume, .. } in sell_books {
            let q = remaining_quantity.min(*volume);
            baught_quantity += q;
            remaining_quantity -= q;
            weighted_price_sum += q * price;
            if q <= Amount::MIN_POSITIVE {
                break;
            }
        }

        weighted_price_sum / baught_quantity
    };

    if average_price / market_state.price.amount < 1.01 {
        let base_quantity = quote_quantity / average_price;
        let order = IncompleteMyorder {
            market_id: market.market_id,
            price: average_price,
            base_quantity,
            quote_quantity,
            side: OrderSide::Buy,
            order_type: OrderType::Market,
        };
        Some(order)
    } else {
        None
    }
}

fn market_sell_order(
    market: &Market,
    market_state: &MarketState,
    base_quantity: Amount,
) -> Option<IncompleteMyorder> {
    let average_price = {
        let sell_books = market_state
            .orderbooks
            .iter()
            .filter(|book| book.side == OrderSide::Buy)
            .filter(|book| !book.price.is_nan())
            .sorted_by(|b1, b2| b1.price.partial_cmp(&b2.price).unwrap())
            .rev();
        let mut sold_quantity = 0.0;
        let mut remaining_quantity = base_quantity;
        let mut weighted_price_sum = 0.0;
        for Orderbook { price, volume, .. } in sell_books {
            let q = remaining_quantity.min(*volume);
            sold_quantity += q;
            remaining_quantity -= q;
            weighted_price_sum += q * price;
            if q <= Amount::MIN_POSITIVE {
                break;
            }
        }

        weighted_price_sum / sold_quantity
    };

    if average_price / market_state.price.amount > 0.99 {
        let quote_quantity = base_quantity * average_price;
        let order = IncompleteMyorder {
            market_id: market.market_id,
            price: average_price,
            base_quantity,
            quote_quantity,
            side: OrderSide::Sell,
            order_type: OrderType::Market,
        };
        Some(order)
    } else {
        None
    }
}

fn limit_buy_order(
    market: &Market,
    market_state: &MarketState,
    quote_quantity: Amount,
) -> IncompleteMyorder {
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

fn limit_sell_order(
    market: &Market,
    market_state: &MarketState,
    base_quantity: Amount,
) -> IncompleteMyorder {
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
