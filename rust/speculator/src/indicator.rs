use anyhow::{ensure, Result};
use chrono::{Duration, DurationRound, NaiveDateTime};
use itertools::Itertools;
use ta::{DataItem, Next};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceStamp {
    stamp: NaiveDateTime,
    price: f64,
}

impl PriceStamp {
    pub fn new(stamp: NaiveDateTime, price: f64) -> Self {
        Self { stamp, price }
    }

    pub fn stamp(&self) -> NaiveDateTime {
        self.stamp
    }

    pub fn price(&self) -> f64 {
        self.price
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataItemBuffer {
    interval: Duration,
    stamps: Vec<PriceStamp>,
}

impl DataItemBuffer {
    /// # Panics
    /// Panics under non-positive `interval`.
    pub fn new(interval: Duration) -> Self {
        assert!(interval > Duration::zero());
        Self {
            interval,
            stamps: vec![],
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    fn next(&mut self, price_stamp: PriceStamp) -> Result<Option<DataItem>> {
        match self.stamps.last() {
            Some(last) => {
                ensure!(
                    last.stamp < price_stamp.stamp,
                    "Timestamp constraint failure"
                );

                let trunc1 = to_utc(last.stamp()).duration_trunc(self.interval)?;
                let trunc2 = to_utc(price_stamp.stamp()).duration_trunc(self.interval)?;
                if trunc1 == trunc2 {
                    self.stamps.push(price_stamp);
                    Ok(None)
                } else {
                    // Use all stamps of previous interval
                    let prices = self.stamps.drain(..).map(|s| s.price).collect_vec();
                    // `prices` is not empty, so no panic occurs below unwrap().
                    let open = prices[0];
                    let close = prices.last().copied().unwrap();
                    let (low, high) = prices.into_iter().minmax().into_option().unwrap();
                    let item = DataItem::builder()
                        .open(open)
                        .close(close)
                        .high(high)
                        .low(low)
                        .volume(0.0)
                        .build()?;
                    // Next interval
                    self.stamps.push(price_stamp);
                    Ok(Some(item))
                }
            }
            None => {
                self.stamps.push(price_stamp);
                Ok(None)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct IndicatorBuffer<T> {
    indicator: T,
    buffer: DataItemBuffer,
    dataitem: DataItem,
}

impl<T> IndicatorBuffer<T> {
    /// # Panics
    /// Panics under non-positive `interval`.
    pub fn new(indicator: T, interval: Duration) -> Self {
        Self {
            indicator,
            buffer: DataItemBuffer::new(interval),
            // dataitem field is only used after after value is set on next() method.
            // uninit value is not used, thus this unsafe code is safe.
            dataitem: unsafe { std::mem::MaybeUninit::zeroed().assume_init() },
        }
    }

    pub fn indicator(&self) -> &T {
        &self.indicator
    }

    pub fn interval(&self) -> Duration {
        self.buffer.interval()
    }

    pub fn next<'a>(&'a mut self, price_stamp: PriceStamp) -> Result<Option<(DataItem, T::Output)>>
    where
        T: Next<&'a DataItem>,
    {
        match self.buffer.next(price_stamp) {
            Ok(Some(dataitem)) => {
                self.dataitem = dataitem;
                let output = self.indicator.next(&self.dataitem);
                Ok(Some((self.dataitem.clone(), output)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IndicatorHistory<T, U> {
    indicator_buffer: IndicatorBuffer<T>,
    history: Vec<Option<(DataItem, U)>>,
}

impl<T, U> IndicatorHistory<T, U> {
    pub fn new<'a>(indicator_buffer: IndicatorBuffer<T>) -> Self
    where
        T: Next<&'a DataItem, Output = U>,
    {
        Self {
            indicator_buffer,
            history: vec![],
        }
    }

    pub fn indicator_buffer(&self) -> &IndicatorBuffer<T> {
        &self.indicator_buffer
    }

    pub fn history(&self) -> &[Option<(DataItem, U)>] {
        &self.history
    }

    pub fn dataitems(&self) -> impl ExactSizeIterator<Item = Option<&DataItem>> {
        self.history
            .iter()
            .map(|h| h.as_ref().map(|(item, _)| item))
    }

    pub fn outputs(&self) -> impl ExactSizeIterator<Item = Option<&U>> {
        self.history
            .iter()
            .map(|h| h.as_ref().map(|(_, output)| output))
    }

    pub fn next<'a>(&'a mut self, price_stamp: PriceStamp) -> Result<Option<&(DataItem, U)>>
    where
        T: Next<&'a DataItem, Output = U>,
    {
        match self.indicator_buffer.next(price_stamp) {
            Ok(opt) => {
                self.history.push(opt);
                Ok(self.history.last().unwrap().as_ref())
            }
            Err(e) => Err(e),
        }
    }
}

fn to_utc(stamp: NaiveDateTime) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_utc(stamp, chrono::Utc)
}

#[cfg(test)]
mod tests_dataitem_buffer {
    use super::tests::*;
    use super::*;
    use ta::*;

    #[test]
    fn test_next() {
        // An hour interval buffer
        let mut b = DataItemBuffer::new(Duration::hours(1));

        // Span 1
        let ret = b.next(pstamp(1, 0, 2.0));
        assert!(matches!(ret, Ok(None)));
        let ret = b.next(pstamp(1, 1, 1.0));
        assert!(matches!(ret, Ok(None)));
        let ret = b.next(pstamp(1, 59, 3.0));
        assert!(matches!(ret, Ok(None)));

        // Span1 finished and Span2 started
        let dataitem_span1 = b.next(pstamp(2, 0, 4.0)).unwrap().unwrap();
        assert_eq!(2.0, dataitem_span1.open());
        assert_eq!(3.0, dataitem_span1.high());
        assert_eq!(1.0, dataitem_span1.low());
        assert_eq!(3.0, dataitem_span1.close());
        assert_eq!(0.0, dataitem_span1.volume());

        // Span2 continues..
        let ret = b.next(pstamp(2, 1, 2.0));
        assert!(matches!(ret, Ok(None)));
        let ret = b.next(pstamp(2, 59, 3.0));
        assert!(matches!(ret, Ok(None)));

        // Span1 finished and Span2 started
        let dataitem_span2 = b.next(pstamp(3, 0, 5.0)).unwrap().unwrap();
        assert_eq!(4.0, dataitem_span2.open());
        assert_eq!(4.0, dataitem_span2.high());
        assert_eq!(2.0, dataitem_span2.low());
        assert_eq!(3.0, dataitem_span2.close());
        assert_eq!(0.0, dataitem_span2.volume());
    }

    #[test]
    fn test_next_invalid_timestamp() {
        let mut b = DataItemBuffer::new(Duration::hours(1));

        // Span 1
        b.next(pstamp(1, 0, 2.0)).unwrap();

        // New price_stamp's timestamp is not latest, then denied
        let ret = b.next(pstamp(1, 0, 2.0));
        assert!(ret.is_err());
    }

    #[test]
    #[should_panic]
    fn test_non_positive_interval() {
        let _ = DataItemBuffer::new(Duration::zero());
    }
}

#[cfg(test)]
mod tests_indicator_buffer {
    use super::tests::*;
    use super::*;
    use ta::indicators::SimpleMovingAverage;
    use ta::*;

    #[test]
    fn test_next() {
        let indicator = SimpleMovingAverage::new(3).unwrap();
        let mut b = IndicatorBuffer::new(indicator, Duration::hours(1));

        // Span 1
        let ret = b.next(pstamp(1, 0, 2.0));
        assert!(matches!(ret, Ok(None)));
        let ret = b.next(pstamp(1, 59, 3.0));
        assert!(matches!(ret, Ok(None)));

        // Span1 finished and Span2 started
        let (dataitem_span1, output_span1) = b.next(pstamp(2, 0, 4.0)).unwrap().unwrap();
        assert_eq!(3.0, output_span1);
        assert_eq!(2.0, dataitem_span1.open());
        assert_eq!(3.0, dataitem_span1.high());
        assert_eq!(2.0, dataitem_span1.low());
        assert_eq!(3.0, dataitem_span1.close());
        assert_eq!(0.0, dataitem_span1.volume());

        // Span2 continues..
        let ret = b.next(pstamp(2, 1, 2.0));
        assert!(matches!(ret, Ok(None)));
        let ret = b.next(pstamp(2, 59, 4.0));
        assert!(matches!(ret, Ok(None)));

        // Span2 finished and Span3 started
        let (dataitem_span2, output_span2) = b.next(pstamp(3, 0, 5.0)).unwrap().unwrap();
        assert_eq!((3.0 + 4.0) / 2.0, output_span2);
        assert_eq!(4.0, dataitem_span2.open());
        assert_eq!(4.0, dataitem_span2.high());
        assert_eq!(2.0, dataitem_span2.low());
        assert_eq!(4.0, dataitem_span2.close());
        assert_eq!(0.0, dataitem_span2.volume());

        // Span3 finished and Span4 started
        let (_, output_span3) = b.next(pstamp(4, 0, 6.0)).unwrap().unwrap();
        assert_eq!((3.0 + 4.0 + 5.0) / 3.0, output_span3);

        // Span4 finished and Span5 started
        let (_, output_span4) = b.next(pstamp(5, 0, 7.0)).unwrap().unwrap();
        assert_eq!((4.0 + 5.0 + 6.0) / 3.0, output_span4);
    }

    #[test]
    fn test_next_invalid_timestamp() {
        let indicator = SimpleMovingAverage::new(3).unwrap();
        let mut b = IndicatorBuffer::new(indicator, Duration::hours(1));

        // Span 1
        b.next(pstamp(1, 0, 2.0)).unwrap();

        // New price_stamp's timestamp is not latest, then denied
        let ret = b.next(pstamp(1, 0, 2.0));
        assert!(ret.is_err());
    }

    #[test]
    #[should_panic]
    fn test_non_positive_interval() {
        let indicator = SimpleMovingAverage::new(3).unwrap();
        let _ = IndicatorBuffer::new(indicator, Duration::zero());
    }
}

#[cfg(test)]
mod tests_indicator_history {
    use super::tests::*;
    use super::*;
    use ta::indicators::SimpleMovingAverage;
    use ta::*;

    #[test]
    fn test_next() {
        let indicator = SimpleMovingAverage::new(3).unwrap();
        let b = IndicatorBuffer::new(indicator, Duration::hours(1));
        let mut h = IndicatorHistory::new(b);

        // Span 1
        let ret = h.next(pstamp(1, 0, 2.0));
        assert!(matches!(ret, Ok(None)));
        let ret = h.next(pstamp(1, 59, 3.0));
        assert!(matches!(ret, Ok(None)));

        // Span1 finished and Span2 started
        let (dataitem_span1, output_span1) = h.next(pstamp(2, 0, 4.0)).unwrap().cloned().unwrap();
        assert_eq!(3.0, output_span1);
        assert_eq!(2.0, dataitem_span1.open());
        assert_eq!(3.0, dataitem_span1.high());
        assert_eq!(2.0, dataitem_span1.low());
        assert_eq!(3.0, dataitem_span1.close());
        assert_eq!(0.0, dataitem_span1.volume());

        // Span2 continues..
        let ret = h.next(pstamp(2, 1, 2.0));
        assert!(matches!(ret, Ok(None)));
        let ret = h.next(pstamp(2, 59, 4.0));
        assert!(matches!(ret, Ok(None)));

        // Span2 finished and Span3 started
        let (dataitem_span2, output_span2) = h.next(pstamp(3, 0, 5.0)).unwrap().cloned().unwrap();
        assert_eq!((3.0 + 4.0) / 2.0, output_span2);
        assert_eq!(4.0, dataitem_span2.open());
        assert_eq!(4.0, dataitem_span2.high());
        assert_eq!(2.0, dataitem_span2.low());
        assert_eq!(4.0, dataitem_span2.close());
        assert_eq!(0.0, dataitem_span2.volume());

        // Span3 finished and Span4 started
        let (_, output_span3) = h.next(pstamp(4, 0, 6.0)).unwrap().cloned().unwrap();
        assert_eq!((3.0 + 4.0 + 5.0) / 3.0, output_span3);

        // Span4 finished and Span5 started
        let (_, output_span4) = h.next(pstamp(5, 0, 7.0)).unwrap().cloned().unwrap();
        assert_eq!((4.0 + 5.0 + 6.0) / 3.0, output_span4);

        // Review history
        let history = h.history();
        assert_eq!(8, history.len());
        assert!(history[0].is_none());
        assert!(history[1].is_none());
        assert!(history[2].is_some()); // Span 1 finish
        assert!(history[3].is_none());
        assert!(history[4].is_none());
        assert!(history[5].is_some()); // Span2 finish
        assert!(history[6].is_some()); // Span3 finish
        assert!(history[7].is_some()); // Span4 finish
        assert_eq!(
            dataitem_span1.close(),
            history[2].as_ref().unwrap().0.close()
        );
        assert_eq!(
            dataitem_span2.close(),
            history[5].as_ref().unwrap().0.close()
        );
        assert_eq!(3.0, history[2].as_ref().unwrap().1);
        assert_eq!((3.0 + 4.0) / 2.0, history[5].as_ref().unwrap().1);
        assert_eq!((3.0 + 4.0 + 5.0) / 3.0, history[6].as_ref().unwrap().1);
        assert_eq!((4.0 + 5.0 + 6.0) / 3.0, history[7].as_ref().unwrap().1);

        // Review dataitems
        assert_eq!(8, h.dataitems().len());

        // Review SMA
        let smas = h.outputs().map(|opt| opt.copied()).collect_vec();
        let expected = vec![
            None,
            None,
            Some(3.0),
            None,
            None,
            Some((3.0 + 4.0) / 2.0),
            Some((3.0 + 4.0 + 5.0) / 3.0),
            Some((4.0 + 5.0 + 6.0) / 3.0),
        ];
        assert_eq!(expected, smas);
    }

    #[test]
    fn test_next_invalid_timestamp() {
        let indicator = SimpleMovingAverage::new(3).unwrap();
        let b = IndicatorBuffer::new(indicator, Duration::hours(1));
        let mut h = IndicatorHistory::new(b);

        // Span 1
        h.next(pstamp(1, 0, 2.0)).unwrap();

        // New price_stamp's timestamp is not latest, then denied
        let ret = h.next(pstamp(1, 0, 2.0));
        assert!(ret.is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::PriceStamp;
    use chrono::NaiveDateTime;

    pub fn pstamp(hour: u32, minute: u32, price: f64) -> PriceStamp {
        let stamp = hm(hour, minute);
        PriceStamp::new(stamp, price)
    }

    fn hm(hour: u32, minute: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd(2021, 1, 1).and_hms(hour, minute, 0)
    }
}
