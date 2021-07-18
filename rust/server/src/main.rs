use apply::Apply;
use common::alias::Result;
use common::err::OkOpt;
use database::diesel::prelude::*;
use database::diesel::{self, QueryDsl, RunQueryDsl};
use database::logic::Conn;
use database::model::{self, IdType};
use database::schema;
use exchange_graph::ExchangeGraph;
use hyper::server::Server;
use hyper::service::*;
use hyper::{Body, Request, Response};
use rayon::prelude::*;
use std::env;
use std::io::Read;
use std::net::SocketAddr;
use templar::*;

mod exchange_graph;

async fn handle(req: Request<Body>) -> Result<Response<Body>> {
    dotenv::dotenv().ok();

    let conn = {
        let url = env::var("DATABASE_URL")?;
        Conn::establish(&url)?
    };

    let latest_timestamp: model::Stamp = {
        let id = schema::stamp::table
            .select(diesel::dsl::max(schema::stamp::stamp_id))
            .get_result::<Option<model::IdType>>(&conn)?
            .ok_opt("No timestamp exists")?;

        schema::stamp::table
            .filter(schema::stamp::stamp_id.eq(id))
            .first(&conn)?
    };

    let exchange_graph = construct_exchange_graph(&conn, latest_timestamp.stamp_id)?;

    let base_currency: model::Currency = {
        let base_symbol = req
            .uri()
            .query()
            .unwrap_or_default()
            .split('&')
            .find_map(|split| {
                let mut iter = split.split('=');
                match (iter.next(), iter.next()) {
                    (Some("fiat"), Some(base_symbol)) => Some(base_symbol),
                    _ => None,
                }
            })
            .unwrap_or("USDT");
        schema::currency::table
            .filter(schema::currency::symbol.eq(base_symbol))
            .first(&conn)?
    };

    let balances = schema::balance::table
        .inner_join(schema::currency::table)
        .filter(schema::balance::stamp_id.eq(latest_timestamp.stamp_id))
        .get_results::<(model::Balance, model::Currency)>(&conn)?;

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

    let mut file = std::fs::File::open("/home/mk/asset_management/WebContent/index.html")?;
    let mut template = String::new();
    file.read_to_string(&mut template)?;

    let template = Templar::global().parse(&template)?;

    let context = StandardContext::new();
    context.set(data)?;

    let rendered = template.render(&context)?;

    Ok(Response::new(Body::from(rendered)))
}

fn construct_exchange_graph(conn: &Conn, timestamp_id: IdType) -> Result<ExchangeGraph<IdType>> {
    use schema::*;

    let prices = price::table
        .inner_join(market::table.on(price::market_id.eq(market::market_id)))
        .filter(price::stamp_id.eq(timestamp_id))
        .load::<(model::Price, model::Market)>(conn)?;

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
