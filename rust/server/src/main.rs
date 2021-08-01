use apply::{Also, Apply};
use common::alias::{BoxErr, Result};
use common::err::OkOpt;
use database::diesel::prelude::*;
use database::diesel::{self, QueryDsl, RunQueryDsl};
use database::logic::{list_currencies, Conn};
use database::model::*;
use database::schema;
use exchange_graph::ExchangeGraph;
use hyper::server::Server;
use hyper::service::*;
use hyper::{Body, Request, Response, Uri};
use rayon::prelude::*;
use std::env;
use std::io::Read;
use std::net::SocketAddr;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use templar::*;

mod exchange_graph;

type HttpQuery<'a> = common::http_query::HttpQuery<&'a str, &'a str>;

enum ContentType<'a> {
    Static(&'a str),
    Template(String, HttpQuery<'a>, fn(HttpQuery<'a>) -> Result<Document>),
    ApiCall(HttpQuery<'a>, fn(HttpQuery<'a>) -> Result<String>),
}

impl<'a> ContentType<'a> {
    pub fn parse_uri(uri: &'a Uri) -> Result<Self> {
        use ContentType::*;

        // Skip front slash
        let path = &uri.path()[1..];
        let query = HttpQuery::parse(uri.query().unwrap_or_default());

        if path.starts_with("api/") {
            let api_path = &path["api/".len()..];
            Err(BoxErr::from(format!("Invalid api path: {}", api_path)))
        } else if path.contains(".template.html") {
            let path = path.to_string();
            match path.as_str().trim_end_matches(".template.html") {
                "balance_current" => Ok(Template(path, query, render_balance_current)),
                "balance_current_sim" => Ok(Template(path, query, render_balance_current_sim)),
                "balance_history_sim" => Ok(Template(path, query, render_balance_history_sim)),
                _ => Err(BoxErr::from(format!("Invalid app path: {}", path))),
            }
        } else {
            let is_safe_path = path
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
            if is_safe_path {
                Ok(Static(path))
            } else {
                Err(BoxErr::from(format!("Invalid path: {}", path)))
            }
        }
    }

    pub fn render(self) -> Result<Vec<u8>> {
        use ContentType::*;

        match self {
            Static(path) => read_bytes_from_file(path).map_err(Into::into),
            Template(path, query, f) => {
                let template_param = f(query)?;

                let template_content = read_string_from_file(path)?;
                let template = Templar::global().parse(&template_content)?;

                let context = StandardContext::new();
                context.set(template_param)?;

                let rendered = template.render(&context)?;
                Ok(rendered.into_bytes())
            }
            ApiCall(query, f) => f(query).map(String::into_bytes),
        }
    }
}

async fn handle(req: Request<Body>) -> Result<Response<Body>> {
    dotenv::dotenv().ok();

    let content = match ContentType::parse_uri(req.uri()).and_then(ContentType::render) {
        Ok(content) => content,
        Err(e) => {
            println!("{}", e);
            "<html><body>An error occurred during dealing with http request <a href=\"index.html\">index</a></body></html>"
            .as_bytes().to_vec()
        }
    };

    Ok(Response::new(Body::from(content)))
}

fn render_balance_current(query: HttpQuery<'_>) -> Result<Document> {
    let conn = env::var("DATABASE_URL")?.deref().apply(Conn::establish)?;

    let latest_timestamp: Stamp = {
        let id = schema::stamp::table
            .select(diesel::dsl::max(schema::stamp::stamp_id))
            .get_result::<Option<IdType>>(&conn)?
            .ok_opt("No timestamp exists")?;

        schema::stamp::table
            .filter(schema::stamp::stamp_id.eq(id))
            .first(&conn)?
    };

    let exchange_graph = construct_exchange_graph(&conn, latest_timestamp.stamp_id)?;

    let base_currency: Currency = {
        let base_symbol = query.get(&"fiat").unwrap_or(&"USDT");
        schema::currency::table
            .filter(schema::currency::symbol.eq(base_symbol))
            .first(&conn)?
    };

    let balances = schema::balance::table
        .inner_join(schema::currency::table)
        .filter(schema::balance::stamp_id.eq(latest_timestamp.stamp_id))
        .get_results::<(Balance, Currency)>(&conn)?;

    let mut data = Document::default();
    data["title"] = "Autotrader Dashboard".into();
    data["date"] = latest_timestamp.timestamp.to_string().into();
    data["conversion"]["name"] = base_currency.name.clone().into();
    data["conversion"]["symbol"] = base_currency.symbol.clone().into();

    data["balances"] = Document::Seq(
        balances
            .into_par_iter()
            .map(|(b, c)| {
                let total_balance = b.available + b.pending;
                let rate = exchange_graph.rate_between(b.currency_id, base_currency.currency_id);
                let map = [
                    ("name", c.name.clone()),
                    ("available", b.available.to_string()),
                    ("pending", b.pending.to_string()),
                    ("total", total_balance.to_string()),
                    ("symbol", c.symbol.clone()),
                    (
                        "rate",
                        rate.map(|r| r.to_string()).unwrap_or(String::from("???")),
                    ),
                    (
                        "conversion",
                        rate.map(|r| r * total_balance as f64)
                            .map(|amount| amount.to_string())
                            .unwrap_or(String::from("???")),
                    ),
                ]
                .iter()
                .map(|(k, v)| (Document::from(*k), Document::from(v)))
                .collect();
                Document::Map(map)
            })
            .collect::<Vec<_>>(),
    );

    Ok(data)
}

fn render_balance_current_sim(query: HttpQuery<'_>) -> Result<Document> {
    let conn = env::var("DATABASE_URL")?.deref().apply(Conn::establish)?;
    let sim_conn = env::var("SIM_DATABASE_URL")?
        .deref()
        .apply(Conn::establish)?;

    let latest_timestamp: Stamp = {
        let id = schema::stamp::table
            .select(diesel::dsl::max(schema::stamp::stamp_id))
            .get_result::<Option<IdType>>(&conn)?
            .ok_opt("No timestamp exists")?;

        schema::stamp::table
            .filter(schema::stamp::stamp_id.eq(id))
            .first(&conn)?
    };

    let exchange_graph = construct_exchange_graph(&conn, latest_timestamp.stamp_id)?;

    let base_currency: Currency = {
        let base_symbol = query.get(&"fiat").unwrap_or(&"USDT");
        schema::currency::table
            .filter(schema::currency::symbol.eq(base_symbol))
            .first(&conn)?
    };

    let currency_collection = list_currencies(&conn)?;
    let balances = schema::balance::table
        .filter(schema::balance::stamp_id.eq(latest_timestamp.stamp_id))
        .load::<Balance>(&sim_conn)?
        .into_iter()
        .filter_map(|b| {
            let currency = currency_collection.by_id(b.currency_id)?.clone();
            Some((b, currency))
        })
        .collect::<Vec<_>>();

    let mut data = Document::default();
    data["title"] = "Autotrader Dashboard".into();
    data["date"] = latest_timestamp.timestamp.to_string().into();
    data["conversion"]["name"] = base_currency.name.clone().into();
    data["conversion"]["symbol"] = base_currency.symbol.clone().into();

    data["balances"] = Document::Seq(
        balances
            .into_par_iter()
            .map(|(b, c)| {
                let total_balance = b.available + b.pending;
                let rate = exchange_graph.rate_between(b.currency_id, base_currency.currency_id);
                let map = [
                    ("name", c.name.clone()),
                    ("available", b.available.to_string()),
                    ("pending", b.pending.to_string()),
                    ("total", total_balance.to_string()),
                    ("symbol", c.symbol.clone()),
                    (
                        "rate",
                        rate.map(|r| r.to_string()).unwrap_or(String::from("???")),
                    ),
                    (
                        "conversion",
                        rate.map(|r| r * total_balance as f64)
                            .map(|amount| amount.to_string())
                            .unwrap_or(String::from("???")),
                    ),
                ]
                .iter()
                .map(|(k, v)| (Document::from(*k), Document::from(v)))
                .collect();
                Document::Map(map)
            })
            .collect::<Vec<_>>(),
    );

    Ok(data)
}

fn render_balance_history_sim(query: HttpQuery<'_>) -> Result<Document> {
    let conn = env::var("DATABASE_URL")?.deref().apply(Conn::establish)?;
    let sim_conn = env::var("SIM_DATABASE_URL")?
        .deref()
        .apply(Conn::establish)?;

    let stamps = {
        let limit = query
            .get(&"limit")
            .and_then(|limit| i64::from_str(limit).ok())
            .unwrap_or(10);

        schema::stamp::table
            .order(schema::stamp::stamp_id.desc())
            .limit(limit)
            .load::<Stamp>(&conn)?
            .also(|v| v.reverse())
    };

    let base_currency: Currency = {
        let base_symbol = query.get(&"fiat").unwrap_or(&"USDT");
        schema::currency::table
            .filter(schema::currency::symbol.eq(base_symbol))
            .first(&conn)?
    };

    let total_balance_history = stamps
        .into_iter()
        .filter_map(|stamp| {
            let exchange_graph = construct_exchange_graph(&conn, stamp.stamp_id).ok()?;
            let sim_balances = schema::balance::table
                .filter(schema::balance::stamp_id.eq(stamp.stamp_id))
                .load::<Balance>(&sim_conn)
                .ok()?;

            let sum = sim_balances
                .into_iter()
                .filter_map(|b| {
                    exchange_graph
                        .rate_between(b.currency_id, base_currency.currency_id)
                        .map(|rate| rate * (b.available + b.pending) as f64)
                })
                .sum::<f64>();

            Some((stamp, sum))
        })
        .map(|(stamp, sum)| {
            let map = vec![
                (
                    Document::from("stamp"),
                    Document::from(stamp.timestamp.to_string()),
                ),
                (Document::from("balance"), Document::from(sum)),
            ]
            .into_iter()
            .collect();
            Document::Map(map)
        })
        .collect::<Vec<_>>();

    let mut data = Document::default();
    data["title"] = "Balance history (simulation)".into();
    data["fiat"] = base_currency.symbol.into();
    data["history"] = Document::Seq(total_balance_history);
    Ok(data)
}

fn read_bytes_from_file<P: AsRef<Path>>(path: P) -> Result<Vec<u8>> {
    let path = env::var("WEBCONTENT_ROOT")?
        .deref()
        .apply(PathBuf::from)
        .apply_ref(|p| p.join(path));

    println!("Debug: path: {:?}", path);

    let mut file = std::fs::File::open(path)?;
    let mut bytes = vec![];

    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_string_from_file<P: AsRef<Path>>(path: P) -> Result<String> {
    read_bytes_from_file(path)?
        .apply(String::from_utf8)
        .map_err(Into::into)
}

fn construct_exchange_graph(conn: &Conn, timestamp_id: IdType) -> Result<ExchangeGraph<IdType>> {
    use schema::*;

    let prices = price::table
        .inner_join(market::table.on(price::market_id.eq(market::market_id)))
        .filter(price::stamp_id.eq(timestamp_id))
        .load::<(Price, Market)>(conn)?;

    prices
        .into_iter()
        .map(|(p, m)| (m.base_id, m.quote_id, p.amount as f64))
        .apply(ExchangeGraph::from_rates)
        .apply(Ok)
}

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 7878));

    let make_service = make_service_fn(|_conn| async { Result::Ok(service_fn(handle)) });

    let server = Server::bind(&addr).serve(make_service);

    // Run forever...
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
