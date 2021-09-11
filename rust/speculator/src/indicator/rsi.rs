use super::chart::{CandleStick, Price, PriceStamp};
use apply::Apply;
use chrono::Duration;
use chrono::DurationRound;
use common::alias::BoxErr;
use itertools::Itertools;

#[derive(Debug, Clone, Copy, PartialEq)]
enum PriceChange {
    Increase(Price),
    Decrease(Price),
}

impl PriceChange {
    fn from_change(change: Price) -> Self {
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
pub struct RsiStamp<T> {
    open: T,
    close: T,
    rsi: Rsi,
}

impl<T> RsiStamp<T> {
    pub fn new(open: T, close: T, rsi: Rsi) -> Self {
        Self { open, close, rsi }
    }

    pub fn open(&self) -> &T {
        &self.open
    }

    pub fn close(&self) -> &T {
        &self.close
    }

    pub fn rsi(&self) -> Rsi {
        self.rsi
    }
}

#[derive(Debug, Clone)]
pub struct RsiHistory<T> {
    candlestick_span: Duration,
    candlestick_required_count: usize,
    candlesticks: Vec<CandleStick<T>>,
    candlestick_rsis: Vec<RsiStamp<T>>,
    /// Prices after the last candlestick
    incomplete_history: IncompleteRsiHistory<T>,
    is_all_candlestick_determined: bool,
}

impl<T> RsiHistory<T> {
    pub fn new(candlestick_span: Duration, candlestick_required_count: usize) -> Self {
        Self {
            candlestick_span,
            candlestick_required_count,
            candlesticks: vec![],
            candlestick_rsis: vec![],
            incomplete_history: IncompleteRsiHistory::new(candlestick_span),
            is_all_candlestick_determined: false,
        }
    }

    pub fn candlestick_span(&self) -> Duration {
        self.candlestick_span
    }

    pub fn candlestick_required_count(&self) -> usize {
        self.candlestick_required_count
    }

    pub fn is_all_candlestick_determined(&self) -> bool {
        self.is_all_candlestick_determined
    }

    pub fn update(&mut self, price_stamp: PriceStamp<T>) -> Result<(), BoxErr>
    where
        T: Copy + Ord + DurationRound,
        <T as DurationRound>::Err: Send + Sync + 'static,
    {
        match self.incomplete_history.update(price_stamp) {
            IncompleteRsiUpdate::CandlestickDetermined(stick) => {
                // update stick analysis
                self.candlesticks.push(stick);
                if let Some(rsi) = self.calculate_rsi() {
                    self.candlestick_rsis.push(rsi);
                    self.is_all_candlestick_determined = true;
                } else {
                    self.is_all_candlestick_determined = false;
                }
                Ok(())
            }
            IncompleteRsiUpdate::ToBeContinued => {
                self.is_all_candlestick_determined = false;
                Ok(())
            }
            IncompleteRsiUpdate::Err(e) => Err(e),
        }
    }

    pub fn rsis(&self) -> impl Iterator<Item = &RsiStamp<T>> {
        self.candlestick_rsis.iter()
    }

    fn calculate_rsi(&self) -> Option<RsiStamp<T>>
    where
        T: Copy,
    {
        // Requires sufficient number of sticks to calculate rsi properly
        if self.candlesticks.len() < self.candlestick_required_count {
            return None;
        }

        // Take last n sticks
        let target_sticks = self
            .candlesticks
            .iter()
            .rev()
            .take(self.candlestick_required_count)
            .rev()
            .map(CandleStick::open);

        target_sticks.apply(Self::calculate_rsi_of)
    }

    fn calculate_rsi_of<'a>(
        iter: impl IntoIterator<Item = &'a PriceStamp<T>> + Clone,
    ) -> Option<RsiStamp<T>>
    where
        T: Copy + 'a,
    {
        let (open, close) = {
            let mut iter = iter.clone().into_iter();
            let open = *iter.next()?.stamp();
            let close = *iter.last()?.stamp();
            (open, close)
        };

        let rsi = iter
            .into_iter()
            .tuple_windows()
            .map(|(prev, next)| next.price() - prev.price())
            .map(PriceChange::from_change)
            .apply(Rsi::from_changes)?;

        let rsi_stamp = RsiStamp::new(open, close, rsi);
        Some(rsi_stamp)
    }
}

#[derive(Debug)]
enum IncompleteRsiUpdate<T> {
    CandlestickDetermined(CandleStick<T>),
    ToBeContinued,
    Err(BoxErr),
}

#[derive(Debug, Clone, PartialEq)]
struct IncompleteRsiHistory<T> {
    span: Duration,
    prices: Vec<PriceStamp<T>>,
}

impl<T> IncompleteRsiHistory<T> {
    fn new(span: Duration) -> Self {
        Self {
            span,
            prices: vec![],
        }
    }

    fn update(&mut self, price_stamp: PriceStamp<T>) -> IncompleteRsiUpdate<T>
    where
        T: Copy + Ord + DurationRound,
        <T as DurationRound>::Err: Send + Sync + 'static,
    {
        if let Some(last) = self.prices.last() {
            if last.stamp() >= price_stamp.stamp() {
                return IncompleteRsiUpdate::Err("Timestamp constraint failure".into());
            }
        }

        match self.prices.first() {
            Some(first) => {
                let trunc1 = match first.stamp().duration_trunc(self.span) {
                    Ok(round) => round,
                    Err(e) => return IncompleteRsiUpdate::Err(e.into()),
                };
                let trunc2 = match price_stamp.stamp().duration_trunc(self.span) {
                    Ok(round) => round,
                    Err(e) => return IncompleteRsiUpdate::Err(e.into()),
                };
                if trunc1 == trunc2 {
                    self.prices.push(price_stamp);
                } else {
                    let stick = CandleStick::from_price_stamps(self.prices.drain(..))
                        .expect("prices must not be empty");
                    // Clear previous prices to calulate next candlestick
                    self.prices = vec![price_stamp];
                    return IncompleteRsiUpdate::CandlestickDetermined(stick);
                }
            }
            None => self.prices.push(price_stamp),
        }
        IncompleteRsiUpdate::ToBeContinued
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, NaiveDate, Utc};

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

    mod tests_incomplete_rsi_history {
        use super::*;

        #[test]
        fn test_update() {
            let span = Duration::hours(1);
            let mut history = IncompleteRsiHistory::new(span);

            // Timespan is not enough, thus no candelstick should be returned
            let ret = history.update(PriceStamp::new(dt_hm(0, 0), 10.0));
            assert!(matches!(ret, IncompleteRsiUpdate::ToBeContinued));
            let ret = history.update(PriceStamp::new(dt_hm(0, 15), 12.0));
            assert!(matches!(ret, IncompleteRsiUpdate::ToBeContinued));
            let ret = history.update(PriceStamp::new(dt_hm(0, 30), 20.0));
            assert!(matches!(ret, IncompleteRsiUpdate::ToBeContinued));
            let ret = history.update(PriceStamp::new(dt_hm(0, 45), 5.0));
            assert!(matches!(ret, IncompleteRsiUpdate::ToBeContinued));

            // Timespan is 5(=6-1), thus candlestick should be determined
            // And timespan is cleared
            let stick = match history.update(PriceStamp::new(dt_hm(1, 0), 15.0)) {
                IncompleteRsiUpdate::CandlestickDetermined(stick) => stick,
                other => panic!("{:?}", other),
            };
            assert_eq!(&PriceStamp::new(dt_hm(0, 0), 10.0), stick.open());
            assert_eq!(&PriceStamp::new(dt_hm(0, 30), 20.0), stick.high());
            assert_eq!(&PriceStamp::new(dt_hm(0, 45), 5.0), stick.low());
            assert_eq!(&PriceStamp::new(dt_hm(0, 45), 5.0), stick.close());

            // Timespan is cleared, thus no candelstick should be returned
            let ret = history.update(PriceStamp::new(dt_hm(1, 15), 5.0));
            assert!(matches!(ret, IncompleteRsiUpdate::ToBeContinued));
        }

        #[test]
        fn test_update_invalid_stamp() {
            let span = Duration::hours(1);
            let mut history = IncompleteRsiHistory::new(span);

            history.update(PriceStamp::new(dt_hm(0, 30), 10.0));
            // Timestamp constraint failure
            let ret = history.update(PriceStamp::new(dt_hm(0, 30), 12.0));
            assert!(matches!(ret, IncompleteRsiUpdate::Err(_)));
        }
    }

    mod tests_rsi_hsitory {
        use super::*;

        #[test]
        fn test_calculate_rsi_of() {
            // Total increase: 30, decrease: 20
            let price_stamps = vec![
                PriceStamp::new(dt_hm(0, 0), 10.0),
                PriceStamp::new(dt_hm(0, 15), 20.0),
                PriceStamp::new(dt_hm(0, 30), 40.0),
                PriceStamp::new(dt_hm(0, 45), 20.0),
            ];
            let rsi = RsiHistory::calculate_rsi_of(&price_stamps).unwrap();

            assert_eq!(&dt_hm(0, 0), rsi.open());
            assert_eq!(&dt_hm(0, 45), rsi.close());
            assert_eq!(30.0 / 50.0 * 100.0, rsi.rsi().percent());
        }

        #[test]
        fn test_calculate_rsi_of_empty() {
            assert_eq!(
                None,
                RsiHistory::<DateTime<Utc>>::calculate_rsi_of(std::iter::empty())
            );
        }

        #[test]
        fn test_update() {
            let span = Duration::hours(1);
            let stick_required_count = 5;
            let mut history = RsiHistory::new(span, stick_required_count);

            // Span 1
            history.update(PriceStamp::new(dt_hm(0, 0), 1.0)).unwrap();
            history.update(PriceStamp::new(dt_hm(0, 30), 2.0)).unwrap();
            // Span 2
            history.update(PriceStamp::new(dt_hm(1, 0), 3.0)).unwrap();
            history.update(PriceStamp::new(dt_hm(1, 30), 4.0)).unwrap();
            // Span 3
            history.update(PriceStamp::new(dt_hm(2, 0), 1.0)).unwrap();
            history.update(PriceStamp::new(dt_hm(2, 30), 2.0)).unwrap();
            // Span 4
            history.update(PriceStamp::new(dt_hm(3, 0), 5.0)).unwrap();
            history.update(PriceStamp::new(dt_hm(3, 30), 6.0)).unwrap();
            // Span 5
            history.update(PriceStamp::new(dt_hm(4, 0), 1.0)).unwrap();
            history.update(PriceStamp::new(dt_hm(4, 30), 2.0)).unwrap();
            // Span 6
            history.update(PriceStamp::new(dt_hm(5, 0), 2.0)).unwrap();

            // Calculate RSI during span1~span6
            let rsis = history.rsis().cloned().collect::<Vec<_>>();
            let expected = vec![
                // Span5 rsi
                RsiStamp::new(dt_hm(0, 0), dt_hm(4, 0), Rsi { rsi: 6.0 / 12.0 }),
                // Span6 rsi has not determined yet
            ];
            assert_eq!(expected, rsis);

            // Spen6
            history.update(PriceStamp::new(dt_hm(5, 30), 3.0)).unwrap();

            // Re-acquire rsi sequence
            let rsis = history.rsis().cloned().collect::<Vec<_>>();
            let expected = vec![
                // Span5 rsi
                RsiStamp::new(dt_hm(0, 0), dt_hm(4, 0), Rsi { rsi: 6.0 / 12.0 }),
                // Span6 rsi has not determined yet
            ];

            assert_eq!(expected, rsis);

            // Span7. thus, Span6 finished
            history.update(PriceStamp::new(dt_hm(6, 0), 4.0)).unwrap();

            let rsis = history.rsis().cloned().collect::<Vec<_>>();
            let expected = vec![
                // Span5 rsi
                RsiStamp::new(dt_hm(0, 0), dt_hm(4, 0), Rsi { rsi: 6.0 / 12.0 }),
                // Span6 rsi
                RsiStamp::new(dt_hm(1, 0), dt_hm(5, 0), Rsi { rsi: 5.0 / 11.0 }),
                // Span7 rsi has not determined yet
            ];
            assert_eq!(expected, rsis);
        }
    }

    fn dt_hm(hour: u32, minute: u32) -> DateTime<Utc> {
        let naive = NaiveDate::from_ymd(2021, 1, 1).and_hms(hour, minute, 0);
        DateTime::from_utc(naive, Utc)
    }
}
