use apply::Apply;
use common::alias::Result;
use common::err::OkOpt;
use database::entity::*;
use database::AssetDatabase;
use exchange_graph::ExchangeGraph;
use hyper::server::Server;
use hyper::service::*;
use hyper::{Body, Request, Response};
use std::io::Read;
use std::net::SocketAddr;
use templar::*;

mod exchange_graph;

async fn handle(req: Request<Body>) -> Result<Response<Body>> {
    println!("{:?}", req);
    println!();
    let today = database::Date::today();

    let mut conn = database::connect_asset_database_as_app()?;
    let today_history = conn.histories_by_date(today)?;
    let mut exchange_graph = construct_exchange_graph(&mut conn, today)?;

    let base_asset = conn.asset_by_unit("JPY")?.ok_opt("No base exchange data")?;

    let mut data = Document::default();
    data["title"] = "Asset Management".into();
    data["date"] = today.to_string().into();
    data["conversion"]["name"] = base_asset.name.as_deref().unwrap_or("???").into();
    data["conversion"]["unit"] = base_asset.unit.as_deref().unwrap_or("???").into();

    data["assets"] = Document::Seq(
        today_history
            .iter()
            .map(|h| {
                let rate = exchange_graph.rate_between(base_asset.id, h.asset.id);
                let map = [
                    ("service", h.service.name.clone()),
                    ("asset", h.asset.name.clone().unwrap_or_default()),
                    ("amount", h.amount.amount.to_string()),
                    ("unit", h.asset.unit.clone().unwrap_or_default()),
                    (
                        "rate",
                        rate.map(|r| r.to_string()).unwrap_or(String::from("???")),
                    ),
                    (
                        "conversion",
                        rate.map(|r| r * h.amount.amount)
                            .map(|amount| amount.to_string())
                            .unwrap_or(String::from("???")),
                    ),
                ]
                .iter()
                .map(|(k, v)| (Document::from(*k), Document::from(v)))
                .collect::<std::collections::BTreeMap<_, _>>();
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

fn construct_exchange_graph<A: AssetDatabase>(
    connection: &mut A,
    date: Date,
) -> Result<ExchangeGraph<AssetId>> {
    connection
        .exchanges_by_date(date)?
        .into_iter()
        .map(|e| (e.base.id, e.target.id, e.rate.amount))
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
