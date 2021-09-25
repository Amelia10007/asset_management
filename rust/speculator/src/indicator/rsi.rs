use super::chart::{Candlestick, CandlestickHistory, IndicatorUpdate, PriceStamp};
use crate::Duration;
use crate::Timestamp;
use anyhow::Result;
use apply::Apply;
use itertools::Itertools;

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
    pub fn from_percent(percent: f64) -> Self {
        Self {
            rsi: percent / 100.0,
        }
    }

    pub fn percent(&self) -> f64 {
        self.rsi * 100.0
    }

    fn from_changes(changes: impl IntoIterator<Item = PriceChange>) -> Option<Self> {
        let (increase, decrease) =
            changes
                .into_iter()
                .fold((0.0, 0.0), |(acc_inc, acc_dec), change| match change {
                    PriceChange::Increase(c) => (acc_inc + c, acc_dec),
                    PriceChange::Decrease(c) => (acc_inc, acc_dec + c),
                });

        let rsi = increase / (increase + decrease);
        if rsi.is_finite() {
            Some(Rsi { rsi })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RsiStamp {
    open: Timestamp,
    close: Timestamp,
    rsi: Rsi,
}

impl RsiStamp {
    pub fn new(open: Timestamp, close: Timestamp, rsi: Rsi) -> Self {
        Self { open, close, rsi }
    }

    pub fn open(&self) -> Timestamp {
        self.open
    }

    pub fn close(&self) -> Timestamp {
        self.close
    }

    pub fn rsi(&self) -> Rsi {
        self.rsi
    }
}

#[derive(Debug, Clone)]
pub struct RsiHistory {
    candlestick_required_count: usize,
    candlestick_history: CandlestickHistory,
    rsis: Vec<Option<RsiStamp>>,
}

impl RsiHistory {
    /// # Panics
    /// 1. Panics if `candlestick_required_count` is 0
    /// 1. Panics under negative `interval`
    pub fn new(candlestick_interval: Duration, candlestick_required_count: usize) -> Self {
        assert!(candlestick_required_count > 0);

        let candlestick_history = CandlestickHistory::new(candlestick_interval);
        Self {
            candlestick_required_count,
            candlestick_history,
            rsis: vec![],
        }
    }

    pub fn candlestick_interval(&self) -> Duration {
        self.candlestick_history.interval()
    }

    pub fn candlestick_required_count(&self) -> usize {
        self.candlestick_required_count
    }

    pub fn is_candlestick_determined_just_now(&self) -> bool {
        self.candlestick_history
            .is_candlestick_determined_just_now()
    }

    pub fn candlesticks(&self) -> &[Candlestick] {
        self.candlestick_history.candlesticks()
    }

    pub fn rsis(&self) -> &[Option<RsiStamp>] {
        &self.rsis
    }

    pub fn update(&mut self, price_stamp: PriceStamp) -> Result<()> {
        if let IndicatorUpdate::Determined(..) = self.candlestick_history.update(price_stamp)? {
            let rsi = self.calculate_rsi();
            self.rsis.push(rsi);
        }
        Ok(())
    }

    fn calculate_rsi(&self) -> Option<RsiStamp> {
        let len = self.candlesticks().len();
        // Requires sufficient number of sticks to calculate rsi properly
        if len < self.candlestick_required_count {
            return None;
        }

        let target_sticks = &self.candlesticks()[len - self.candlestick_required_count..];

        let open = target_sticks[0].open().stamp();
        let close = target_sticks.last().unwrap().close().stamp();

        // Take last n sticks
        let rsi = target_sticks
            .iter()
            .map(|stick| stick.close().price())
            .tuple_windows()
            .map(|(prev, current)| current - prev)
            .map(PriceChange::from_change)
            .apply(Rsi::from_changes)?;

        let rsi_stamp = RsiStamp::new(open, close, rsi);
        Some(rsi_stamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod tests_rsi {
        use super::*;
        use std::iter::once;

        #[test]
        fn test_from_changes() {
            let changes = vec![
                PriceChange::Increase(1.0),
                PriceChange::Increase(5.0),
                PriceChange::Decrease(3.0),
                PriceChange::Increase(4.0),
                PriceChange::Decrease(3.0),
            ];
            let rsi = Rsi::from_changes(changes).unwrap();

            assert_eq!(10.0 / (10.0 + 6.0) * 100.0, rsi.percent());
        }

        #[test]
        fn test_from_changes_once_increase() {
            let changes = once(PriceChange::Increase(5.0));
            let rsi = Rsi::from_changes(changes).unwrap();

            assert_eq!(100.0, rsi.percent());
        }

        #[test]
        fn test_from_changes_once_decrease() {
            let changes = once(PriceChange::Decrease(5.0));
            let rsi = Rsi::from_changes(changes).unwrap();

            assert_eq!(0.0, rsi.percent());
        }

        #[test]
        fn test_from_changes_empty() {
            assert_eq!(None, Rsi::from_changes(std::iter::empty()));
        }
    }

    mod tests_rsi_history {
        use super::*;

        #[test]
        fn test_initial_state() {
            let interval = Duration::hours(1);
            let stick_required_count = 5;
            let history = RsiHistory::new(interval, stick_required_count);

            assert!(history.candlesticks().is_empty());
            assert!(history.rsis().is_empty());
        }

        #[test]
        fn test_correct_case() {
            let interval = Duration::hours(1);
            let stick_required_count = 5;
            let mut history = RsiHistory::new(interval, stick_required_count);

            // Span 1
            history.update(PriceStamp::new(dt_hm(0, 0), 1.0)).unwrap();
            history.update(PriceStamp::new(dt_hm(0, 30), 2.0)).unwrap();
            // Span 2
            history.update(PriceStamp::new(dt_hm(1, 0), 2.0)).unwrap();
            history.update(PriceStamp::new(dt_hm(1, 30), 4.0)).unwrap();
            // Span 3
            history.update(PriceStamp::new(dt_hm(2, 0), 4.0)).unwrap();
            history.update(PriceStamp::new(dt_hm(2, 30), 2.0)).unwrap();
            // Span 4
            history.update(PriceStamp::new(dt_hm(3, 0), 2.0)).unwrap();
            history.update(PriceStamp::new(dt_hm(3, 30), 6.0)).unwrap();
            // Span 5
            history.update(PriceStamp::new(dt_hm(4, 0), 6.0)).unwrap();
            history.update(PriceStamp::new(dt_hm(4, 30), 2.0)).unwrap();
            // Span 6
            history.update(PriceStamp::new(dt_hm(5, 0), 2.0)).unwrap();

            // Calculate RSI during span1~span6
            let rsis = history.rsis();
            assert_eq!(5, rsis.len());
            assert_eq!(None, rsis[0]);
            assert_eq!(None, rsis[1]);
            assert_eq!(None, rsis[2]);
            assert_eq!(None, rsis[3]);
            assert_eq!(
                Some(RsiStamp::new(
                    dt_hm(0, 0),
                    dt_hm(4, 30),
                    Rsi { rsi: 6.0 / 12.0 }
                )),
                rsis[4]
            );

            // Spen6
            history.update(PriceStamp::new(dt_hm(5, 30), 5.0)).unwrap();

            // Re-acquire rsi sequence
            // But Span6 rsi has not determined yet, so no change about rsi occurs
            let rsis = history.rsis();
            assert_eq!(5, rsis.len());

            // Span7. thus, Span6 finished
            history.update(PriceStamp::new(dt_hm(6, 0), 5.0)).unwrap();

            let rsis = history.rsis();
            assert_eq!(6, rsis.len());
            assert_eq!(
                Some(RsiStamp::new(
                    dt_hm(1, 0),
                    dt_hm(5, 30),
                    Rsi { rsi: 7.0 / 13.0 }
                )),
                rsis[5]
            );

            // Candlesticks and Rsis must have equal length
            assert_eq!(history.rsis().len(), history.candlesticks().len());
        }

        #[test]
        fn test_incorrect_timestamp() {
            let interval = Duration::hours(1);
            let stick_required_count = 5;
            let mut history = RsiHistory::new(interval, stick_required_count);

            history.update(PriceStamp::new(dt_hm(0, 30), 1.0)).unwrap();

            // Price timestamp must be greater than previous ones, so this update fails
            let ret = history.update(PriceStamp::new(dt_hm(0, 29), 1.0));
            assert!(ret.is_err());
        }

        #[test]
        #[should_panic]
        fn test_incorrect_interval() {
            RsiHistory::new(Duration::microseconds(-1), 10);
        }

        #[test]
        #[should_panic]
        fn test_incorrect_candlestick_count() {
            RsiHistory::new(Duration::hours(1), 0);
        }
    }

    fn dt_hm(hour: u32, minute: u32) -> Timestamp {
        chrono::NaiveDate::from_ymd(2021, 1, 1).and_hms(hour, minute, 0)
    }
}
