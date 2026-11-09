//! Cliente de autenticação — espelho da superfície de
//! `app/electron/supabase/client.js` (`AuthClient`) e do login por username de
//! `auth.js`.
//!
//! Endpoints (todos com `apikey`, e `Authorization: Bearer <access_token>`
//! quando já existe sessão):
//!
//! - `POST /auth/v1/signup`
//! - `POST /auth/v1/token?grant_type=password`
//! - `POST /auth/v1/token?grant_type=refresh_token`
//! - `GET  /auth/v1/user`
//! - `POST /auth/v1/logout`
//! - `GET  /auth/v1/health`

use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::sync::broadcast;

use crate::config::ApiConfig;
use crate::error::{ApiError, DecodeError, Error, Result};
use crate::http::{HttpClient, HttpRequest, RawResponse};

/// Janela de cache do usuário validado (o JS usa 60 s).
pub const USER_CACHE_TTL: Duration = Duration::from_secs(60);

/// Sessão no formato GoTrue consumido pelo restante do app.
///
/// Campos desconhecidos são preservados em [`Session::extra`] para que uma
/// sessão carregada do `session.json` volte ao disco com o mesmo shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default)]
    pub user: Value,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Session {
    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token
            .as_deref()
            .filter(|token| !token.is_empty())
    }

    pub fn user_id(&self) -> Option<&str> {
        self.user
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
    }

    fn fallback_user(&self) -> Value {
        self.user.clone()
    }
}

/// Resultado de `signUp` (`{ user, session }`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SignUpResponse {
    #[serde(default)]
    pub user: Value,
    #[serde(default)]
    pub session: Option<Session>,
}

/// Resultado de login: sessão completa + usuário (atalho do `session.user`).
#[derive(Debug, Clone, PartialEq)]
pub struct SignInResponse {
    pub user: Value,
    pub session: Session,
}

/// Eventos de autenticação equivalentes a `onAuthStateChange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthEvent {
    SignedIn,
    TokenRefreshed,
    SignedOut,
    UserDeleted,
}

impl AuthEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthEvent::SignedIn => "SIGNED_IN",
            AuthEvent::TokenRefreshed => "TOKEN_REFRESHED",
            AuthEvent::SignedOut => "SIGNED_OUT",
            AuthEvent::UserDeleted => "USER_DELETED",
        }
    }
}

impl std::fmt::Display for AuthEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Notificação entregue aos assinantes de [`AuthClient::on_auth_state_change`].
#[derive(Debug, Clone, PartialEq)]
pub struct AuthStateChange {
    pub event: AuthEvent,
    pub session: Option<Session>,
}

/// Opções de `setSession`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SetSessionOptions {
    /// Emite `SIGNED_IN` ao instalar a sessão.
    pub emit_signed_in: bool,
    /// Instala a sessão sem validar na rede; a validação fica por conta de
    /// [`AuthClient::validate_session`] (o boot otimista do Electron).
    pub optimistic: bool,
}

/// Cliente de auth compartilhável (clone aponta para o mesmo estado).
#[derive(Debug, Clone)]
pub struct AuthClient {
    inner: Arc<AuthInner>,
}

#[derive(Debug)]
struct AuthInner {
    config: Arc<ApiConfig>,
    http: HttpClient,
    session: RwLock<Option<Session>>,
    user_validated_at: Mutex<Option<Instant>>,
    refresh_lock: tokio::sync::Mutex<()>,
    events: broadcast::Sender<AuthStateChange>,
}

impl AuthClient {
    pub(crate) fn new(config: Arc<ApiConfig>, http: HttpClient) -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            inner: Arc::new(AuthInner {
                config,
                http,
                session: RwLock::new(None),
                user_validated_at: Mutex::new(None),
                refresh_lock: tokio::sync::Mutex::new(()),
                events,
            }),
        }
    }

    /// Assina os eventos de auth. O `broadcast` descarta mensagens antigas se o
    /// consumidor ficar para trás, como o emitter do JS.
    pub fn on_auth_state_change(&self) -> broadcast::Receiver<AuthStateChange> {
        self.inner.events.subscribe()
    }

    /// Sessão atual (equivalente a `getSession`; não faz rede).
    pub async fn get_session(&self) -> Option<Session> {
        self.read_session()
    }

    pub async fn access_token(&self) -> Option<String> {
        self.read_session()
            .map(|session| session.access_token)
            .filter(|token| !token.is_empty())
    }

    /// Instala a sessão (sem validar). Usado pelo boot otimista.
    pub async fn set_session_optimistic(&self, session: Session, emit_signed_in: bool) -> Session {
        self.write_session(Some(session.clone()));
        self.reset_user_cache();
        if emit_signed_in {
            self.emit(AuthEvent::SignedIn, Some(session.clone()));
        }
        session
    }

    /// Instala a sessão e valida em `/auth/v1/user`; em 401 (ou outra falha)
    /// tenta um refresh. Igual ao `setSession` não-otimista do JS: se o refresh
    /// também falhar, a sessão é limpa e o erro devolvido.
    pub async fn set_session(
        &self,
        session: Session,
        options: SetSessionOptions,
    ) -> Result<Session> {
        if options.optimistic {
            return Ok(self
                .set_session_optimistic(session, options.emit_signed_in)
                .await);
        }

        self.write_session(Some(session.clone()));
        self.reset_user_cache();

        let response = self.request(Method::GET, "/auth/v1/user", None).await;
        if let Ok(response) = &response {
            if response.is_success() {
                self.mark_user_validated();
                if options.emit_signed_in {
                    self.emit(AuthEvent::SignedIn, Some(session.clone()));
                }
                return Ok(session);
            }
        }

        // O JS tenta refresh para qualquer erro do /user; uma falha de rede
        // também acaba aqui, então mantemos a mesma ordem.
        if let Ok(refreshed) = self.refresh_session().await {
            return Ok(refreshed);
        }

        self.clear_session_silently();
        Err(ApiError::new(401, None, "sessao invalida", "").into())
    }

    /// Valida a sessão restaurada em background (equivalente a
    /// `_validarSessaoSalva`). Só limpa a sessão quando o backend confirma 401
    /// e o refresh falha; falha de rede mantém a sessão intacta.
    pub async fn validate_session(&self) -> Result<()> {
        let Some(session) = self.read_session() else {
            return Ok(());
        };
        let token = session.access_token;

        let response = self.request(Method::GET, "/auth/v1/user", None).await;
        match response {
            Ok(response) if response.is_success() => {
                if self.current_token().as_deref() == Some(token.as_str()) {
                    self.mark_user_validated();
                }
                Ok(())
            }
            Ok(response) if response.status == 401 => match self.refresh_session().await {
                Ok(_) => Ok(()),
                Err(error) => {
                    if self.current_token().as_deref() == Some(token.as_str()) {
                        self.clear_session();
                    }
                    Err(error)
                }
            },
            Ok(response) => Err(response.api_error().into()),
            // Falha de rede não desloga quem está offline.
            Err(error) => Err(error),
        }
    }

    /// `POST /auth/v1/signup`.
    pub async fn sign_up(
        &self,
        email: &str,
        password: &str,
        options: Option<Value>,
    ) -> Result<SignUpResponse> {
        let mut body = Map::new();
        body.insert("email".into(), Value::String(email.to_string()));
        body.insert("password".into(), Value::String(password.to_string()));
        if let Some(options) = options {
            body.insert("options".into(), options);
        }

        let response = self
            .request(Method::POST, "/auth/v1/signup", Some(Value::Object(body)))
            .await?;
        if !response.is_success() {
            return Err(response.api_error().into());
        }
        let parsed: SignUpResponse = response.json()?;

        self.write_session(parsed.session.clone());
        self.reset_user_cache();
        if parsed.session.is_some() {
            self.mark_user_validated();
        }
        self.emit(AuthEvent::SignedIn, parsed.session.clone());
        Ok(parsed)
    }

    /// `POST /auth/v1/token?grant_type=password`.
    pub async fn sign_in_with_password(
        &self,
        email: &str,
        password: &str,
    ) -> Result<SignInResponse> {
        let response = self
            .request(
                Method::POST,
                "/auth/v1/token?grant_type=password",
                Some(json!({ "email": email, "password": password })),
            )
            .await?;
        if !response.is_success() {
            return Err(response.api_error().into());
        }
        let session: Session = response.json()?;
        self.write_session(Some(session.clone()));
        self.reset_user_cache();
        self.mark_user_validated();
        self.emit(AuthEvent::SignedIn, Some(session.clone()));
        Ok(SignInResponse {
            user: session.user.clone(),
            session,
        })
    }

    /// Login por username exatamente como `auth.js`: RPC `login_check`
    /// (`p_username`/`p_password`) devolve o e-mail só depois da senha correta;
    /// em seguida o grant de senha usa esse e-mail.
    pub async fn sign_in_with_username(
        &self,
        username: &str,
        password: &str,
    ) -> Result<SignInResponse> {
        let username = username.trim().to_lowercase();
        let response = self
            .request(
                Method::POST,
                "/rest/v1/rpc/login_check",
                Some(json!({ "p_username": username, "p_password": password })),
            )
            .await?;
        if !response.is_success() {
            return Err(response.api_error().into());
        }
        let check: Value = response.json()?;
        if check.get("ok") != Some(&Value::Bool(true)) {
            let message = check
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("usuario_nao_existe")
                .to_string();
            return Err(ApiError::new(401, None, message, response.text()).into());
        }
        let Some(email) = check
            .get("email")
            .and_then(Value::as_str)
            .filter(|email| !email.is_empty())
        else {
            return Err(ApiError::new(401, None, "usuario_nao_existe", response.text()).into());
        };

        self.sign_in_with_password(email, password).await
    }

    /// `POST /auth/v1/token?grant_type=refresh_token`.
    ///
    /// Um único refresh é compartilhado por chamadas concorrentes. Em falha a
    /// sessão atual é preservada (nada é limpo silenciosamente).
    pub async fn refresh_session(&self) -> Result<Session> {
        let Some(captured) = self.read_session() else {
            return Err(ApiError::new(401, None, "sessao sem refresh_token", "").into());
        };
        let Some(refresh_token) = captured.refresh_token().map(str::to_string) else {
            return Err(ApiError::new(401, None, "sessao sem refresh_token", "").into());
        };
        let old_user = captured.fallback_user();
        let access_token = captured.access_token;

        let _guard = self.inner.refresh_lock.lock().await;

        // Outra tarefa pode ter renovado enquanto esperávamos o lock.
        match self.read_session() {
            Some(current) if current.refresh_token() != Some(refresh_token.as_str()) => {
                return Ok(current);
            }
            None => {
                return Err(ApiError::new(401, None, "sessao sem refresh_token", "").into());
            }
            Some(_) => {}
        }

        let response = self
            .request_with_token(
                Method::POST,
                "/auth/v1/token?grant_type=refresh_token",
                Some(json!({ "refresh_token": refresh_token })),
                Some(access_token),
            )
            .await?;
        if !response.is_success() {
            return Err(response.api_error().into());
        }

        let mut session: Session = response.json()?;
        if session.access_token.trim().is_empty() {
            return Err(ApiError::new(401, None, "resposta de refresh invalida", "").into());
        }
        if session.refresh_token().is_none() {
            session.refresh_token = Some(refresh_token.clone());
        }
        if session.user.is_null() {
            session.user = old_user;
        }

        // Uma resposta antiga não pode substituir uma sessão que mudou durante
        // o refresh (logout/login rápido ou troca de conta).
        let still_current = self
            .read_session()
            .and_then(|current| current.refresh_token)
            .as_deref()
            == Some(refresh_token.as_str());
        if !still_current {
            return Err(ApiError::new(409, None, "sessao_trocada", "").into());
        }

        self.write_session(Some(session.clone()));
        self.reset_user_cache();
        self.mark_user_validated();
        self.emit(AuthEvent::TokenRefreshed, Some(session.clone()));
        Ok(session)
    }

    /// `GET /auth/v1/user` com cache de 60 s e refresh único em 401.
    pub async fn get_user(&self) -> Result<Value> {
        let Some(session) = self.read_session() else {
            return Err(ApiError::new(401, None, "nao_logado", "").into());
        };
        if session.user_id().is_some() && self.user_cache_valid() {
            return Ok(session.user);
        }

        let token = session.access_token.clone();
        let response = self.request(Method::GET, "/auth/v1/user", None).await?;
        if response.status == 401 && session.refresh_token().is_some() {
            match self.refresh_session().await {
                Ok(refreshed) => return Ok(refreshed.user),
                // O JS devolve o erro original do /user quando o refresh falha.
                Err(_) => return Err(response.api_error().into()),
            }
        }
        if !response.is_success() {
            return Err(response.api_error().into());
        }

        let body: Value = response.json()?;
        let user = body
            .get("user")
            .cloned()
            .or_else(|| {
                body.as_object()
                    .filter(|object| object.contains_key("id"))
                    .map(|_| body.clone())
            })
            .unwrap_or(Value::Null);

        if self.current_token().as_deref() == Some(token.as_str()) {
            self.mark_user_validated();
        }
        Ok(user)
    }

    /// `POST /auth/v1/logout`. Sempre limpa a sessão local, mesmo se a rede
    /// falhar (o token expira sozinho), como o `finally` do JS.
    pub async fn sign_out(&self) -> Result<()> {
        let result = self.request(Method::POST, "/auth/v1/logout", None).await;
        self.clear_session();
        match result {
            Ok(response) if response.is_success() => Ok(()),
            Ok(response) => Err(response.api_error().into()),
            Err(error) => Err(error),
        }
    }

    /// `GET /auth/v1/health` (usado pelo warmup do backend).
    pub async fn health(&self) -> Result<Value> {
        let response = self.request(Method::GET, "/auth/v1/health", None).await?;
        if !response.is_success() {
            return Err(response.api_error().into());
        }
        Ok(response.json_or_text())
    }

    /// Limpa a sessão local e emite `SIGNED_OUT`.
    pub fn clear_session(&self) {
        self.clear_session_silently();
        self.emit(AuthEvent::SignedOut, None);
    }

    fn clear_session_silently(&self) {
        self.write_session(None);
        self.reset_user_cache();
    }

    fn read_session(&self) -> Option<Session> {
        self.inner
            .session
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn write_session(&self, session: Option<Session>) {
        *self
            .inner
            .session
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = session;
    }

    fn current_token(&self) -> Option<String> {
        self.read_session()
            .map(|session| session.access_token)
            .filter(|token| !token.is_empty())
    }

    fn reset_user_cache(&self) {
        *self
            .inner
            .user_validated_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    fn mark_user_validated(&self) {
        *self
            .inner
            .user_validated_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
    }

    fn user_cache_valid(&self) -> bool {
        self.inner
            .user_validated_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .map(|validated| validated.elapsed() < USER_CACHE_TTL)
            .unwrap_or(false)
    }

    fn emit(&self, event: AuthEvent, session: Option<Session>) {
        // Sem assinantes o broadcast devolve erro; não é falha do fluxo.
        let _ = self.inner.events.send(AuthStateChange { event, session });
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<RawResponse> {
        let token = self.current_token();
        self.request_with_token(method, path, body, token).await
    }

    async fn request_with_token(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        token: Option<String>,
    ) -> Result<RawResponse> {
        let mut request = HttpRequest::new(method, format!("{}{path}", self.inner.config.url));
        request = request.header("apikey", self.inner.config.anon_key.clone());
        if let Some(token) = token.filter(|token| !token.is_empty()) {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        if let Some(body) = body {
            let bytes = serde_json::to_vec(&body).map_err(|error| {
                Error::Decode(DecodeError {
                    status: 0,
                    message: error.to_string(),
                    body: String::new(),
                })
            })?;
            request = request
                .header("content-type", "application/json")
                .body(bytes);
        }
        self.inner.http.send(&request).await.map_err(Into::into)
    }
}
