use apply::Apply;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct BollingerBand {
    window_size: usize,
    prices: VecDeque<f64>,
}

impl BollingerBand {
    pub fn with_window_size(window_size: usize) -> Self {
        assert!(window_size > 0);
        Self {
            window_size,
            prices: VecDeque::with_capacity(window_size),
        }
    }

    pub fn current_state(&self) -> Option<BandState> {
        if self.prices.len() < self.window_size {
            None
        } else {
            let len = self.prices.len() as f64;
            let average = self.prices.iter().sum::<f64>() / len;
            let stddev = self
                .prices
                .iter()
                .map(|p| p - average)
                .map(|d| d * d)
                .sum::<f64>()
                .apply(|sum| sum / len)
                .apply(f64::sqrt);

            let state = BandState { average, stddev };
            Some(state)
        }
    }

    pub fn update_price(&mut self, price: f64) -> Option<f64> {
        let popped_price = if self.prices.len() >= self.window_size {
            self.prices.pop_front()
        } else {
            None
        };

        self.prices.push_back(price);

        popped_price
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandState {
    pub average: f64,
    pub stddev: f64,
}

impl BandState {
    pub fn deviation_score(&self, price: f64) -> f64 {
        (price - self.average) / self.stddev
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn test_with_empty_window() {
        let _ = BollingerBand::with_window_size(0);
    }

    #[test]
    fn test_update_price() {
        let mut bb = BollingerBand::with_window_size(4);

        assert_eq!(None, bb.update_price(10.0));
        assert_eq!(None, bb.update_price(20.0));
        assert_eq!(None, bb.update_price(30.0));
        assert_eq!(None, bb.update_price(40.0));
        // Pop old prices
        assert_eq!(Some(10.0), bb.update_price(50.0));
        assert_eq!(Some(20.0), bb.update_price(60.0));
    }

    #[test]
    fn test_currenct_state() {
        let mut bb = BollingerBand::with_window_size(4);

        assert_eq!(None, bb.current_state());

        bb.update_price(10.0);
        bb.update_price(20.0);
        bb.update_price(30.0);

        // Window not filled yet
        assert_eq!(None, bb.current_state());

        // Fill window
        bb.update_price(40.0);

        let state = bb.current_state().unwrap();

        assert_eq!(25.0, state.average);
        assert_eq!(125_f64.sqrt(), state.stddev);
    }

    #[test]
    fn test_deviation_score() {
        let average = 10.0;
        let stddev = 2.0;
        let state = BandState { average, stddev };

        assert_eq!(0.0, state.deviation_score(10.0));
        assert_eq!(1.0, state.deviation_score(12.0));
        assert_eq!(-1.5, state.deviation_score(7.0));
    }
}
