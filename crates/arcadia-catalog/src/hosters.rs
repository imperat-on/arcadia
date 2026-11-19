//! Resolvedores de hosters e debrids — port de `app/electron/hosters.js`.
//!
//! O fluxo do cliente congelado é preservado:
//!
//! 1. resolvedores gratuitos primeiro (`gofile.io`, `pixeldrain`, `rootz.so`);
//! 2. debrids configurados como fallback, na ordem
//!    Real-Debrid → TorBox → AllDebrid → Premiumize;
//! 3. para links de página, `unrestrict`/`unlock`/`directdl` devolvem a URL
//!    direta; para magnet, o torrent é enviado ao debrid e o maior arquivo é
//!    selecionado (RD/AD) ou empacotado (TorBox `zip_link=true`).
//!
//! Tokens são lidos de um mapa com as mesmas chaves de `config.json`
//! (`realdebrid_token`, `torbox_token`, `alldebrid_token`,
//! `premiumize_token`). O `Debug` de [`DebridTokens`] e [`DebridStatus`] é
//! redigido: nenhuma chave aparece em logs.
//!
//! O oráculo `hosters.js` não consulta "conta" de debrid — ele só conhece
//! presença de token e os endpoints usados no download. Por isso a API de
//! status aqui é local ([`DebridStatus`]); endpoints de conta não foram
//! inventados.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::{encode_uri_component, form_urlencode, Fetcher, Request, Response};

/// User-Agent do TorBox e do download de fontes.
pub const ARCADIA_UA: &str = "arcadia";
/// UA de navegador usado por `ehBinario` e pelo rootz.
pub const BROWSER_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// Janela do webdl do TorBox (`Date.now() + 30000`).
pub const TORBOX_WEB_TIMEOUT: Duration = Duration::from_secs(30);
/// Intervalo do poll do webdl no JS (`setTimeout(..., 3000)`).
pub const TORBOX_WEB_INTERVAL: Duration = Duration::from_secs(3);
/// Janela do magnet (`MAGNET_POLL_LIMITE = 30 * 60 * 1000`).
pub const MAGNET_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Intervalo do poll de magnet no JS (`setTimeout(..., 4000)`).
pub const MAGNET_INTERVAL: Duration = Duration::from_secs(4);

/// Provedores debrid, na ordem de tentativa do cliente congelado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DebridProvider {
    RealDebrid,
    TorBox,
    AllDebrid,
    Premiumize,
}

impl DebridProvider {
    pub const ALL: [DebridProvider; 4] = [
        DebridProvider::RealDebrid,
        DebridProvider::TorBox,
        DebridProvider::AllDebrid,
        DebridProvider::Premiumize,
    ];

    /// Identificador usado pela aba Integrações (`chave`).
    pub fn id(self) -> &'static str {
        match self {
            DebridProvider::RealDebrid => "realdebrid",
            DebridProvider::TorBox => "torbox",
            DebridProvider::AllDebrid => "alldebrid",
            DebridProvider::Premiumize => "premiumize",
        }
    }

    /// Chave da configuração local.
    pub fn config_key(self) -> &'static str {
        match self {
            DebridProvider::RealDebrid => "realdebrid_token",
            DebridProvider::TorBox => "torbox_token",
            DebridProvider::AllDebrid => "alldebrid_token",
            DebridProvider::Premiumize => "premiumize_token",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            DebridProvider::RealDebrid => "Real-Debrid",
            DebridProvider::TorBox => "TorBox",
            DebridProvider::AllDebrid => "AllDebrid",
            DebridProvider::Premiumize => "Premiumize",
        }
    }

    pub fn from_config_key(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.config_key() == key)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|provider| provider.id() == id)
    }
}

impl fmt::Display for DebridProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.display_name())
    }
}

/// Tokens configurados, indexados pela chave de configuração.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct DebridTokens {
    values: HashMap<&'static str, String>,
}

impl DebridTokens {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lê `config.json` como `lerConfig()` + `String(cfg[chave] || "").trim()`.
    pub fn from_config_json(config: &Value) -> Self {
        let mut tokens = Self::default();
        for provider in DebridProvider::ALL {
            let Some(value) = config.get(provider.config_key()) else {
                continue;
            };
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(boolean) => boolean.to_string(),
                _ => String::new(),
            };
            let text = text.trim();
            if !text.is_empty() {
                tokens
                    .values
                    .insert(provider.config_key(), text.to_string());
            }
        }
        tokens
    }

    pub fn set(&mut self, provider: DebridProvider, token: impl Into<String>) {
        let token = token.into();
        let token = token.trim();
        if token.is_empty() {
            self.values.remove(provider.config_key());
        } else {
            self.values.insert(provider.config_key(), token.to_string());
        }
    }

    pub fn get(&self, provider: DebridProvider) -> Option<&str> {
        self.values.get(provider.config_key()).map(String::as_str)
    }

    pub fn is_configured(&self, provider: DebridProvider) -> bool {
        self.get(provider).is_some()
    }

    pub fn configured_providers(&self) -> Vec<DebridProvider> {
        DebridProvider::ALL
            .into_iter()
            .filter(|provider| self.is_configured(*provider))
            .collect()
    }

    pub fn has_any(&self) -> bool {
        !self.values.is_empty()
    }

    /// Valor mascarado no mesmo formato de `redigirSegredos` do `main.js`.
    pub fn masked(&self, provider: DebridProvider) -> Option<String> {
        self.get(provider).map(mask_token)
    }
}

impl fmt::Debug for DebridTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let configured: Vec<&str> = self
            .configured_providers()
            .into_iter()
            .map(DebridProvider::id)
            .collect();
        formatter
            .debug_struct("DebridTokens")
            .field("configured", &configured)
            .finish()
    }
}

/// Máscara de segredo: `abc•••xy` (ou `•••` para tokens curtos).
pub fn mask_token(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() > 8 {
        let start: String = chars.iter().take(3).collect();
        let end: String = chars.iter().skip(chars.len() - 2).collect();
        format!("{start}•••{end}")
    } else {
        "•••".to_string()
    }
}

/// Status local de um provedor (presença/máscara; sem rede).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebridStatus {
    pub provider: String,
    pub config_key: String,
    pub configured: bool,
    pub token_masked: Option<String>,
}

/// `temDebridConfigurado()` do `torrent.js`.
pub fn has_debrid_configured(config: &Value) -> bool {
    DebridProvider::ALL
        .into_iter()
        .any(|provider| DebridTokens::from_config_json(config).is_configured(provider))
}

/// Link direto resolvido; `headers` extra só o rootz usa.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedLink {
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
}

impl ResolvedLink {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
        }
    }

    pub fn with_headers(url: impl Into<String>, headers: Vec<(String, String)>) -> Self {
        Self {
            url: url.into(),
            headers,
        }
    }
}

/// Erro de resolução. A mensagem agregada segue o `|` do JS.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DebridError {
    #[error("{0}")]
    Provider(String),
    #[error("{0}: token nao configurado")]
    NotConfigured(&'static str),
}

/// Janela de polling (intervalo + prazo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollWindow {
    pub interval: Duration,
    pub timeout: Duration,
}

impl PollWindow {
    pub const fn new(interval: Duration, timeout: Duration) -> Self {
        Self { interval, timeout }
    }

    /// Quantas tentativas cabem na janela. Sempre pelo menos uma, como o
    /// `while (Date.now() < limite)` do JS na primeira volta.
    pub fn attempts(&self) -> usize {
        let interval = self.interval.as_millis();
        if interval == 0 {
            return 1;
        }
        ((self.timeout.as_millis() / interval) + 1).min(100_000) as usize
    }
}

impl Default for PollWindow {
    fn default() -> Self {
        Self::new(MAGNET_INTERVAL, MAGNET_TIMEOUT)
    }
}

/// Dorme entre tentativas; testes injetam [`NoopSleeper`].
#[async_trait]
pub trait Sleeper: Send + Sync {
    async fn sleep(&self, duration: Duration);
}

/// Produção: `setTimeout` equivalente.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioSleeper;

#[async_trait]
impl Sleeper for TokioSleeper {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// Testes: não espera.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopSleeper;

#[async_trait]
impl Sleeper for NoopSleeper {
    async fn sleep(&self, _duration: Duration) {}
}

/// Cancelamento cooperativo (equivalente ao `AbortSignal`).
pub trait AbortSignal: Send + Sync {
    fn is_aborted(&self) -> bool;
}

/// Nunca cancelado.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverAbort;

impl AbortSignal for NeverAbort {
    fn is_aborted(&self) -> bool {
        false
    }
}

/// Cancelamento acionável por outra thread (pause/cancel do usuário).
#[derive(Debug, Clone, Default)]
pub struct AtomicAbort {
    flag: Arc<AtomicBool>,
}

impl AtomicAbort {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn abort(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn reset(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

impl AbortSignal for AtomicAbort {
    fn is_aborted(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// Bases HTTP; os defaults são exatamente os do `hosters.js`. Testes trocam
/// por um servidor local.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebridEndpoints {
    pub real_debrid: String,
    pub tor_box: String,
    pub all_debrid: String,
    pub premiumize: String,
    pub gofile_cdn: String,
    pub pixeldrain_bypass: String,
    pub pixeldrain_api: String,
    pub rootz_web: String,
}

impl Default for DebridEndpoints {
    fn default() -> Self {
        Self {
            real_debrid: "https://api.real-debrid.com".into(),
            tor_box: "https://api.torbox.app".into(),
            all_debrid: "https://api.alldebrid.com".into(),
            premiumize: "https://www.premiumize.me".into(),
            gofile_cdn: "https://gofilecdn.eu.cc".into(),
            pixeldrain_bypass: "https://cdn.pixeldrain.eu.cc".into(),
            pixeldrain_api: "https://pixeldrain.com/api/file".into(),
            rootz_web: "https://www.rootz.so".into(),
        }
    }
}

/// Cliente de hosters/debrids com HTTP, relógio de poll e tokens injetados.
pub struct DebridClient {
    fetcher: Arc<dyn Fetcher>,
    tokens: RwLock<DebridTokens>,
    endpoints: DebridEndpoints,
    sleeper: Arc<dyn Sleeper>,
    webdl_window: PollWindow,
    magnet_window: PollWindow,
}

impl DebridClient {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            fetcher,
            tokens: RwLock::new(DebridTokens::new()),
            endpoints: DebridEndpoints::default(),
            sleeper: Arc::new(TokioSleeper),
            webdl_window: PollWindow::new(TORBOX_WEB_INTERVAL, TORBOX_WEB_TIMEOUT),
            magnet_window: PollWindow::new(MAGNET_INTERVAL, MAGNET_TIMEOUT),
        }
    }

    pub fn with_tokens(fetcher: Arc<dyn Fetcher>, tokens: DebridTokens) -> Self {
        let client = Self::new(fetcher);
        *client.lock_tokens_mut() = tokens;
        client
    }

    pub fn from_config_json(fetcher: Arc<dyn Fetcher>, config: &Value) -> Self {
        Self::with_tokens(fetcher, DebridTokens::from_config_json(config))
    }

    pub fn with_endpoints(mut self, endpoints: DebridEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    pub fn with_sleeper(mut self, sleeper: Arc<dyn Sleeper>) -> Self {
        self.sleeper = sleeper;
        self
    }

    pub fn with_poll_windows(mut self, webdl: PollWindow, magnet: PollWindow) -> Self {
        self.webdl_window = webdl;
        self.magnet_window = magnet;
        self
    }

    pub fn fetcher(&self) -> &Arc<dyn Fetcher> {
        &self.fetcher
    }

    pub fn endpoints(&self) -> &DebridEndpoints {
        &self.endpoints
    }

    pub fn tokens(&self) -> DebridTokens {
        self.lock_tokens().clone()
    }

    pub fn set_tokens(&self, tokens: DebridTokens) {
        *self.lock_tokens_mut() = tokens;
    }

    pub fn set_config_json(&self, config: &Value) {
        self.set_tokens(DebridTokens::from_config_json(config));
    }

    /// `true` se qualquer provedor tem token (gate do renderer).
    pub fn configured(&self) -> bool {
        self.lock_tokens().has_any()
    }

    pub fn statuses(&self) -> Vec<DebridStatus> {
        let tokens = self.lock_tokens();
        DebridProvider::ALL
            .into_iter()
            .map(|provider| DebridStatus {
                provider: provider.id().to_string(),
                config_key: provider.config_key().to_string(),
                configured: tokens.is_configured(provider),
                token_masked: tokens.masked(provider),
            })
            .collect()
    }

    fn lock_tokens(&self) -> std::sync::RwLockReadGuard<'_, DebridTokens> {
        self.tokens
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_tokens_mut(&self) -> std::sync::RwLockWriteGuard<'_, DebridTokens> {
        self.tokens
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    // ── entrada pública ────────────────────────────────────────────────────

    /// Resolve a página de um hoster. Ordem: gratuito → debrids. `Ok(None)`
    /// quando nenhum resolvedor conhece o host e não há token configurado.
    pub async fn resolve_hoster(&self, url: &str) -> Result<Option<ResolvedLink>, DebridError> {
        let free: Option<ResolvedLink> = if gofile_regex().is_match(url) {
            self.gofile(url).await?
        } else if pixeldrain_regex().is_match(url) {
            self.pixeldrain(url).await?
        } else if rootz_regex().is_match(url) {
            self.rootz(url).await?
        } else {
            None
        };
        if let Some(link) = free {
            if !link.url.is_empty() {
                return Ok(Some(link));
            }
        }
        self.tentar_debrids(url).await
    }

    /// Magnet via debrid. `Ok(None)` = nenhum token configurado **ou**
    /// cancelado; `Err` = todos os configurados falharam.
    pub async fn resolve_magnet(
        &self,
        magnet: &str,
        abort: &dyn AbortSignal,
    ) -> Result<Option<ResolvedLink>, DebridError> {
        let tokens = self.tokens();
        let mut errors = Vec::new();
        for provider in DebridProvider::ALL {
            let Some(token) = tokens.get(provider) else {
                continue;
            };
            let result = match provider {
                DebridProvider::RealDebrid => self.rd_magnet(magnet, token, abort).await,
                DebridProvider::TorBox => self.tb_magnet(magnet, token, abort).await,
                DebridProvider::AllDebrid => self.ad_magnet(magnet, token, abort).await,
                DebridProvider::Premiumize => self.premiumize(magnet, token).await,
            };
            match result {
                Ok(link) if !link.url.is_empty() => return Ok(Some(link)),
                Ok(_) => {}
                Err(error) => errors.push(error.to_string()),
            }
            if abort.is_aborted() {
                return Ok(None);
            }
        }
        if errors.is_empty() {
            Ok(None)
        } else {
            Err(DebridError::Provider(errors.join(" | ")))
        }
    }

    /// Unrestrict direto de um provedor específico (usado por testes e por
    /// integrações que já sabem qual conta usar).
    pub async fn unrestrict(
        &self,
        provider: DebridProvider,
        url: &str,
    ) -> Result<ResolvedLink, DebridError> {
        let tokens = self.tokens();
        let token = tokens
            .get(provider)
            .ok_or(DebridError::NotConfigured(provider.display_name()))?;
        match provider {
            DebridProvider::RealDebrid => self.real_debrid(url, token).await,
            DebridProvider::TorBox => self.tor_box(url, token).await,
            DebridProvider::AllDebrid => self.all_debrid(url, token).await,
            DebridProvider::Premiumize => self.premiumize(url, token).await,
        }
    }

    // ── debrids: link de página ────────────────────────────────────────────

    async fn tentar_debrids(&self, url: &str) -> Result<Option<ResolvedLink>, DebridError> {
        let tokens = self.tokens();
        let mut errors = Vec::new();
        for provider in DebridProvider::ALL {
            let Some(token) = tokens.get(provider) else {
                continue;
            };
            let result = match provider {
                DebridProvider::RealDebrid => self.real_debrid(url, token).await,
                DebridProvider::TorBox => self.tor_box(url, token).await,
                DebridProvider::AllDebrid => self.all_debrid(url, token).await,
                DebridProvider::Premiumize => self.premiumize(url, token).await,
            };
            match result {
                Ok(link) if !link.url.is_empty() => return Ok(Some(link)),
                Ok(_) => {}
                Err(error) => errors.push(error.to_string()),
            }
        }
        if errors.is_empty() {
            Ok(None)
        } else {
            Err(DebridError::Provider(errors.join(" | ")))
        }
    }

    async fn real_debrid(&self, url: &str, token: &str) -> Result<ResolvedLink, DebridError> {
        let endpoint = format!("{}/rest/1.0/unrestrict/link", self.endpoints.real_debrid);
        let request = Request::post(endpoint)
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("link={}", encode_uri_component(url)))
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "Real-Debrid").await?;
        if !response.is_success() {
            return Err(DebridError::Provider(format!(
                "Real-Debrid: HTTP {} (token inválido ou conta expirada?)",
                response.status
            )));
        }
        let json = self.json(response, "Real-Debrid")?;
        match truthy_string(json.get("download")) {
            Some(download) => Ok(ResolvedLink::new(download)),
            None => Err(DebridError::Provider(
                "Real-Debrid não liberou este link (hoster fora do catálogo deles)".into(),
            )),
        }
    }

    async fn all_debrid(&self, url: &str, token: &str) -> Result<ResolvedLink, DebridError> {
        let endpoint = format!(
            "{}/v4/link/unlock?agent=arcadia&apikey={}",
            self.endpoints.all_debrid,
            encode_uri_component(token)
        );
        let request = Request::post(endpoint)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("link={}", encode_uri_component(url)))
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "AllDebrid").await?;
        let status = response.status;
        let json = self.json(response, "AllDebrid")?;
        let success = json.get("status").and_then(Value::as_str) == Some("success");
        let link = json.get("data").and_then(|data| data.get("link"));
        match (success, truthy_string(link)) {
            (true, Some(link)) => Ok(ResolvedLink::new(link)),
            _ => {
                let message = json
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(|message| truthy_string(Some(message)))
                    .unwrap_or_else(|| format!("HTTP {status}"));
                Err(DebridError::Provider(format!("AllDebrid: {message}")))
            }
        }
    }

    async fn premiumize(&self, url: &str, token: &str) -> Result<ResolvedLink, DebridError> {
        let endpoint = format!(
            "{}/api/transfer/directdl?apikey={}",
            self.endpoints.premiumize,
            encode_uri_component(token)
        );
        let request = Request::post(endpoint)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("src={}", encode_uri_component(url)))
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "Premiumize").await?;
        let status = response.status;
        let json = self.json(response, "Premiumize")?;
        let success = json.get("status").and_then(Value::as_str) == Some("success");
        let link = json
            .get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|first| first.get("link"));
        match (success, truthy_string(link)) {
            (true, Some(link)) => Ok(ResolvedLink::new(link)),
            _ => {
                let message =
                    truthy_string(json.get("message")).unwrap_or_else(|| format!("HTTP {status}"));
                Err(DebridError::Provider(format!("Premiumize: {message}")))
            }
        }
    }

    async fn tor_box(&self, url: &str, token: &str) -> Result<ResolvedLink, DebridError> {
        let (body, content_type) = multipart_body(&[("link", url)]);
        let endpoint = format!("{}/v1/api/webdl/createwebdownload", self.endpoints.tor_box);
        let request = Request::post(endpoint)
            .headers(torbox_auth(token))
            .header("content-type", content_type)
            .body(body)
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "TorBox").await?;
        let status = response.status;
        let json = self.json(response, "TorBox")?;
        let web_id = if json
            .get("success")
            .is_some_and(|value| crate::http::js_truthy(Some(value)))
        {
            json.get("data").and_then(|data| data.get("webdownload_id"))
        } else {
            None
        };
        let Some(web_id) = web_id.filter(|value| crate::http::js_truthy(Some(value))) else {
            let detail = first_truthy_string(&json, &["detail", "error"])
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(DebridError::Provider(format!("TorBox: {detail}")));
        };

        // mylist devolve a LISTA inteira (o ?id= não filtra toda conta): o item
        // é encontrado pelo id, como no Hydra.
        let list_url = format!(
            "{}/v1/api/webdl/mylist?id={}",
            self.endpoints.tor_box,
            js_scalar(web_id)
        );
        let web_id_for_poll = web_id.clone();
        let file_id = self
            .poll_until(self.webdl_window, &NeverAbort, || {
                let list_url = list_url.clone();
                let web_id = web_id_for_poll.clone();
                async move {
                    let request = Request::get(list_url)
                        .headers(torbox_auth(token))
                        .timeout(REQUEST_TIMEOUT);
                    let response = self.send(request, "TorBox").await.ok()?;
                    let json = response.json_value().ok()?;
                    let info = find_torbox_item(json.get("data"), &web_id)?;
                    if !torbox_completed(&info) {
                        return None;
                    }
                    Some(torbox_largest_file(&info))
                }
            })
            .await;

        let Some(file_id) = file_id.flatten() else {
            return Err(DebridError::Provider(
                "TorBox: link ainda não cacheado (tente de novo em alguns minutos)".into(),
            ));
        };

        let endpoint = format!(
            "{}/v1/api/webdl/requestdl?token={}&web_id={}&file_id={}",
            self.endpoints.tor_box,
            encode_uri_component(token),
            js_scalar(web_id),
            js_scalar(&file_id)
        );
        let request = Request::get(endpoint)
            .headers(torbox_auth(token))
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "TorBox").await?;
        let json = self.json(response, "TorBox")?;
        match truthy_string(json.get("data")) {
            Some(link) => Ok(ResolvedLink::new(link)),
            None => {
                let detail = first_truthy_string(&json, &["detail"])
                    .unwrap_or_else(|| "requestdl sem URL".into());
                Err(DebridError::Provider(format!("TorBox: {detail}")))
            }
        }
    }

    // ── magnets ────────────────────────────────────────────────────────────

    async fn rd_magnet(
        &self,
        magnet: &str,
        token: &str,
        abort: &dyn AbortSignal,
    ) -> Result<ResolvedLink, DebridError> {
        let endpoint = format!("{}/rest/1.0/torrents/addMagnet", self.endpoints.real_debrid);
        let request = Request::post(endpoint)
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("magnet={}", encode_uri_component(magnet)))
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "Real-Debrid addMagnet").await?;
        let status = response.status;
        let json = self.json(response, "Real-Debrid addMagnet")?;
        let Some(id) = json
            .get("id")
            .filter(|value| crate::http::js_truthy(Some(value)))
        else {
            return Err(DebridError::Provider(format!(
                "Real-Debrid addMagnet: HTTP {status}"
            )));
        };
        let id_text = js_scalar(id);

        let select = format!(
            "{}/rest/1.0/torrents/selectFiles/{}",
            self.endpoints.real_debrid, id_text
        );
        let request = Request::post(select)
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/x-www-form-urlencoded")
            .body("files=all")
            .timeout(REQUEST_TIMEOUT);
        let _ = self.send(request, "Real-Debrid selectFiles").await?;

        let info_url = format!(
            "{}/rest/1.0/torrents/info/{}",
            self.endpoints.real_debrid, id_text
        );
        let info = self
            .poll_until(self.magnet_window, abort, || {
                let info_url = info_url.clone();
                async move {
                    let request = Request::get(info_url)
                        .header("authorization", format!("Bearer {token}"))
                        .timeout(REQUEST_TIMEOUT);
                    let response = self.send(request, "Real-Debrid").await.ok()?;
                    let json = response.json_value().ok()?;
                    if json.get("status").and_then(Value::as_str) == Some("downloaded") {
                        Some(json)
                    } else {
                        None
                    }
                }
            })
            .await;
        let Some(info) = info else {
            return Err(DebridError::Provider(
                "Real-Debrid: torrent não cacheou em 30min".into(),
            ));
        };

        let files = info
            .get("files")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let selected: Vec<Value> = files
            .iter()
            .filter(|file| crate::http::js_truthy(file.get("selected")))
            .cloned()
            .collect();
        if selected.is_empty() {
            return Err(DebridError::Provider(
                "Real-Debrid: nenhum arquivo no torrent".into(),
            ));
        }
        // Maior arquivo selecionado = o jogo; links[] segue a ordem dos
        // arquivos selecionados no JSON original.
        let mut ordered = selected.clone();
        ordered.sort_by(|a, b| {
            number(b.get("bytes"))
                .partial_cmp(&number(a.get("bytes")))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let largest = ordered.first().cloned().expect("selecionado não vazio");
        let index = selected
            .iter()
            .position(|file| file == &largest)
            .unwrap_or(0);
        let links = info
            .get("links")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let link = links
            .get(index)
            .and_then(|value| truthy_string(Some(value)))
            .or_else(|| links.first().and_then(|value| truthy_string(Some(value))));
        let Some(link) = link else {
            return Err(DebridError::Provider(
                "Real-Debrid: sem link para o arquivo".into(),
            ));
        };
        self.real_debrid(&link, token).await
    }

    async fn tb_magnet(
        &self,
        magnet: &str,
        token: &str,
        abort: &dyn AbortSignal,
    ) -> Result<ResolvedLink, DebridError> {
        let (body, content_type) = multipart_body(&[("magnet", magnet)]);
        let endpoint = format!("{}/v1/api/torrents/createtorrent", self.endpoints.tor_box);
        let request = Request::post(endpoint)
            .headers(torbox_auth(token))
            .header("content-type", content_type)
            .body(body)
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "TorBox createtorrent").await?;
        let json = self.json(response, "TorBox createtorrent")?;
        let torrent_id = if json
            .get("success")
            .is_some_and(|value| crate::http::js_truthy(Some(value)))
        {
            json.get("data").and_then(|data| data.get("torrent_id"))
        } else {
            None
        };
        let Some(torrent_id) = torrent_id.filter(|value| crate::http::js_truthy(Some(value)))
        else {
            let detail = first_truthy_string(&json, &["detail"]).unwrap_or_else(|| "falha".into());
            return Err(DebridError::Provider(format!(
                "TorBox createtorrent: {detail}"
            )));
        };
        let id_text = js_scalar(torrent_id);

        let list_url = format!(
            "{}/v1/api/torrents/mylist?id={}",
            self.endpoints.tor_box, id_text
        );
        let torrent_id_for_poll = torrent_id.clone();
        let ready = self
            .poll_until(self.magnet_window, abort, || {
                let list_url = list_url.clone();
                let torrent_id = torrent_id_for_poll.clone();
                async move {
                    let request = Request::get(list_url)
                        .headers(torbox_auth(token))
                        .timeout(REQUEST_TIMEOUT);
                    let response = self.send(request, "TorBox").await.ok()?;
                    let json = response.json_value().ok()?;
                    let info = find_torbox_item(json.get("data"), &torrent_id)?;
                    if torbox_completed(&info) {
                        Some(())
                    } else {
                        None
                    }
                }
            })
            .await;
        if ready.is_none() {
            return Err(DebridError::Provider(
                "TorBox: torrent não cacheou em 30min".into(),
            ));
        }

        let endpoint = format!(
            "{}/v1/api/torrents/requestdl?token={}&torrent_id={}&zip_link=true",
            self.endpoints.tor_box,
            encode_uri_component(token),
            id_text
        );
        let request = Request::get(endpoint)
            .headers(torbox_auth(token))
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "TorBox requestdl").await?;
        let json = self.json(response, "TorBox requestdl")?;
        match truthy_string(json.get("data")) {
            Some(link) => Ok(ResolvedLink::new(link)),
            None => {
                let detail =
                    first_truthy_string(&json, &["detail"]).unwrap_or_else(|| "sem URL".into());
                Err(DebridError::Provider(format!("TorBox requestdl: {detail}")))
            }
        }
    }

    async fn ad_magnet(
        &self,
        magnet: &str,
        token: &str,
        abort: &dyn AbortSignal,
    ) -> Result<ResolvedLink, DebridError> {
        let upload = self
            .ad_post("v4.1/magnet/upload", token, &[("magnets[]", magnet)])
            .await?;
        let Some(id) = first_ad_magnet(&upload, "magnets")
            .and_then(|magnet| magnet.get("id"))
            .filter(|id| crate::http::js_truthy(Some(id)))
        else {
            return Err(DebridError::Provider("AllDebrid: upload sem id".into()));
        };
        let id = id.clone();
        let id_text = js_scalar(&id);

        let status_id = id_text.clone();
        let ready = self
            .poll_until(self.magnet_window, abort, || {
                let status_id = status_id.clone();
                async move {
                    let data = self
                        .ad_post("v4.1/magnet/status", token, &[("id", &status_id)])
                        .await
                        .ok()?;
                    let magnet = first_ad_magnet(&data, "magnets")?;
                    let status = magnet.get("status").and_then(Value::as_str);
                    let code = magnet.get("statusCode").and_then(Value::as_i64);
                    if status == Some("Ready") || code == Some(4) {
                        Some(())
                    } else {
                        None
                    }
                }
            })
            .await;
        if ready.is_none() {
            return Err(DebridError::Provider(
                "AllDebrid: torrent não cacheou em 30min".into(),
            ));
        }

        let files_id = id_text;
        let files = match self
            .ad_post("v4.1/magnet/files", token, &[("id[]", &files_id)])
            .await
        {
            Ok(data) => data,
            Err(_) => {
                self.ad_post("v4/magnet/files", token, &[("id[]", &files_id)])
                    .await?
            }
        };
        let magnet = first_ad_magnet(&files, "magnets");
        let nodes = magnet.and_then(|magnet| magnet.get("files"));
        let mut collected = Vec::new();
        collect_ad_files(nodes, &mut collected);
        if collected.is_empty() {
            return Err(DebridError::Provider(
                "AllDebrid: sem arquivos no torrent".into(),
            ));
        }
        collected.sort_by(|a, b| {
            b.size
                .partial_cmp(&a.size)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let largest = collected.first().expect("não vazio");
        self.all_debrid(&largest.link, token).await
    }

    async fn ad_post(
        &self,
        endpoint: &str,
        token: &str,
        params: &[(&str, &str)],
    ) -> Result<Value, DebridError> {
        let url = format!(
            "{}/{}?agent=arcadia&apikey={}",
            self.endpoints.all_debrid,
            endpoint,
            encode_uri_component(token)
        );
        let mut fields = vec![("agent", "arcadia")];
        fields.extend_from_slice(params);
        let body = form_urlencode(&fields);
        let request = Request::post(url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "AllDebrid").await?;
        let status = response.status;
        let json = self.json(response, "AllDebrid")?;
        if json.get("status").and_then(Value::as_str) != Some("success") {
            let message = json
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(|message| truthy_string(Some(message)))
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(DebridError::Provider(format!("AllDebrid: {message}")));
        }
        Ok(json.get("data").cloned().unwrap_or(Value::Null))
    }

    // ── resolvedores gratuitos ─────────────────────────────────────────────

    async fn gofile(&self, url: &str) -> Result<Option<ResolvedLink>, DebridError> {
        let id = match url_segment(url, 1) {
            Some(segment) => segment,
            None => return Ok(None),
        };
        let cdn = format!(
            "{}/{}",
            self.endpoints.gofile_cdn,
            encode_uri_component(&id)
        );
        if self.eh_binario(&cdn, &[]).await {
            Ok(Some(ResolvedLink::new(cdn)))
        } else {
            Ok(None)
        }
    }

    async fn pixeldrain(&self, url: &str) -> Result<Option<ResolvedLink>, DebridError> {
        let Some(first) = url_segment(url, 0) else {
            return Ok(None);
        };
        let Some(id) = url_segment(url, 1) else {
            return Ok(None);
        };
        if first != "u" {
            return Ok(None);
        }
        let bypass = format!("{}/{}", self.endpoints.pixeldrain_bypass, id);
        if self.eh_binario(&bypass, &[]).await {
            return Ok(Some(ResolvedLink::new(bypass)));
        }
        Ok(Some(ResolvedLink::new(format!(
            "{}/{}?download",
            self.endpoints.pixeldrain_api, id
        ))))
    }

    async fn rootz(&self, url: &str) -> Result<Option<ResolvedLink>, DebridError> {
        let Some(first) = url_segment(url, 0) else {
            return Ok(None);
        };
        let Some(id) = url_segment(url, 1) else {
            return Ok(None);
        };
        if first != "d" {
            return Ok(None);
        }
        let id = id.to_string();
        let page_url = format!("{}/d/{}", self.endpoints.rootz_web, id);
        let request = Request::get(page_url.clone())
            .header("user-agent", BROWSER_UA)
            .header("accept", "text/html")
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "rootz").await?;
        let html = response.text_body();
        let token = rootz_token_regex()
            .captures(&html)
            .and_then(|captures| captures.get(1))
            .map(|capture| capture.as_str().to_string());
        let Some(token) = token else {
            return Err(DebridError::Provider(
                "rootz: pageToken não encontrado".into(),
            ));
        };

        let endpoint = format!(
            "{}/api/files/download-by-short?shortId={}",
            self.endpoints.rootz_web,
            encode_uri_component(&id)
        );
        let request = Request::get(endpoint)
            .header("user-agent", BROWSER_UA)
            .header("accept", "application/json")
            .header("referer", page_url.clone())
            .header("x-page-token", token)
            .timeout(REQUEST_TIMEOUT);
        let response = self.send(request, "rootz").await?;
        let json = self.json(response, "rootz")?;
        let data = json.get("data");
        let success = json
            .get("success")
            .is_some_and(|value| crate::http::js_truthy(Some(value)));
        let file_id = data
            .and_then(|data| data.get("fileId"))
            .filter(|value| crate::http::js_truthy(Some(value)));
        let Some(file_id) = file_id.filter(|_| success) else {
            let message = truthy_string(json.get("error"))
                .unwrap_or_else(|| "rootz: arquivo não encontrado".into());
            return Err(DebridError::Provider(message));
        };
        if let Some(status) = data
            .and_then(|data| data.get("status"))
            .and_then(|status| truthy_string(Some(status)))
        {
            if status != "active" {
                return Err(DebridError::Provider(format!("rootz: arquivo {status}")));
            }
        }
        if data
            .and_then(|data| data.get("downloadAllowed"))
            .and_then(Value::as_bool)
            == Some(false)
        {
            return Err(DebridError::Provider(
                "rootz: download não permitido".into(),
            ));
        }
        let file_id = js_scalar(file_id);
        Ok(Some(ResolvedLink::with_headers(
            format!(
                "{}/api/files/proxy-download/{}",
                self.endpoints.rootz_web, file_id
            ),
            vec![("Referer".into(), page_url)],
        )))
    }

    async fn eh_binario(&self, url: &str, extra: &[(String, String)]) -> bool {
        let mut headers = vec![
            ("user-agent".to_string(), BROWSER_UA.to_string()),
            ("range".to_string(), "bytes=0-0".to_string()),
        ];
        headers.extend(extra.iter().cloned());
        let request = Request::get(url).headers(headers).timeout(PROBE_TIMEOUT);
        let response = match self.fetcher.fetch(request).await {
            Ok(response) => response,
            Err(_) => return false,
        };
        if !response.is_success() && response.status != 206 {
            return false;
        }
        let content_type = response
            .find_header("content-type")
            .unwrap_or_default()
            .to_ascii_lowercase();
        !content_type.is_empty()
            && !content_type.contains("html")
            && !content_type.contains("json")
            && !content_type.contains("text/plain")
    }

    // ── utilitários internos ───────────────────────────────────────────────

    async fn send(&self, request: Request, provider: &str) -> Result<Response, DebridError> {
        self.fetcher.fetch(request).await.map_err(|_| {
            // Nunca propaga a URL: algumas carregam `apikey`/`token` na query.
            DebridError::Provider(format!("{provider}: falha de rede"))
        })
    }

    fn json(&self, response: Response, provider: &str) -> Result<Value, DebridError> {
        response
            .json_value()
            .map_err(|_| DebridError::Provider(format!("{provider}: resposta JSON inválida")))
    }

    async fn poll_until<F, Fut, T>(
        &self,
        window: PollWindow,
        abort: &dyn AbortSignal,
        mut attempt: F,
    ) -> Option<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Option<T>>,
    {
        let attempts = window.attempts();
        for index in 0..attempts {
            if abort.is_aborted() {
                return None;
            }
            if let Some(value) = attempt().await {
                return Some(value);
            }
            if index + 1 < attempts {
                self.sleeper.sleep(window.interval).await;
            }
        }
        None
    }
}

fn torbox_auth(token: &str) -> Vec<(String, String)> {
    vec![
        ("authorization".to_string(), format!("Bearer {token}")),
        ("user-agent".to_string(), ARCADIA_UA.to_string()),
    ]
}

fn multipart_body(fields: &[(&str, &str)]) -> (Vec<u8>, String) {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let boundary = format!(
        "----arcadia-boundary-{}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let mut body = String::new();
    for (name, value) in fields {
        body.push_str(&format!("--{boundary}\r\n"));
        body.push_str(&format!(
            "Content-Disposition: form-data; name=\"{name}\"\r\n\r\n"
        ));
        body.push_str(value);
        body.push_str("\r\n");
    }
    body.push_str(&format!("--{boundary}--\r\n"));
    (
        body.into_bytes(),
        format!("multipart/form-data; boundary={boundary}"),
    )
}

fn js_scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Null => "null".into(),
        Value::Array(_) | Value::Object(_) => String::new(),
    }
}

fn truthy_string(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if !crate::http::js_truthy(Some(value)) {
        return None;
    }
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Array(items) => items.iter().map(js_scalar).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
        Value::Null => return None,
    };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn first_truthy_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| truthy_string(value.get(*key)))
}

fn number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        Some(Value::String(text)) => text.trim().parse::<f64>().unwrap_or(0.0),
        Some(Value::Bool(true)) => 1.0,
        _ => 0.0,
    }
}

fn torbox_completed(info: &Value) -> bool {
    info.get("download_finished")
        .is_some_and(|value| crate::http::js_truthy(Some(value)))
        || matches!(
            info.get("download_state").and_then(Value::as_str),
            Some("completed") | Some("cached")
        )
}

fn find_torbox_item(data: Option<&Value>, id: &Value) -> Option<Value> {
    match data? {
        Value::Array(items) => items
            .iter()
            .find(|item| item.get("id") == Some(id))
            .cloned(),
        Value::Object(_) => Some(data?.clone()),
        _ => None,
    }
}

fn torbox_largest_file(info: &Value) -> Option<Value> {
    let files = info.get("files").and_then(Value::as_array)?;
    let mut sorted: Vec<&Value> = files.iter().collect();
    sorted.sort_by(|a, b| {
        number(b.get("size"))
            .partial_cmp(&number(a.get("size")))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted.first().and_then(|file| file.get("id")).cloned()
}

fn first_ad_magnet<'a>(data: &'a Value, key: &str) -> Option<&'a Value> {
    match data.get(key)? {
        Value::Array(items) => items.first(),
        Value::Object(_) => data.get(key),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct AdFile {
    link: String,
    size: f64,
    #[allow(dead_code)]
    name: String,
}

fn collect_ad_files(nodes: Option<&Value>, out: &mut Vec<AdFile>) {
    let Some(Value::Array(nodes)) = nodes else {
        return;
    };
    for node in nodes {
        if let Some(link) = truthy_string(node.get("l")) {
            out.push(AdFile {
                link,
                size: number(node.get("s")),
                name: truthy_string(node.get("n")).unwrap_or_default(),
            });
        }
        if let Some(children) = node.get("e") {
            if children
                .as_array()
                .is_some_and(|children| !children.is_empty())
            {
                collect_ad_files(Some(children), out);
            }
        }
    }
}

fn url_segment(url: &str, index: usize) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .nth(index)
        .map(str::to_string)
}

fn gofile_regex() -> Regex {
    Regex::new(r"gofile\.io/d/").expect("regex fixa válida")
}

fn pixeldrain_regex() -> Regex {
    Regex::new(r"pixeldra\.?in\.com/u/|pixeldrain\.com/u/").expect("regex fixa válida")
}

fn rootz_regex() -> Regex {
    Regex::new(r"rootz\.so/d/").expect("regex fixa válida")
}

fn rootz_token_regex() -> Regex {
    Regex::new(r#"\\?"pageToken\\?"\s*:\s*\\?"([^"\\]+)"#).expect("regex fixa válida")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_follow_config_keys_and_trim_values() {
        let config = serde_json::json!({
            "realdebrid_token": "  rd-token  ",
            "torbox_token": "",
            "alldebrid_token": "ad-token",
            "premiumize_token": "pz-token",
            "steam_api_key": "ignorada",
        });
        let tokens = DebridTokens::from_config_json(&config);
        assert_eq!(tokens.get(DebridProvider::RealDebrid), Some("rd-token"));
        assert!(!tokens.is_configured(DebridProvider::TorBox));
        assert_eq!(
            tokens.configured_providers(),
            vec![
                DebridProvider::RealDebrid,
                DebridProvider::AllDebrid,
                DebridProvider::Premiumize
            ]
        );
        assert!(tokens.has_any());
    }

    #[test]
    fn debug_and_status_never_expose_tokens() {
        let mut tokens = DebridTokens::new();
        tokens.set(DebridProvider::RealDebrid, "supersecret-token");
        let debug = format!("{tokens:?}");
        assert!(!debug.contains("supersecret"));
        assert!(debug.contains("realdebrid"));
        assert_eq!(
            tokens.masked(DebridProvider::RealDebrid).as_deref(),
            Some("sup•••en")
        );
        assert_eq!(mask_token("short"), "•••");
        let status = DebridStatus {
            provider: "realdebrid".into(),
            config_key: "realdebrid_token".into(),
            configured: true,
            token_masked: tokens.masked(DebridProvider::RealDebrid),
        };
        assert!(!format!("{status:?}").contains("supersecret"));
    }

    #[test]
    fn debrid_config_presence_matches_torrent_gate() {
        assert!(!has_debrid_configured(&serde_json::json!({})));
        assert!(has_debrid_configured(
            &serde_json::json!({ "premiumize_token": "x" })
        ));
    }

    #[test]
    fn rootz_token_regex_matches_plain_and_escaped_json() {
        let regex = rootz_token_regex();
        assert_eq!(
            regex
                .captures(r#"{"pageToken":"abc123"}"#)
                .and_then(|captures| captures.get(1))
                .map(|capture| capture.as_str()),
            Some("abc123")
        );
        assert_eq!(
            regex
                .captures(r#"{\"pageToken\":\"def456\"}"#)
                .and_then(|captures| captures.get(1))
                .map(|capture| capture.as_str()),
            Some("def456")
        );
    }

    #[test]
    fn poll_window_attempts_has_floor_of_one() {
        assert_eq!(
            PollWindow::new(Duration::ZERO, Duration::from_secs(1)).attempts(),
            1
        );
        assert_eq!(
            PollWindow::new(Duration::from_secs(3), Duration::from_secs(30)).attempts(),
            11
        );
    }
}
