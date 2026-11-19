//! Storage REST do Supabase (`/storage/v1/object/...`) — port de
//! `StorageClient` de `client.js`, com `content-type` e `x-upsert` explícitos.

use reqwest::Method;
use serde_json::{json, Value};

use crate::error::{DecodeError, Error, Result};
use crate::ApiClient;

/// Bucket de storage ligado a um [`ApiClient`].
#[derive(Debug, Clone)]
pub struct StorageBucket<'a> {
    client: &'a ApiClient,
    bucket: String,
}

impl<'a> StorageBucket<'a> {
    pub(crate) fn new(client: &'a ApiClient, bucket: String) -> Self {
        Self { client, bucket }
    }

    pub fn name(&self) -> &str {
        &self.bucket
    }

    /// Envia um objeto: `POST /storage/v1/object/{bucket}/{path}`.
    ///
    /// O cliente JS original ignora as opções e não envia `content-type` nem
    /// `x-upsert`; aqui os dois são explícitos, como o backend espera.
    pub async fn upload(
        &self,
        path: &str,
        data: impl Into<Vec<u8>>,
        content_type: &str,
        upsert: bool,
    ) -> Result<Value> {
        let content_type = if content_type.is_empty() {
            "application/octet-stream"
        } else {
            content_type
        };
        let mut request = crate::http::HttpRequest::new(
            Method::POST,
            format!(
                "{}/storage/v1/object/{}/{}",
                self.client.config().url,
                self.bucket,
                path
            ),
        )
        .header("content-type", content_type)
        .body(data.into());

        if upsert {
            request = request.header("x-upsert", "true");
        }

        let response = self.client.send_authenticated(request).await?;
        if !response.is_success() {
            return Err(response.api_error().into());
        }
        if response.body.is_empty() {
            return Ok(Value::Object(serde_json::Map::new()));
        }
        Ok(response.json::<Value>()?)
    }

    /// Remove objetos: `DELETE /storage/v1/object/{bucket}` com `{"paths":[]}`.
    pub async fn remove(&self, paths: &[impl AsRef<str>]) -> Result<Value> {
        let paths: Vec<&str> = paths.iter().map(AsRef::as_ref).collect();
        let body = serde_json::to_vec(&json!({ "paths": paths })).map_err(|error| {
            Error::Decode(DecodeError {
                status: 0,
                message: error.to_string(),
                body: String::new(),
            })
        })?;
        let request = crate::http::HttpRequest::new(
            Method::DELETE,
            format!(
                "{}/storage/v1/object/{}",
                self.client.config().url,
                self.bucket
            ),
        )
        .header("content-type", "application/json")
        .body(body);

        let response = self.client.send_authenticated(request).await?;
        if !response.is_success() {
            return Err(response.api_error().into());
        }
        if response.body.is_empty() {
            return Ok(Value::Array(Vec::new()));
        }
        Ok(response.json::<Value>()?)
    }
}
