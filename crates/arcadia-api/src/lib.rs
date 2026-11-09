//! Cliente do backend do Arcadia (auth/REST/RPC/storage) — port de
//! `app/electron/supabase/client.js` + `httpfetch.js` + `config.js`.
//!
//! Diferenças deliberadas em relação ao cliente congelado:
//!
//! - erros são valores tipados ([`Error`]) em vez de `{ data, error }`;
//! - JSON inválido vira [`Error::Decode`] com o corpo bruto preservado, nunca
//!   pânico;
//! - o builder REST oferece `order`, `upsert` e `return=representation`;
//! - uploads de storage enviam `content-type`/`x-upsert`.
//!
//! O fallback de endereço é idêntico ao do JS: só falha de rede troca de
//! endereço; 4xx/5xx nunca são repetidos.

pub mod auth;
pub mod config;
pub mod error;
pub mod http;
pub mod rest;
pub mod storage;

pub use auth::{
    AuthClient, AuthEvent, AuthStateChange, Session, SetSessionOptions, SignInResponse,
    SignUpResponse, USER_CACHE_TTL,
};
pub use config::{normalize_url, ApiConfig, DEFAULT_ANON_KEY, DEFAULT_API_URL, FALLBACK_API_URL};
pub use error::{ApiError, DecodeError, Error, Result, TransportError, TransportKind};
pub use http::{
    sanitize_header_name, sanitize_header_value, sanitize_headers, HttpClient, HttpRequest,
    RawResponse, DEFAULT_CONNECT_TIMEOUT, DEFAULT_HTTP_TIMEOUT,
};
pub use rest::{Order, QueryBuilder};
pub use storage::StorageBucket;

pub use reqwest::Method;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::auth::SetSessionOptions as SessionOptions;

/// Cliente principal: configuração + HTTP + auth + REST/RPC/storage.
#[derive(Debug, Clone)]
pub struct ApiClient {
    config: Arc<ApiConfig>,
    http: HttpClient,
    auth: AuthClient,
}

impl ApiClient {
    pub fn new(config: ApiConfig) -> std::result::Result<Self, TransportError> {
        Self::with_timeout(config, DEFAULT_HTTP_TIMEOUT)
    }

    pub fn with_timeout(
        config: ApiConfig,
        timeout: Duration,
    ) -> std::result::Result<Self, TransportError> {
        let config = Arc::new(config);
        let http = HttpClient::with_timeout(Arc::clone(&config), timeout)?;
        let auth = AuthClient::new(Arc::clone(&config), http.clone());
        Ok(Self { config, http, auth })
    }

    /// Configuração a partir do ambiente (`ARCADIA_API_URL` etc.).
    pub fn from_env() -> std::result::Result<Self, TransportError> {
        Self::new(ApiConfig::from_env())
    }

    pub fn config(&self) -> &ApiConfig {
        &self.config
    }

    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    pub fn auth(&self) -> &AuthClient {
        &self.auth
    }

    /// Query builder de `/rest/v1/{table}`.
    pub fn from(&self, table: impl Into<String>) -> QueryBuilder<'_> {
        QueryBuilder::new(self, table.into())
    }

    /// Storage de um bucket (`/storage/v1/object/{bucket}/...`).
    pub fn storage_from(&self, bucket: impl Into<String>) -> StorageBucket<'_> {
        StorageBucket::new(self, bucket.into())
    }

    /// `POST /rest/v1/rpc/{function}`; `null` vira `{}` como no JS.
    pub async fn rpc(&self, function: &str, args: Value) -> Result<Value> {
        let args = if args.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            args
        };
        let body = serde_json::to_vec(&args).map_err(|error| {
            Error::Decode(DecodeError {
                status: 0,
                message: error.to_string(),
                body: String::new(),
            })
        })?;
        let url = format!("{}/rest/v1/rpc/{}", self.config.url, function);
        let response = self
            .request_authenticated(
                Method::POST,
                url,
                Some(body),
                Some("application/json"),
                None,
            )
            .await?;

        if !response.is_success() {
            return Err(response.api_error().into());
        }
        if response.body.is_empty() {
            return Ok(Value::Null);
        }
        Ok(response.json::<Value>()?)
    }

    /// Login por username (RPC `login_check` + grant de senha), como `auth.js`.
    pub async fn sign_in_with_username(
        &self,
        username: &str,
        password: &str,
    ) -> Result<SignInResponse> {
        self.auth.sign_in_with_username(username, password).await
    }

    /// Instala/valida uma sessão salva, como `setSession` do JS.
    pub async fn set_session(&self, session: Session, options: SessionOptions) -> Result<Session> {
        self.auth.set_session(session, options).await
    }

    /// Executa uma requisição já montada, adicionando `apikey` e o bearer atual.
    /// Em 401 renova uma vez e repete a requisição original.
    pub(crate) async fn send_authenticated(&self, mut request: HttpRequest) -> Result<RawResponse> {
        let token_at_request = self.auth.access_token().await;
        self.apply_auth_headers(&mut request, token_at_request.as_deref());
        let mut response = self.http.send(&request).await?;

        if response.status == 401 {
            if let Some(current) = self.auth.get_session().await {
                if current.refresh_token().is_some() {
                    if token_at_request.as_deref() != Some(current.access_token.as_str()) {
                        // Outra tarefa já renovou: repete direto com o token novo.
                        self.apply_auth_headers(&mut request, Some(&current.access_token));
                        response = self.http.send(&request).await?;
                    } else {
                        match self.auth.refresh_session().await {
                            Ok(refreshed) => {
                                self.apply_auth_headers(
                                    &mut request,
                                    Some(&refreshed.access_token),
                                );
                                response = self.http.send(&request).await?;
                            }
                            // Falha de renovação: devolve o erro de auth e não
                            // mexe na sessão (nada é limpo silenciosamente).
                            Err(error) => return Err(error),
                        }
                    }
                }
            }
        }

        Ok(response)
    }

    pub(crate) async fn request_authenticated(
        &self,
        method: Method,
        url: String,
        body: Option<Vec<u8>>,
        content_type: Option<&str>,
        prefer: Option<&str>,
    ) -> Result<RawResponse> {
        let mut request = HttpRequest::new(method, url);
        if let Some(content_type) = content_type {
            request = request.header("content-type", content_type);
        }
        if let Some(prefer) = prefer {
            request = request.header("prefer", prefer);
        }
        if let Some(body) = body {
            request = request.body(body);
        }
        self.send_authenticated(request).await
    }

    fn apply_auth_headers(&self, request: &mut HttpRequest, token: Option<&str>) {
        request.headers.retain(|(name, _)| {
            !name.eq_ignore_ascii_case("apikey") && !name.eq_ignore_ascii_case("authorization")
        });
        request
            .headers
            .push(("apikey".to_string(), self.config.anon_key.clone()));
        if let Some(token) = token.filter(|token| !token.is_empty()) {
            request
                .headers
                .push(("authorization".to_string(), format!("Bearer {token}")));
        }
    }
}
