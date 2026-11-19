//! Cliente HTTP da RetroAchievements — port 1:1 de
//! `app/electron/retroachievements/client.js`.
//!
//! Dois protocolos convivem aqui, de propósito:
//!
//! - **API pública** (`/API/API_*.php`), autenticada pela Web API Key
//!   (`u` + `y`), usada para validar a conta, buscar jogo por título e ler
//!   progresso/conquistas;
//! - **protocolo dos emuladores** (`/dorequest.php`, `r=login2`), que troca
//!   usuário+senha por um token de sessão. A senha só existe como parâmetro
//!   durante a chamada: este crate nunca a guarda, loga ou serializa.
//!
//! Todas as requisições usam exatamente os mesmos paths, ordem de query
//! (`u`, `y`, `g`/`i`), `Content-Type` de formulário, User-Agent
//! `Arcadia/1.0 (Electron)` e timeout de 15 s do oráculo congelado.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::error::RaResult;
use crate::progress::{parse_game_progress, GameProgress};
use crate::util::{
    extract_api_error, js_number, js_number_or, js_number_to_i64, js_or_empty_string, js_to_string,
    js_trim, js_truthy,
};
use crate::RaError;

/// Host padrão da RA (dados e API).
pub const DEFAULT_HOST: &str = "https://retroachievements.org";
/// User-Agent recomendado pelo rcheevos; sem ele o nginx da RA devolve 403.
pub const USER_AGENT: &str = "Arcadia/1.0 (Electron)";
/// Domínio de mídia (badges/avatares).
pub const MEDIA_HOST: &str = "https://media.retroachievements.org";
/// Timeout de cada requisição, igual a `REQUEST_TIMEOUT_MS` do JS.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub const LOGIN_PATH: &str = "/dorequest.php";
pub const USER_SUMMARY_PATH: &str = "/API/API_GetUserSummary.php";
pub const GAME_LIST_PATH: &str = "/API/API_GetGameList.php";
pub const GAME_PROGRESS_PATH: &str = "/API/API_GetGameInfoAndUserProgress.php";

/// Resposta de `loginRequest` quando o servidor confirma o login.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResult {
    pub username: String,
    pub token: String,
    pub score: i64,
}

/// Resposta de `verifyApiKey` (o `Username` confirma a conta).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedUser {
    pub username: String,
}

/// Jogo encontrado por título (`findGameByTitle`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRef {
    pub id: i64,
    pub title: String,
}

/// Cliente HTTP configurável. Clone é barato (o `reqwest::Client` interno é
/// compartilhado) e seguro para uso concorrente.
#[derive(Clone, Debug)]
pub struct RetroAchievementsClient {
    http: reqwest::Client,
    host: String,
    user_agent: String,
    timeout: Duration,
}

impl Default for RetroAchievementsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RetroAchievementsClient {
    /// Cliente com host, User-Agent e timeout padrão do Electron.
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Cliente apontando para outro host (testes/mock ou proxy).
    pub fn with_host(host: impl Into<String>) -> Self {
        Self::builder().host(host).build()
    }

    pub fn builder() -> RetroAchievementsClientBuilder {
        RetroAchievementsClientBuilder::default()
    }

    /// Host efetivo, sem barra final.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// `loginRequest`: POST `dorequest.php` com `r=login2&u=...&p=...`.
    ///
    /// A senha é apenas um parâmetro emprestado; não é clonada para nenhuma
    /// estrutura persistente. O status HTTP é ignorado de propósito: a RA
    /// devolve JSON `{ Success, Token, Error }` mesmo em 401.
    pub async fn login(&self, username: &str, password: &str) -> RaResult<LoginResult> {
        let user = js_trim(username);
        if user.is_empty() || password.is_empty() {
            return Err(RaError::EmptyCredentials);
        }

        let response = self
            .http
            .post(self.endpoint(LOGIN_PATH)?)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .header(reqwest::header::USER_AGENT, self.user_agent.as_str())
            .form(&[("r", "login2"), ("u", user), ("p", password)])
            .send()
            .await
            .map_err(map_transport_error)?;

        let (_, data) = read_json(response).await?;
        finish_login(&data)
    }

    /// `verifyApiKey`: GET `API_GetUserSummary.php?u=...&y=...`.
    ///
    /// Não devolve nem loga a chave. Um 200 com `{"Error": ...}` continua
    /// sendo sucesso para o JS (o corpo só é inspecionado quando `!ok`).
    pub async fn verify_api_key(&self, username: &str, api_key: &str) -> RaResult<VerifiedUser> {
        let user = js_trim(username);
        let key = js_trim(api_key);
        if user.is_empty() || key.is_empty() {
            return Err(RaError::EmptyCredentials);
        }

        let mut url = self.endpoint(USER_SUMMARY_PATH)?;
        url.query_pairs_mut()
            .append_pair("u", user)
            .append_pair("y", key);

        let (status_ok, data) = self.get_json(url).await?;
        if !status_ok {
            return Err(api_key_error(&data));
        }
        let username = match js_or_empty_string(data.get("User")) {
            value if value.is_empty() => user.to_string(),
            value => value,
        };
        Ok(VerifiedUser { username })
    }

    /// Nome histórico do endpoint de validação (`API_GetUserSummary.php`).
    pub async fn get_user_summary(&self, username: &str, api_key: &str) -> RaResult<VerifiedUser> {
        self.verify_api_key(username, api_key).await
    }

    /// `findGameByTitle`: GET `API_GetGameList.php?u=...&y=...&i=<console>`.
    ///
    /// Primeiro procura título exato (case-insensitive), depois o primeiro que
    /// contém o termo — exatamente como o oráculo. `Ok(None)` é "não achou".
    pub async fn find_game_by_title(
        &self,
        username: &str,
        api_key: &str,
        title: &str,
        console_id: u32,
    ) -> RaResult<Option<GameRef>> {
        let user = js_trim(username);
        let key = js_trim(api_key);
        let query = js_trim(title);
        if user.is_empty() || key.is_empty() || query.is_empty() {
            return Err(RaError::InvalidParameters);
        }

        let mut url = self.endpoint(GAME_LIST_PATH)?;
        url.query_pairs_mut()
            .append_pair("u", user)
            .append_pair("y", key)
            .append_pair(
                "i",
                &if console_id == 0 {
                    String::new()
                } else {
                    console_id.to_string()
                },
            );

        let (status_ok, data) = self.get_json(url).await?;
        if !status_ok {
            return Err(search_error(&data));
        }
        let Some(games) = data.as_array() else {
            return Err(RaError::SearchFailed);
        };

        let normalized = query.to_lowercase();
        let exact = games
            .iter()
            .find(|game| js_or_empty_string(game.get("Title")).to_lowercase() == normalized);
        let matched = exact.or_else(|| {
            games.iter().find(|game| {
                js_or_empty_string(game.get("Title"))
                    .to_lowercase()
                    .contains(&normalized)
            })
        });
        let Some(game) = matched else {
            return Ok(None);
        };
        Ok(Some(GameRef {
            id: js_number_to_i64(js_number(game.get("ID").unwrap_or(&Value::Null))),
            title: js_or_empty_string(game.get("Title")),
        }))
    }

    /// `getGameProgress`: GET
    /// `API_GetGameInfoAndUserProgress.php?u=...&y=...&g=<id>`.
    pub async fn get_game_progress(
        &self,
        username: &str,
        api_key: &str,
        game_id: i64,
    ) -> RaResult<GameProgress> {
        let user = js_trim(username);
        let key = js_trim(api_key);
        if user.is_empty() || key.is_empty() || game_id <= 0 {
            return Err(RaError::InvalidParameters);
        }

        let mut url = self.endpoint(GAME_PROGRESS_PATH)?;
        url.query_pairs_mut()
            .append_pair("u", user)
            .append_pair("y", key)
            .append_pair("g", &game_id.to_string());

        let (status_ok, data) = self.get_json(url).await?;
        if !status_ok {
            return Err(search_error(&data));
        }
        Ok(parse_game_progress(&data))
    }

    fn endpoint(&self, path: &str) -> RaResult<reqwest::Url> {
        reqwest::Url::parse(&format!("{}{path}", self.host)).map_err(|_| RaError::Network)
    }

    async fn get_json(&self, url: reqwest::Url) -> RaResult<(bool, Value)> {
        let response = self
            .http
            .get(url)
            .header(reqwest::header::USER_AGENT, self.user_agent.as_str())
            .send()
            .await
            .map_err(map_transport_error)?;
        read_json(response).await
    }
}

/// Builder do cliente. `timeout` é aplicado ao `reqwest::Client` inteiro.
#[derive(Clone, Debug)]
pub struct RetroAchievementsClientBuilder {
    host: String,
    user_agent: String,
    timeout: Duration,
}

impl Default for RetroAchievementsClientBuilder {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            user_agent: USER_AGENT.to_string(),
            timeout: REQUEST_TIMEOUT,
        }
    }
}

impl RetroAchievementsClientBuilder {
    /// Host base sem barra final (a barra é removida, como no JS).
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into().trim_end_matches('/').to_string();
        self
    }

    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn build(self) -> RetroAchievementsClient {
        let http = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .expect("cliente HTTP reqwest");
        RetroAchievementsClient {
            http,
            host: self.host,
            user_agent: self.user_agent,
            timeout: self.timeout,
        }
    }
}

fn finish_login(data: &Value) -> RaResult<LoginResult> {
    let success = data.get("Success").map(js_truthy).unwrap_or(false);
    let token = data.get("Token").filter(|value| js_truthy(value));
    let Some(token) = token else {
        return Err(login_error(data));
    };
    if !success {
        return Err(login_error(data));
    }
    Ok(LoginResult {
        username: js_or_empty_string(data.get("User")),
        token: js_to_string(token),
        score: normalize_score(data.get("Score")),
    })
}

fn normalize_score(value: Option<&Value>) -> i64 {
    let number = js_number_or(value, 0.0);
    if !number.is_finite() {
        return 0;
    }
    number as i64
}

fn login_error(data: &Value) -> RaError {
    match data.get("Error").filter(|value| js_truthy(value)) {
        Some(error) => RaError::Server(js_to_string(error)),
        None => RaError::InvalidCredentials,
    }
}

async fn read_json(response: reqwest::Response) -> RaResult<(bool, Value)> {
    let status_ok = response.status().is_success();
    let text = response.text().await.map_err(map_transport_error)?;
    let data = serde_json::from_str::<Value>(&text).map_err(|_| RaError::InvalidResponse)?;
    Ok((status_ok, data))
}

fn map_transport_error(error: reqwest::Error) -> RaError {
    if error.is_timeout() {
        RaError::Timeout
    } else {
        RaError::Network
    }
}

/// `data?.Error || data?.message || "falha_busca"` para a API pública.
fn search_error(data: &Value) -> RaError {
    extract_api_error(data).map_or(RaError::SearchFailed, RaError::Server)
}

/// `data?.Error || data?.message || "chave_invalida"` para a API pública.
fn api_key_error(data: &Value) -> RaError {
    extract_api_error(data).map_or(RaError::InvalidApiKey, RaError::Server)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finish_login_aceita_sucesso() {
        let result = finish_login(&json!({
            "Success": true,
            "User": "Fulano",
            "Token": "tok-123",
            "Score": "42"
        }))
        .unwrap();
        assert_eq!(result.username, "Fulano");
        assert_eq!(result.token, "tok-123");
        assert_eq!(result.score, 42);
    }

    #[test]
    fn finish_login_usa_error_real() {
        let error = finish_login(&json!({
            "Success": false,
            "Error": "Invalid User/Password combination."
        }))
        .unwrap_err();
        assert_eq!(error.code(), "Invalid User/Password combination.");
    }

    #[test]
    fn finish_login_sem_error_usa_codigo_local() {
        let error = finish_login(&json!({ "Success": false })).unwrap_err();
        assert_eq!(error, RaError::InvalidCredentials);
    }

    #[test]
    fn finish_login_sem_token_falha_mesmo_com_success() {
        let error = finish_login(&json!({ "Success": true, "User": "Fulano" })).unwrap_err();
        assert_eq!(error, RaError::InvalidCredentials);
    }

    #[test]
    fn score_nao_numerico_vira_zero() {
        assert_eq!(normalize_score(Some(&json!("abc"))), 0);
        assert_eq!(normalize_score(Some(&json!(null))), 0);
    }
}
