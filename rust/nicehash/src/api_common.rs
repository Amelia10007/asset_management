use apply::Apply;
use common::alias::Result;
use common::err::OkOpt;
use database::model::NaiveDateTime;
use json::JsonValue;
pub use reqwest::Method;
use reqwest::Url;
use std::env;

type HttpQuery = common::http_query::HttpQuery<String, String>;

#[derive(Debug, Clone)]
pub struct ApiKey {
    organization_id: String,
    key: String,
    secret_key: String,
}

impl ApiKey {
    pub fn new(organization_id: String, key: String, secret_key: String) -> Self {
        Self {
            organization_id,
            key,
            secret_key,
        }
    }

    /// Load `NICEHASH_ORGANIZATION_ID`, `NICEHASH_API_KEY`, and `NICEHASH_API_SECRET_KEY` environment variable,
    /// then return api key.
    pub fn from_env() -> std::result::Result<Self, std::env::VarError> {
        let organization_id = env::var("NICEHASH_ORGANIZATION_ID")?;
        let key = env::var("NICEHASH_API_KEY")?;
        let secret_key = env::var("NICEHASH_API_SECRET_KEY")?;

        Self::new(organization_id, key, secret_key).apply(Ok)
    }
}

pub struct PublicApi;

pub struct PrivateApi;

#[derive(Debug)]
pub struct ApiCallBuilder<T, M, P, Q, K> {
    api_type: T,
    method: M,
    api_path: P,
    query_collection: Q,
    api_key: K,
}

impl ApiCallBuilder<(), (), (), (), ()> {
    pub fn new() -> ApiCallBuilder<(), (), (), (), ()> {
        ApiCallBuilder {
            api_type: (),
            method: (),
            api_path: (),
            query_collection: (),
            api_key: (),
        }
    }
}

impl<M, P, Q, K> ApiCallBuilder<(), M, P, Q, K> {
    pub fn public_api(self) -> ApiCallBuilder<PublicApi, M, P, Q, K> {
        ApiCallBuilder {
            api_type: PublicApi,
            method: self.method,
            api_path: self.api_path,
            query_collection: self.query_collection,
            api_key: self.api_key,
        }
    }

    pub fn private_api(self) -> ApiCallBuilder<PrivateApi, M, P, Q, K> {
        ApiCallBuilder {
            api_type: PrivateApi,
            method: self.method,
            api_path: self.api_path,
            query_collection: self.query_collection,
            api_key: self.api_key,
        }
    }
}

impl<T, P, Q, K> ApiCallBuilder<T, (), P, Q, K> {
    pub fn method(self, method: Method) -> ApiCallBuilder<T, Method, P, Q, K> {
        ApiCallBuilder {
            api_type: self.api_type,
            method,
            api_path: self.api_path,
            query_collection: self.query_collection,
            api_key: self.api_key,
        }
    }
}

impl<T, M, Q, K> ApiCallBuilder<T, M, (), Q, K> {
    /// # Panics
    /// Panics if `path` does not start with '/'
    pub fn path(self, path: impl Into<String>) -> ApiCallBuilder<T, M, String, Q, K> {
        let path = path.into();
        assert!(path.starts_with('/'));

        ApiCallBuilder {
            api_type: self.api_type,
            method: self.method,
            api_path: path,
            query_collection: self.query_collection,
            api_key: self.api_key,
        }
    }
}

impl<T, M, P, K> ApiCallBuilder<T, M, P, (), K> {
    pub fn query<QK, QV>(
        self,
        query: impl IntoIterator<Item = (QK, QV)>,
    ) -> ApiCallBuilder<T, M, P, HttpQuery, K>
    where
        QK: ToString,
        QV: ToString,
    {
        let query_collection = query
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ApiCallBuilder {
            api_type: self.api_type,
            method: self.method,
            api_path: self.api_path,
            query_collection,
            api_key: self.api_key,
        }
    }

    pub fn query_empty(self) -> ApiCallBuilder<T, M, P, HttpQuery, K> {
        let query_collection = std::iter::empty().collect();
        ApiCallBuilder {
            api_type: self.api_type,
            method: self.method,
            api_path: self.api_path,
            query_collection,
            api_key: self.api_key,
        }
    }
}

impl<PrivateApi, M, P, Q> ApiCallBuilder<PrivateApi, M, P, Q, ()> {
    pub fn api_key(self, api_key: ApiKey) -> ApiCallBuilder<PrivateApi, M, P, Q, ApiKey> {
        ApiCallBuilder {
            api_type: self.api_type,
            method: self.method,
            api_path: self.api_path,
            query_collection: self.query_collection,
            api_key,
        }
    }
}

impl ApiCallBuilder<PublicApi, Method, String, HttpQuery, ()> {
    pub fn call(self) -> Result<JsonValue> {
        let url = build_url(&self.api_path)?;
        let client = reqwest::blocking::ClientBuilder::default().build()?;

        let req = client
            .request(self.method, url)
            .query(self.query_collection.as_slice())
            .build()?;

        // Get reponse
        client
            .execute(req)?
            .text()?
            .as_str()
            .apply_ref(json::parse)
            .map_err(Into::into)
    }
}

impl ApiCallBuilder<PrivateApi, Method, String, HttpQuery, ApiKey> {
    pub fn call(self) -> Result<JsonValue> {
        let url = build_url(&self.api_path)?;
        // Fetch timestamp
        let server_timestamp_millis = fetch_server_time()?.timestamp_millis();

        // Onetime phrase
        let nonce = uuid::Uuid::new_v4();
        let request_id = uuid::Uuid::new_v4();

        //
        let query = self.query_collection.build_query();
        let organization_id = &self.api_key.organization_id;
        let api_key = &self.api_key.key;
        let api_secret_key = &self.api_key.secret_key;

        // Digital signing
        let auth = {
            let input = format!(
                "{}\0{}\0{}\0\0{}\0\0{}\0{}\0{}",
                api_key,
                server_timestamp_millis,
                nonce,
                organization_id,
                self.method.as_str(),
                self.api_path,
                query
            );
            let signature = hmac_sha256::HMAC::mac(input.as_bytes(), api_secret_key.as_bytes())
                .iter()
                .map(|b| format!("{:02x}", b))
                .fold(String::new(), |acc, cur| acc + &cur);
            format!("{}:{}", api_key, signature)
        };

        //
        let client = reqwest::blocking::ClientBuilder::default().build()?;

        let req = client
            .request(self.method, url)
            .header("X-Time", server_timestamp_millis)
            .header("X-Nonce", nonce.to_string())
            .header("X-Organization-Id", organization_id)
            .header("X-Request-Id", request_id.to_string())
            .header("X-Auth", auth)
            .query(self.query_collection.as_slice())
            .build()?;

        // Get reponse
        client
            .execute(req)?
            .text()?
            .as_str()
            .apply_ref(json::parse)
            .map_err(Into::into)
    }
}

fn build_url(api_path: &str) -> Result<Url> {
    Url::parse("https://api2.nicehash.com")?
        .join(api_path)
        .map_err(Into::into)
}

pub fn fetch_server_time() -> Result<NaiveDateTime> {
    let json = ApiCallBuilder::new()
        .public_api()
        .method(Method::GET)
        .path("/api/v2/time")
        .query_empty()
        .call()?;

    let millis = json["serverTime"].as_u64().ok_opt("Invalid serverTime")?;
    let secs = millis / 1000;
    let nsecs = millis % 1000 * 1_000_000;
    let time = NaiveDateTime::from_timestamp(secs as i64, nsecs as u32);
    Ok(time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_server_time() {
        let time = fetch_server_time().unwrap();
        assert!(time.timestamp() > 0);
    }
}
