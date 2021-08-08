use apply::Apply;
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

    fn from_changes(changes: impl IntoIterator<Item = PriceChange>) -> Option<Self> {
        let mut increase = 0.0;
        let mut decrease = 0.0;
        for change in changes.into_iter() {
            match change {
                PriceChange::Increase(c) => increase += c,
                PriceChange::Decrease(c) => decrease += c,
            }
        }

        let rsi = increase / (increase + decrease);
        if rsi.is_finite() {
            Some(Rsi { rsi })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct RsiHistory {
    timestamps: VecDeque<NaiveDateTime>,
    prices: VecDeque<f64>,
    changes: VecDeque<PriceChange>,
    rsis: VecDeque<Option<Rsi>>,
    timespan: Duration,
}

impl RsiHistory {
    pub fn new(timespan: Duration) -> Self {
        Self {
            timestamps: VecDeque::new(),
            prices: VecDeque::new(),
            changes: VecDeque::new(),
            rsis: VecDeque::new(),
            timespan,
        }
    }

    pub fn timespan(&self) -> Duration {
        self.timespan
    }

    pub fn update_price(&mut self, time: NaiveDateTime, price: f64) {
        let diff = match self.prices.back() {
            Some(prev) => price - prev,
            None => 0.0,
        };
        let change = PriceChange::from_change(diff);

        self.timestamps.push_back(time);
        self.prices.push_back(price);
        self.changes.push_back(change);

        let rsi = self
            .timestamps
            .iter()
            .zip(self.changes.iter())
            .skip_while(|(stamp, _)| time - **stamp > self.timespan)
            .map(|(_, change)| change)
            .copied()
            .apply(Rsi::from_changes);

        self.rsis.push_back(rsi);

        // Drop history out of RSI timespan
        let dropped_count = self
            .timestamps
            .iter()
            .take_while(|stamp| time - **stamp > self.timespan)
            .count();
        self.timestamps.drain(..dropped_count);
        self.prices.drain(..dropped_count);
        self.changes.drain(..dropped_count);
        self.rsis.drain(..dropped_count);
    }

    pub fn rsis(&self) -> std::collections::vec_deque::Iter<'_, Option<Rsi>> {
        self.rsis.iter()
    }
}
