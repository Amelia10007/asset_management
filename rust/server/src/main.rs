use apply::{Also, Apply};
use common::alias::{BoxErr, Result};
use common::log::prelude::*;
use hyper::server::Server;
use hyper::service::*;
use hyper::{Body, Request, Response, Uri};
use json::JsonValue;
use once_cell::sync::Lazy;
use std::env;
use std::io::{stdout, Read, Stdout};
use std::net::SocketAddr;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;

mod api;
mod exchange_graph;

use crate::api::api_balance_history;

pub type HttpQuery<'a> = common::http_query::HttpQuery<&'a str, &'a str>;

static LOGGER: Lazy<Logger<Stdout>> = Lazy::new(|| {
    let level = match env::var("SERVER_LOGGER_LEVEL")
        .map(|s| s.to_lowercase())
        .as_deref()
    {
        Ok("error") => LogLevel::Error,
        Ok("warn") => LogLevel::Warning,
        Ok("info") => LogLevel::Info,
        Ok("debug") => LogLevel::Debug,
        _ => LogLevel::Debug,
    };
    Logger::new(stdout(), level)
});

enum ContentType<'a> {
    Static(&'a str),
    ApiCall(HttpQuery<'a>, fn(HttpQuery<'a>) -> Result<JsonValue>),
}

impl<'a> ContentType<'a> {
    pub fn parse_uri(uri: &'a Uri) -> Result<Self> {
        use ContentType::*;

        // Skip front slash
        let path = &uri.path()[1..];
        let query = HttpQuery::parse(uri.query().unwrap_or_default());

        if path.starts_with("api/") {
            let api_path = &path["api/".len()..];
            match api_path {
                "balance_history" => Ok(ApiCall(query, api_balance_history)),
                _ => Err(BoxErr::from(format!("Invalid api path: {}", api_path))),
            }
        } else {
            let is_safe_path = path
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-');
            if is_safe_path {
                Ok(Static(path))
            } else {
                Err(BoxErr::from(format!("Invalid path: {}", path)))
            }
        }
    }

    pub fn render(self) -> Vec<u8> {
        use ContentType::*;

        match self {
            Static(path) => match read_bytes_from_file(path) {
                Ok(content) => content,
                Err(e) => {
                    warn!(LOGGER, "Failed to load static file {}: {}", path, e);
                    "<html><body>An error occurred during dealing with http request <a href=\"index.html\">index</a></body></html>"
                    .as_bytes().to_vec()
                }
            },
            ApiCall(query, f) => {
                let json = match f(query) {
                    Ok(json) => json,
                    Err(e) => {
                        warn!(LOGGER, "API failure: {}", e);
                        JsonValue::new_object().also(|json| json["success"] = false.into())
                    }
                };
                json.to_string().into_bytes()
            }
        }
    }
}

async fn handle(req: Request<Body>) -> Result<Response<Body>> {
    let content = match ContentType::parse_uri(req.uri()).map(ContentType::render) {
        Ok(content) => content,
        Err(e) => {
            warn!(LOGGER, "{}", e);
            "<html><body>An error occurred during parsing http request <a href=\"index.html\">index</a></body></html>"
            .as_bytes().to_vec()
        }
    };

    Ok(Response::new(Body::from(content)))
}

fn read_bytes_from_file<P: AsRef<Path>>(path: P) -> Result<Vec<u8>> {
    let path = env::var("WEBCONTENT_ROOT")?
        .deref()
        .apply(PathBuf::from)
        .apply_ref(|p| p.join(path));

    debug!(LOGGER, "Read file: {:?}", path);

    let mut file = std::fs::File::open(path)?;
    let mut bytes = vec![];

    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    let addr = match env::var("SERVER_ADDRESS")
        .map_err(BoxErr::from)
        .and_then(|addr| SocketAddr::from_str(&addr).map_err(BoxErr::from))
    {
        Ok(addr) => addr,
        Err(e) => {
            error!(LOGGER, "Can't determine server address: {}", e);
            return;
        }
    };

    let make_service = make_service_fn(|_conn| async { Result::Ok(service_fn(handle)) });

    let server = Server::bind(&addr).serve(make_service);

    // Run forever...
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
