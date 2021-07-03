use apply::Apply;
use common::alias::Result;
use common::err::OkOpt;
use common::http_query::HttpQuery;
use common::log::prelude::*;
use common::settings::Settings;
use database::entity::*;
use database::AssetDatabase;
use database::Date;
use json::JsonValue;
use std::env;
use std::str::FromStr;

/// Executes nicehash API via api-key written in the specified file, then returns the response as json.
fn call_private_api(
    settings: &Settings,
    path: &str,
    query_collection: &HttpQuery<&str, &str>,
) -> Result<JsonValue> {
    // Load api key
    let organization_id = settings
        .get("organization_id")
        .ok_opt("organization_id undefined")?;
    let api_key_code = settings
        .get("api_key_code")
        .ok_opt("api_key_code undefined")?;
    let api_secret_key_code = settings
        .get("api_secret_key_code")
        .ok_opt("api_secret_key_code undefined")?;

    // Fetch timestamp
    let server_timestamp_millis = {
        let res = reqwest::blocking::get("https://api2.nicehash.com/api/v2/time")?;
        let text = res.text()?;
        let res = json::parse(&text)?;

        res["serverTime"].as_u64().ok_opt("Invalid serverTime")?
    };

    // Onetime phrase
    let nonce = uuid::Uuid::new_v4();
    let request_id = uuid::Uuid::new_v4();

    //
    let method = "GET";
    let query = query_collection.build_query();

    // Digital signing
    let auth = {
        let input = format!(
            "{}\0{}\0{}\0\0{}\0\0{}\0{}\0{}",
            api_key_code, server_timestamp_millis, nonce, organization_id, method, path, query
        );
        let signature = hmac_sha256::HMAC::mac(input.as_bytes(), api_secret_key_code.as_bytes())
            .iter()
            .map(|b| format!("{:02x}", b))
            .fold(String::new(), |acc, cur| acc + &cur);
        format!("{}:{}", api_key_code, signature)
    };

    //
    let url = format!("https://api2.nicehash.com{}", path);
    let client = reqwest::blocking::ClientBuilder::default().build()?;

    let req = client
        .request(reqwest::Method::GET, url)
        .header("X-Time", server_timestamp_millis)
        .header("X-Nonce", nonce.to_string())
        .header("X-Organization-Id", organization_id)
        .header("X-Request-Id", request_id.to_string())
        .header("X-Auth", auth)
        .query(query_collection.as_slice())
        .build()?;

    // Get reponse
    let res = client.execute(req)?;
    let res = res.text()?;
    let json = json::parse(&res)?;

    Ok(json)
}

fn main() {
    let mut logger = Logger::new(std::io::stdout(), LogLevel::Debug);
    if let Err(e) = batch(&mut logger) {
        error!(logger, "{}", e);
    }
}

/// Usage: batch_nicehash ***api-key-path***
fn batch(logger: &mut Logger<std::io::Stdout>) -> Result<()> {
    info!(logger, "Nicehash batch started");

    let today = Date::today();

    info!(logger, "Date: {}", today);

    // Load setting file
    let settings = env::args()
        .skip(1)
        .next()
        .ok_opt("Usage: batch_nicehash api-key-path")?
        .apply(Settings::read_from)?;

    let path = "/main/api/v2/accounting/accounts2";
    let query = std::iter::once(("fiat", "BTC")).collect();
    let json = call_private_api(&settings, path, &query)?;

    let mut db_con = database::connect_asset_database_as_batch()?;

    let service_name = "nicehash";

    // Register service if necessary
    let service_id = match db_con.service_by_name(service_name)? {
        Some(s) => s.id,
        None => db_con.insert_service(service_name)?.id,
    };

    // Register today's date
    if let Err(e) = db_con.insert_date(today) {
        warn!(logger, "{}", e);
    }

    // Register bitcoin asset. This is necessary to record exchange rates
    let bitcoin = asset_or_insert(&mut db_con, Some("Bitcoin"), "BTC")?;

    for c in json["currencies"]
        .members()
        // Manipulate only active wallets
        .filter(|j| j["active"].as_bool() == Some(true))
    {
        if let (Some(asset_unit), Some(Ok(balance)), Some(btc_rate)) = (
            c["currency"].as_str(),
            c["totalBalance"].as_str().map(f64::from_str),
            c["btcRate"].as_f64(),
        ) {
            info!(
                logger,
                "currency: {} balance: {}, rate: {}BTC", asset_unit, balance, btc_rate
            );

            let amount = Amount::new(balance);

            // Register asset if necessary
            let asset_id = match asset_or_insert(&mut db_con, None, asset_unit) {
                Ok(asset) => asset.id,
                Err(e) => {
                    warn!(logger, "{}", e);
                    continue;;
                }
            };

            // Register exhange rate
            let rate = Amount::new(btc_rate);
            if let Err(e) = db_con.insert_exchange(today, bitcoin.id, asset_id, rate) {
                warn!(logger, "{}", e);
            }

            // Append history
            if let Err(e) = db_con.insert_hisotry(service_id, asset_id, today, amount) {
                warn!(logger, "{}", e);
            }
        } else {
            warn!(logger, "Invalid json: {}", c);
        }
    }

    info!(logger, "Nicehash batch finished");
    info!(logger, "");

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
