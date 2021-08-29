pub type Price = f64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceStamp<T> {
    stamp: T,
    price: Price,
}

impl<T> PriceStamp<T> {
    pub const fn new(stamp: T, price: Price) -> Self {
        Self { stamp, price }
    }

    pub const fn stamp(&self) -> &T {
        &self.stamp
    }

    pub const fn price(&self) -> Price {
        self.price
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandleStick<T> {
    open: PriceStamp<T>,
    close: PriceStamp<T>,
    high: PriceStamp<T>,
    low: PriceStamp<T>,
}

impl<T> CandleStick<T> {
    /// # Panics
    /// Panics if timestamps in `iter` are arrangeed in non-monotonous increase
    pub fn from_price_stamps(iter: impl IntoIterator<Item = PriceStamp<T>>) -> Option<Self>
    where
        T: Clone + Ord,
    {
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

    pub fn open(&self) -> &PriceStamp<T> {
        &self.open
    }

    pub fn close(&self) -> &PriceStamp<T> {
        &self.close
    }

    pub fn high(&self) -> &PriceStamp<T> {
        &self.high
    }

    pub fn low(&self) -> &PriceStamp<T> {
        &self.low
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::iter::{empty, once};

    #[test]
    fn test_from_price_stamps_empty() {
        let iter = empty();
        let stick = CandleStick::<i32>::from_price_stamps(iter);
        assert_eq!(None, stick);
    }

    #[test]
    fn test_from_price_stamps_once() {
        let iter = once(PriceStamp::new(42, 12.3));
        let stick = CandleStick::<i32>::from_price_stamps(iter).unwrap();

        assert_eq!(&PriceStamp::new(42, 12.3), stick.open());
        assert_eq!(&PriceStamp::new(42, 12.3), stick.close());
        assert_eq!(&PriceStamp::new(42, 12.3), stick.high());
        assert_eq!(&PriceStamp::new(42, 12.3), stick.low());
    }

    #[test]
    fn test_from_price_stamps() {
        let iter = vec![
            PriceStamp::new(1, 23.4), // open
            PriceStamp::new(2, 56.7), // high
            PriceStamp::new(3, 12.3), // low
            PriceStamp::new(4, 45.6), // close
        ];
        let stick = CandleStick::<i32>::from_price_stamps(iter).unwrap();

        assert_eq!(&PriceStamp::new(1, 23.4), stick.open());
        assert_eq!(&PriceStamp::new(4, 45.6), stick.close());
        assert_eq!(&PriceStamp::new(2, 56.7), stick.high());
        assert_eq!(&PriceStamp::new(3, 12.3), stick.low());
    }

    #[test]
    #[should_panic]
    fn test_from_price_stamps_invalid_stamp_order() {
        let iter = vec![PriceStamp::new(1, 23.4), PriceStamp::new(1, 56.7)];
        let _ = CandleStick::<i32>::from_price_stamps(iter);
    }
}
