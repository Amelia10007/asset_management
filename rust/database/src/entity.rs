pub use mysql::time::Date;

macro_rules! id_type {
    ($t:tt) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $t(pub(crate) i32);
    };
}

id_type!(ServiceId);
id_type!(AssetId);

#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    pub id: ServiceId,
    pub name: String,
}

impl Service {
    pub fn new(id: ServiceId, name: String) -> Self {
        Self { id, name }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub id: AssetId,
    pub name: Option<String>,
    pub unit: Option<String>,
}

impl Asset {
    pub fn new(id: AssetId, name: Option<String>, unit: Option<String>) -> Self {
        Asset { id, name, unit }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Amount {
    pub amount: f64,
}

impl Amount {
    pub fn new(amount: f64) -> Self {
        Self { amount }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    pub date: Date,
    pub base: Asset,
    pub target: Asset,
    pub rate: Amount,
}

#[derive(Debug, Clone, PartialEq)]
pub struct History {
    pub service: Service,
    pub asset: Asset,
    pub amount: Amount,
    pub date: Date,
}

impl History {
    pub fn new(service: Service, asset: Asset, amount: Amount, date: Date) -> Self {
        Self {
            service,
            asset,
            amount,
            date,
        }
    }
}
