use common::alias::Result;
use common::err::OkOpt;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

#[derive(Debug)]
pub struct MarketSetting {
    pub fee_ratio: f64,
}

impl MarketSetting {
    pub fn from_json(path: impl AsRef<Path>) -> Result<Self> {
        let json = {
            let file = File::open(path)?;
            let mut reader = BufReader::new(file);
            let mut s = String::new();
            reader.read_to_string(&mut s)?;
            json::parse(&s)?
        };

        let fee_ratio = json["feeRatio"].as_f64().ok_opt("Market json: invalid feeRatio")?;
        Ok(Self { fee_ratio })
    }
}
