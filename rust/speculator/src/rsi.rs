use database::model::NaiveDateTime;
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

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Rsi {
    rsi: f64,
}

impl Rsi {
    pub fn percent(&self) -> f64 {
        self.rsi * 100.0
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

    pub fn rsi_sequence(&self) -> Option<Vec<Rsi>> {
        if self.rsis.len() >= self.window_size {
            self.rsis
                .iter()
                .map(|r| r.map(|rsi| Rsi { rsi }))
                .collect::<Option<_>>()
        } else {
            None
        }
    }

    pub fn rsi_sequence_opt(&self) -> Option<Vec<Option<Rsi>>> {
        if self.rsis.len() >= self.window_size {
            Some(self.rsis.iter().map(|r| r.map(|rsi| Rsi { rsi })).collect())
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
    timespan: Duration,
    rsi_sequence: RsiSequence,
    buffer: Vec<f64>,
    last_rsi_timestamp: Option<NaiveDateTime>,
}

impl TimespanRsiSequence {
    pub fn new(timespan: Duration, window_size: usize) -> Self {
        Self {
            timespan,
            rsi_sequence: RsiSequence::with_window_size(window_size),
            buffer: vec![],
            last_rsi_timestamp: None,
        }
    }

    pub fn timespan(&self) -> Duration {
        self.timespan
    }

    pub fn window_size(&self) -> usize {
        self.rsi_sequence.window_size
    }

    pub fn update_price(&mut self, timestamp: NaiveDateTime, price: f64) {
        self.buffer.push(price);

        let buffer_filled = match self.last_rsi_timestamp.as_ref().copied() {
            Some(last) => (timestamp - last) >= self.timespan,
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

    pub fn rsi_sequence(&self) -> Option<Vec<Rsi>> {
        self.rsi_sequence.rsi_sequence()
    }

    pub fn rsi_sequence_opt(&self) -> Option<Vec<Option<Rsi>>> {
        self.rsi_sequence.rsi_sequence_opt()
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
