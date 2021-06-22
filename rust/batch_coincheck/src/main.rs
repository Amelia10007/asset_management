use apply::Apply;
use common::alias::{BoxErr, Result};
use common::err::OkOpt;
use common::http_query::HttpQuery;
use common::settings::Settings;
use database::entity::*;
use database::AssetDatabase;
use json::JsonValue;
use std::collections::HashMap;
use std::env;
use std::str::FromStr;

fn call_public_api(path: &str, query_collection: &HttpQuery<&str, &str>) -> Result<JsonValue> {
    let url = format!("https://coincheck.com{}", path);
    // Build http request
    let client = reqwest::blocking::ClientBuilder::default().build()?;
    let req = client
        .request(reqwest::Method::GET, url)
        .query(query_collection.as_slice())
        .build()?;

    // Get reponse
    let res = client.execute(req)?;
    let res = res.text()?;
    let json = json::parse(&res)?;

    Ok(json)
}

fn call_private_api(
    settings: &Settings,
    path: &str,
    query_collection: &HttpQuery<&str, &str>,
) -> Result<JsonValue> {
    // Load api key
    let access_key = settings.get("access_key").ok_opt("access_key undefined")?;
    let secret_access_key = settings
        .get("secret_access_key")
        .ok_opt("secret_access_key undefined")?;

    // Onetime phrase. This must increase every request.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    let url = format!("https://coincheck.com{}", path);

    // Signing
    let signature = {
        let body = query_collection.build_query();
        let input = format!("{}{}{}", nonce, url, body);
        hmac_sha256::HMAC::mac(input.as_bytes(), secret_access_key.as_bytes())
            .iter()
            .map(|b| format!("{:02x}", b))
            .fold(String::new(), |acc, cur| acc + &cur)
    };

    // Build http request
    let client = reqwest::blocking::ClientBuilder::default().build()?;
    let req = client
        .request(reqwest::Method::GET, url)
        .header("ACCESS-KEY", access_key)
        .header("ACCESS-NONCE", nonce)
        .header("ACCESS-SIGNATURE", signature)
        .query(query_collection.as_slice())
        .build()?;

    // Get reponse
    let res = client.execute(req)?;
    let res = res.text()?;
    let json = json::parse(&res)?;

    Ok(json)
}

fn exchange_rate_between(base_unit: &str, target_unit: &str) -> Result<Amount> {
    let path = format!("/api/rate/{}_{}", base_unit, target_unit);
    let query = HttpQuery::empty();
    let json = call_public_api(&path, &query)?;

    match json["rate"].as_str().map(f64::from_str) {
        Some(Ok(rate)) => Ok(Amount::new(rate)),
        Some(Err(e)) => Err(e.into()),
        None => Err(BoxErr::from(format!("Invalid json: {}", json.to_string()))),
    }
}

fn main() -> Result<()> {
    println!("Coincheck batch started");

    let today = Date::today();
    println!("date: {}", today);
    // Load setting file
    let settings = env::args()
        .skip(1)
        .next()
        .ok_opt("Usage: batch_coincheck access-key-path")?
        .apply(Settings::read_from)?;

    let path = "/api/accounts/balance";
    let query = std::iter::empty().collect();
    let json = call_private_api(&settings, path, &query)?;

    let mut db_con = database::connect_asset_database_as_batch()?;

    let service_name = "coincheck";

    // Register service if necessary
    let service_id = match db_con.service_by_name(service_name)? {
        Some(s) => s.id,
        None => db_con.insert_service(service_name)?.id,
    };

    // Register today
    if let Err(e) = db_con.insert_date(today) {
        println!("{}", e);
    }

    // Register asset. This is necessary to record exchange rates
    let jpy = asset_or_insert(&mut db_con, Some("Japanese Yen"), "JPY")?;
    let usd = asset_or_insert(&mut db_con, Some("US Dollar"), "USD")?;
    {
        if let Err(e) =
            exchange_rate_between(jpy.unit.as_deref().unwrap(), usd.unit.as_deref().unwrap())
        {
            println!("{}", e);
        }
    };

    // Total amount of each currency
    let mut asset_sums = HashMap::<&str, f64>::new();

    for (name, amount) in json
        .entries()
        // Filter non-currency response
        .filter(|(key, _)| *key != "success")
        // Group btc, btc_reserved, btc_debt, etc.
        .filter_map(|(k, v)| k.split('_').next().map(|k| (k, v)))
        // Extract amount
        .filter_map(|(k, v)| {
            v.as_str()
                .and_then(|s| f64::from_str(s).ok())
                .map(|v| (k, v))
        })
        .filter(|(_, v)| *v != 0.0)
    {
        asset_sums
            .entry(name)
            .and_modify(|sum| *sum += amount)
            .or_insert(amount);
    }

    for (asset_unit, amount) in asset_sums.into_iter() {
        // Make the same format with other services
        let asset_unit = asset_unit.to_ascii_uppercase();

        let amount = Amount::new(amount);

        // Register asset if necessary
        let asset_id = match asset_or_insert(&mut db_con, None, &asset_unit) {
            Ok(asset) => asset.id,
            Err(e) => {
                println!("{}", e);
                continue;
            }
        };

        // Insert exchange rate
        // Because JPY is very cheap, get inverse exchange rate.
        // This increases digit of the returned rate as json.
        if let Err(e) = exchange_rate_between(&asset_unit, jpy.unit.as_deref().unwrap()).and_then(
            |inverse_rate| {
                let rate = Amount::new(1.0 / inverse_rate.amount);
                db_con.insert_exchange(today, jpy.id, asset_id, rate)
            },
        ) {
            println!("{}", e);
        }

        // Add history
        if let Err(e) = db_con.insert_hisotry(service_id, asset_id, today, amount) {
            println!("{}", e);
        }
    }

    println!("Coincheck batch finished");
    println!();

    Ok(())
}

fn asset_or_insert<A: AssetDatabase>(
    conn: &mut A,
    name: Option<&str>,
    unit: &str,
) -> Result<Asset> {
    match conn.asset_by_unit(unit) {
        Ok(Some(asset)) => Ok(asset),
        Ok(None) => match conn.insert_asset(name, Some(unit)) {
            Ok(asset) => Ok(asset),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    }
}
