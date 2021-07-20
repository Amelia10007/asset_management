use database::model::NaiveDateTime;
use itertools::Itertools;
use std::collections::VecDeque;

pub type Duration = <NaiveDateTime as std::ops::Sub>::Output;

#[derive(Debug, Clone, Copy, PartialEq)]
enum PriceChange {
    Increase(f64),
    Decrease(f64),
}

impl PriceChange {
    fn from_change(change: f64) -> Self {
        if change > 0.0 {
            PriceChange::Increase(change)
        } else {
            PriceChange::Decrease(-change)
        }
    }
}

#[derive(Debug, Clone)]
pub struct RsiSequence {
    window_size: usize,
    prices: VecDeque<f64>,
    changes: VecDeque<Option<PriceChange>>,
    rsis: VecDeque<Option<f64>>,
}

impl RsiSequence {
    pub fn with_window_size(window_size: usize) -> Self {
        assert!(window_size > 0);

        Self {
            window_size,
            prices: VecDeque::with_capacity(window_size),
            changes: VecDeque::with_capacity(window_size),
            rsis: VecDeque::with_capacity(window_size),
        }
    }

    pub fn prices(&self) -> Option<impl Iterator<Item = f64> + '_> {
        if self.rsis.len() >= self.window_size {
            Some(self.prices.iter().copied())
        } else {
            None
        }
    }

    pub fn rsi_sequence(&self) -> Option<Vec<f64>> {
        if self.rsis.len() >= self.window_size {
            self.rsis.iter().copied().collect::<Option<Vec<f64>>>()
        } else {
            None
        }
    }

    pub fn update_price(&mut self, price: f64) {
        // Drop old price
        if self.prices.len() >= self.window_size {
            self.prices.pop_front();
            self.changes.pop_front();
            self.rsis.pop_front();
        }

        let diff = self.prices.back().map(|prev| price - prev);

        match diff {
            Some(diff) => self.changes.push_back(Some(PriceChange::from_change(diff))),
            None => self.changes.push_back(None),
        }

        self.prices.push_back(price);
        self.rsis.push_back(self.currenct_rsi())
    }

    fn currenct_rsi(&self) -> Option<f64> {
        if self.prices.len() >= self.window_size {
            let increase_sum = self
                .changes
                .iter()
                .filter_map(|c| {
                    if let Some(PriceChange::Increase(diff)) = c {
                        Some(diff)
                    } else {
                        None
                    }
                })
                .sum::<f64>();
            let decrease_sum = self
                .changes
                .iter()
                .filter_map(|c| {
                    if let Some(PriceChange::Decrease(diff)) = c {
                        Some(diff)
                    } else {
                        None
                    }
                })
                .sum::<f64>();

            let rsi = increase_sum / (increase_sum + decrease_sum);

            Some(rsi)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct TimespanRsiSequence {
    duration_chunk: Duration,
    rsi_sequence: RsiSequence,
    buffer: Vec<f64>,
    last_rsi_timestamp: Option<NaiveDateTime>,
}

impl TimespanRsiSequence {
    pub fn new(duration_chunk: Duration, window_size: usize) -> Self {
        Self {
            duration_chunk,
            rsi_sequence: RsiSequence::with_window_size(window_size),
            buffer: vec![],
            last_rsi_timestamp: None,
        }
    }

    pub fn duration_chunk(&self) -> Duration {
        self.duration_chunk
    }

    pub fn window_size(&self) -> usize {
        self.rsi_sequence.window_size
    }

    pub fn update_price(&mut self, timestamp: NaiveDateTime, price: f64) {
        self.buffer.push(price);

        let buffer_filled = match self.last_rsi_timestamp.as_ref().copied() {
            Some(last) => (timestamp - last) >= self.duration_chunk,
            None => {
                self.last_rsi_timestamp = Some(timestamp);
                false
            }
        };

        if buffer_filled {
            let len = self.buffer.len() as f64;
            let average_price = self.buffer.drain(..).sum::<f64>() / len;
            self.rsi_sequence.update_price(average_price);
            self.last_rsi_timestamp = Some(timestamp);
        }
    }

    pub fn rsi_sequence(&self) -> Option<Vec<f64>> {
        self.rsi_sequence.rsi_sequence()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsi_inner() {
        use PriceChange::*;

        let mut rsi_sequence = RsiSequence::with_window_size(5);

        rsi_sequence.update_price(10.0);
        rsi_sequence.update_price(30.0); // +20
        rsi_sequence.update_price(20.0); // -10
        rsi_sequence.update_price(40.0); // +20
        rsi_sequence.update_price(20.0); // -20

        let mut change_iter = rsi_sequence.changes.iter().copied();
        assert_eq!(Some(None), change_iter.next());
        assert_eq!(Some(Some(Increase(20.))), change_iter.next());
        assert_eq!(Some(Some(Decrease(10.))), change_iter.next());
        assert_eq!(Some(Some(Increase(20.))), change_iter.next());
        assert_eq!(Some(Some(Decrease(20.))), change_iter.next());
        assert_eq!(None, change_iter.next());
    }

    #[test]
    fn test_current_rsi() {
        let mut rsi_sequence = RsiSequence::with_window_size(5);

        rsi_sequence.update_price(10.0);
        rsi_sequence.update_price(30.0); // +20
        rsi_sequence.update_price(20.0); // -10
        rsi_sequence.update_price(40.0); // +20
        rsi_sequence.update_price(20.0); // -20

        let rsi = (20. + 20.) / (20. + 10. + 20. + 20.);
        assert_eq!(Some(rsi), rsi_sequence.currenct_rsi());

        // Update
        rsi_sequence.update_price(10.); // -10
        let rsi = (20. + 20.) / (20. + 10. + 20. + 20. + 10.);
        assert_eq!(Some(rsi), rsi_sequence.currenct_rsi());
    }
}

/// The Relative Strenth Index
#[derive(Debug, Clone)]
pub struct Rsi {
    window_size: usize,
    prices: VecDeque<f64>,
}

impl Rsi {
    pub fn with_window_size(window_size: usize) -> Self {
        assert!(window_size > 0);

        Self {
            window_size,
            prices: VecDeque::with_capacity(window_size),
        }
    }

    pub fn rsi_percent(&self) -> Option<f64> {
        if self.prices.len() < self.window_size {
            None
        } else {
            let (increase, decrease) = self.prices.iter().tuple_windows().fold(
                (0., 0.),
                |(acc_inc, acc_dec), (prev, next)| {
                    let diff = next - prev;
                    if diff > 0. {
                        (acc_inc + diff, acc_dec)
                    } else {
                        (acc_inc, acc_dec - diff)
                    }
                },
            );

            let index = increase / (increase + decrease);
            Some(index)
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
