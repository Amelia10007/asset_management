use crate::{Duration, Timestamp};
use apply::Apply;
use chrono::DurationRound;
use common::alias::BoxErr;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceStamp {
    stamp: Timestamp,
    price: f64,
}

impl PriceStamp {
    pub const fn new(stamp: Timestamp, price: f64) -> Self {
        Self { stamp, price }
    }

    pub const fn stamp(&self) -> Timestamp {
        self.stamp
    }

    pub const fn price(&self) -> f64 {
        self.price
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candlestick {
    open: PriceStamp,
    close: PriceStamp,
    high: PriceStamp,
    low: PriceStamp,
}

impl Candlestick {
    /// # Panics
    /// Panics if timestamps in `iter` are arrangeed in non-monotonous increase
    pub fn from_price_stamps(iter: impl IntoIterator<Item = PriceStamp>) -> Option<Self> {
        let mut iter = iter.into_iter();

        // Set first price
        let open = iter.next()?;
        let mut close = open.clone();
        let mut high = open.clone();
        let mut low = open.clone();

        // Make stick by remaining prices
        for price_stamp in iter {
            // Price sequence constraint
            assert!(close.stamp < price_stamp.stamp);

            close = price_stamp.clone();

            if high.price() < price_stamp.price() {
                high = price_stamp.clone();
            }
            if low.price() > price_stamp.price() {
                low = price_stamp;
            }
        }

        let stick = Self {
            open,
            close,
            high,
            low,
        };
        Some(stick)
    }

    pub fn open(&self) -> PriceStamp {
        self.open
    }

    pub fn close(&self) -> PriceStamp {
        self.close
    }

    pub fn high(&self) -> PriceStamp {
        self.high
    }

    pub fn low(&self) -> PriceStamp {
        self.low
    }

    pub fn change(&self) -> f64 {
        self.close.price - self.open.price
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandlestickIndicator {
    interval: Duration,
    remaining_price_stamps: Vec<PriceStamp>,
    is_candlestick_determined_just_now: bool,
}

impl CandlestickIndicator {
    /// # Panics
    /// Panics under negative `interval`
    pub fn new(interval: Duration) -> Self {
        assert!(interval > Duration::zero());

        Self {
            interval,
            remaining_price_stamps: vec![],
            is_candlestick_determined_just_now: false,
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn is_candlestick_determined_just_now(&self) -> bool {
        self.is_candlestick_determined_just_now
    }

    pub fn update(&mut self, price_stamp: PriceStamp) -> Result<IndicatorUpdate, BoxErr> {
        match self.remaining_price_stamps.last() {
            Some(last) if last.stamp >= price_stamp.stamp => {
                Err("Timestamp constraint failure".into())
            }
            Some(last) => {
                let trunc1 = last.stamp().apply(to_utc).duration_trunc(self.interval)?;
                let trunc2 = price_stamp
                    .stamp()
                    .apply(to_utc)
                    .duration_trunc(self.interval)?;
                if trunc1 == trunc2 {
                    self.remaining_price_stamps.push(price_stamp);
                    self.is_candlestick_determined_just_now = false;
                    Ok(IndicatorUpdate::NotDeterminedYet)
                } else {
                    let stick =
                        Candlestick::from_price_stamps(self.remaining_price_stamps.drain(..))
                            .expect("prices must not be empty");
                    // Clear previous prices to calulate next candlestick
                    self.remaining_price_stamps = vec![price_stamp];
                    self.is_candlestick_determined_just_now = true;
                    Ok(IndicatorUpdate::Determined(stick))
                }
            }
            None => {
                self.remaining_price_stamps.push(price_stamp);
                self.is_candlestick_determined_just_now = false;
                Ok(IndicatorUpdate::NotDeterminedYet)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IndicatorUpdate {
    Determined(Candlestick),
    NotDeterminedYet,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandlestickHistory {
    indicator: CandlestickIndicator,
    candlesticks: Vec<Candlestick>,
}

impl CandlestickHistory {
    /// # Panics
    /// Panics under negative `interval`
    pub fn new(interval: Duration) -> Self {
        Self {
            indicator: CandlestickIndicator::new(interval),
            candlesticks: vec![],
        }
    }

    pub fn interval(&self) -> Duration {
        self.indicator.interval()
    }

    pub fn is_candlestick_determined_just_now(&self) -> bool {
        self.indicator.is_candlestick_determined_just_now()
    }

    pub fn candlesticks(&self) -> &[Candlestick] {
        &self.candlesticks
    }

    pub fn update(&mut self, price_stamp: PriceStamp) -> Result<IndicatorUpdate, BoxErr> {
        let res = self.indicator.update(price_stamp);

        if let Ok(IndicatorUpdate::Determined(stick)) = res {
            self.candlesticks.push(stick);
        }

        res
    }
}

fn to_utc(timestamp: Timestamp) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_utc(timestamp, chrono::Utc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_approx_eq::assert_approx_eq;
    use std::iter::{empty, once};

    mod tests_candlestick {
        use super::*;

        #[test]
        fn test_from_price_stamps_empty() {
            let iter = empty();
            let stick = Candlestick::from_price_stamps(iter);
            assert_eq!(None, stick);
        }

        #[test]
        fn test_from_price_stamps_once() {
            let iter = once(PriceStamp::new(dt(1), 12.3));
            let stick = Candlestick::from_price_stamps(iter).unwrap();

            assert_eq!(PriceStamp::new(dt(1), 12.3), stick.open());
            assert_eq!(PriceStamp::new(dt(1), 12.3), stick.close());
            assert_eq!(PriceStamp::new(dt(1), 12.3), stick.high());
            assert_eq!(PriceStamp::new(dt(1), 12.3), stick.low());
            assert_eq!(0.0, stick.change());
        }

        #[test]
        fn test_from_price_stamps() {
            let iter = vec![
                PriceStamp::new(dt(1), 23.4), // open
                PriceStamp::new(dt(2), 56.7), // high
                PriceStamp::new(dt(3), 12.3), // low
                PriceStamp::new(dt(4), 45.6), // close
            ];
            let stick = Candlestick::from_price_stamps(iter).unwrap();

            assert_eq!(PriceStamp::new(dt(1), 23.4), stick.open());
            assert_eq!(PriceStamp::new(dt(4), 45.6), stick.close());
            assert_eq!(PriceStamp::new(dt(2), 56.7), stick.high());
            assert_eq!(PriceStamp::new(dt(3), 12.3), stick.low());
            assert_approx_eq!(22.2, stick.change());
        }

        #[test]
        #[should_panic]
        fn test_from_price_stamps_invalid_stamp_order() {
            let iter = vec![PriceStamp::new(dt(1), 23.4), PriceStamp::new(dt(1), 56.7)];
            let _ = Candlestick::from_price_stamps(iter);
        }

        fn dt(hour: u32) -> Timestamp {
            chrono::NaiveDate::from_ymd(2021, 1, 1).and_hms(hour, 0, 0)
        }
    }

    mod tests_candlestick_indicator {
        use super::*;

        #[test]
        fn test_correct_case() {
            let interval = Duration::hours(1);
            let mut indicator = CandlestickIndicator::new(interval);

            // Initital state
            assert_eq!(interval, indicator.interval());
            assert!(!indicator.is_candlestick_determined_just_now());

            // Add span 1
            let ret = indicator.update(PriceStamp::new(dt_hm(0, 0), 1.0)).unwrap();
            assert_eq!(IndicatorUpdate::NotDeterminedYet, ret);
            assert!(!indicator.is_candlestick_determined_just_now());
            let ret = indicator
                .update(PriceStamp::new(dt_hm(0, 30), 3.0))
                .unwrap();
            assert_eq!(IndicatorUpdate::NotDeterminedYet, ret);
            assert!(!indicator.is_candlestick_determined_just_now());

            // Add span2, then span1 determine
            let ret = indicator.update(PriceStamp::new(dt_hm(1, 0), 2.0)).unwrap();
            match ret {
                IndicatorUpdate::Determined(stick) => {
                    assert_eq!(1.0, stick.open().price());
                    assert_eq!(3.0, stick.close().price());
                }
                other => panic!("{:?}", other),
            }
            assert!(indicator.is_candlestick_determined_just_now());

            // Add span2 more
            let ret = indicator
                .update(PriceStamp::new(dt_hm(1, 30), 3.0))
                .unwrap();
            assert_eq!(IndicatorUpdate::NotDeterminedYet, ret);
            assert!(!indicator.is_candlestick_determined_just_now());

            // Add span3, then span2 determine
            let ret = indicator.update(PriceStamp::new(dt_hm(2, 0), 2.0)).unwrap();
            match ret {
                IndicatorUpdate::Determined(stick) => {
                    assert_eq!(2.0, stick.open().price());
                    assert_eq!(3.0, stick.close().price());
                }
                other => panic!("{:?}", other),
            }
            assert!(indicator.is_candlestick_determined_just_now());
        }

        #[test]
        fn test_incorrect_timestamp() {
            let interval = Duration::hours(1);
            let mut indicator = CandlestickIndicator::new(interval);

            indicator
                .update(PriceStamp::new(dt_hm(0, 30), 1.0))
                .unwrap();
            // Price timestamp must be greater than previous ones, so this update fails
            let ret = indicator.update(PriceStamp::new(dt_hm(0, 30), 1.0));
            assert!(ret.is_err());
        }

        #[test]
        #[should_panic]
        fn test_incorrect_interval() {
            CandlestickIndicator::new(Duration::milliseconds(-1));
        }
    }

    mod tests_candlestick_history {
        use super::*;

        #[test]
        fn test_correct_case() {
            let interval = Duration::hours(1);
            let mut history = CandlestickHistory::new(interval);

            // Initital state
            assert_eq!(interval, history.interval());
            assert!(!history.is_candlestick_determined_just_now());

            // Add span 1
            let ret = history.update(PriceStamp::new(dt_hm(0, 0), 1.0));
            assert!(matches!(ret, Ok(IndicatorUpdate::NotDeterminedYet)));
            assert!(!history.is_candlestick_determined_just_now());
            let ret = history.update(PriceStamp::new(dt_hm(0, 30), 3.0));
            assert!(matches!(ret, Ok(IndicatorUpdate::NotDeterminedYet)));
            assert!(!history.is_candlestick_determined_just_now());

            // Add span2, then span1 determine
            let ret = history.update(PriceStamp::new(dt_hm(1, 0), 2.0));
            assert!(matches!(ret, Ok(IndicatorUpdate::Determined(..))));
            assert!(history.is_candlestick_determined_just_now());

            // Add span2 more
            let ret = history.update(PriceStamp::new(dt_hm(1, 30), 3.0));
            assert!(matches!(ret, Ok(IndicatorUpdate::NotDeterminedYet)));
            assert!(!history.is_candlestick_determined_just_now());

            // Add span3, then span2 determine
            let ret = history.update(PriceStamp::new(dt_hm(2, 0), 2.0));
            assert!(matches!(ret, Ok(IndicatorUpdate::Determined(..))));
            assert!(history.is_candlestick_determined_just_now());

            // check candlesticks
            let sticks = history.candlesticks();

            assert_eq!(2, sticks.len());
            assert_eq!(1.0, sticks[0].open().price());
            assert_eq!(3.0, sticks[0].close().price());
            assert_eq!(2.0, sticks[1].open().price());
            assert_eq!(3.0, sticks[1].close().price());
        }

        #[test]
        fn test_incorrect_timestamp() {
            let interval = Duration::hours(1);
            let mut history = CandlestickHistory::new(interval);

            history.update(PriceStamp::new(dt_hm(0, 30), 1.0)).unwrap();
            // Price timestamp must be greater than previous ones, so this update fails
            let ret = history.update(PriceStamp::new(dt_hm(0, 30), 1.0));
            assert!(ret.is_err());
        }

        #[test]
        #[should_panic]
        fn test_incorrect_interval() {
            CandlestickHistory::new(Duration::milliseconds(-1));
        }
    }

    fn dt_hm(hour: u32, minute: u32) -> Timestamp {
        chrono::NaiveDate::from_ymd(2021, 1, 1).and_hms(hour, minute, 0)
    }
}
