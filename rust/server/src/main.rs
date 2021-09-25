use anyhow::{anyhow, ensure, Error, Result};
use apply::Apply;
use hyper::server::Server;
use hyper::service::*;
use hyper::{Body, Request, Response, Uri};
use json::JsonValue;
use qstring::QString;
use std::env;
use std::io::Read;
use std::net::SocketAddr;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
#[macro_use]
extern crate log;

mod api;
mod exchange_graph;

fn render(uri: &Uri) -> Result<Vec<u8>> {
    // Skip front slash
    let path = &uri.path()[1..];
    let query = QString::from(uri.query().unwrap_or_default());

    if path.starts_with("api/") {
        let api_path = &path["api/".len()..];
        render_api(api_path, &query).map(|json| json.to_string().into_bytes())
    } else {
        render_file(path)
    }
}

fn render_file(path: &str) -> Result<Vec<u8>> {
    let is_safe_path = path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-');
    ensure!(is_safe_path, "Invalid file path: {}", path);

    let path = env::var("WEBCONTENT_ROOT")?
        .deref()
        .apply(PathBuf::from)
        .apply_ref(|p| p.join(path));

    debug!("Read file: {:?}", path);

    let mut file = std::fs::File::open(path)?;
    let mut bytes = vec![];

    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn render_api(api_path: &str, query: &QString) -> Result<JsonValue> {
    match api_path {
        "balance_history" => api::api_balance_history(query),
        other => Err(anyhow!("Invalid api: {}", other)),
    }
}

async fn handle(req: Request<Body>) -> Result<Response<Body>> {
    let content = match render(req.uri()) {
        Ok(content) => content,
        Err(e) => {
            warn!("{}", e);
            "<html><body>An error occurred during parsing http request <a href=\"index.html\">index</a></body></html>"
            .as_bytes().to_vec()
        }
    };

    Ok(Response::new(Body::from(content)))
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::try_init().ok();

    let addr = match env::var("SERVER_ADDRESS")
        .map_err(Error::from)
        .and_then(|addr| SocketAddr::from_str(&addr).map_err(Error::from))
    {
        Ok(addr) => addr,
        Err(e) => {
            error!("Can't determine server address: {}", e);
            return;
        }
    };

    let make_service =
        make_service_fn(|_conn| async { Result::<_, Error>::Ok(service_fn(handle)) });

    let server = Server::bind(&addr).serve(make_service);

    // Run forever...
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
