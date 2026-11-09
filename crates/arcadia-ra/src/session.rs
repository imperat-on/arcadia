//! Sessão RetroAchievements com armazenamento de config injetado.
//!
//! Reproduz os handlers `retroachievements:login`, `:logout`, `:status`,
//! `:setApiKey` e `:gameProgress` de `app/electron/main.js`, sem IPC e sem
//! conhecer `config.json`:
//!
//! | Operação | Chaves escritas |
//! |---|---|
//! | login | `retroachievements_username`, `retroachievements_token` |
//! | logout | as mesmas duas, com string vazia |
//! | setApiKey | `retroachievements_web_api_key` (após validar na RA) |
//!
//! A senha é somente um parâmetro de [`RaSession::login`]: nunca é gravada,
//! clonada para o estado da sessão, logada ou incluída em erro. O valor
//! persistido é o **token de sessão** devolvido por `dorequest.php`.
//!
//! Assim como o oráculo congelado, trocar de conta (login com outro usuário)
//! substitui usuário+token mas **não** apaga a Web API Key antiga; `logout`
//! também preserva a chave. O IPC pode limpar a chave explicitamente pelo
//! próprio `config:set` se desejar outro comportamento.

use std::fmt;
use std::sync::{Arc, RwLock};

use serde::Serialize;
use serde_json::{json, Value};

use crate::client::{LoginResult, RetroAchievementsClient};
use crate::error::{RaError, RaResult};
use crate::progress::{find_game_for_system, GameProgress};
use crate::util::{js_field_string, js_trim, js_truthy};

pub const KEY_USERNAME: &str = "retroachievements_username";
pub const KEY_TOKEN: &str = "retroachievements_token";
pub const KEY_WEB_API_KEY: &str = "retroachievements_web_api_key";

/// Config injetado. O adaptador concreto (arquivo, store, teste) decide como
/// ler e mesclar o patch; o contrato é o mesmo de `readConfig`/`writeConfig`
/// do Electron: leitura tolerante e merge raso preservando chaves alheias.
pub trait RaConfigStorage: Send + Sync {
    /// Config atual. Falhas de leitura podem virar `{}` (como `readConfig`).
    fn read_config(&self) -> Value;

    /// Aplica um patch raso e devolve a config resultante. `Err` vira
    /// [`RaError::Storage`] e é propagado sem tocar em credenciais.
    fn write_config_patch(&self, patch: &Value) -> Result<Value, String>;
}

/// Armazenamento em memória para testes e para o IPC compor o seu próprio.
#[derive(Clone, Debug, Default)]
pub struct MemoryConfigStorage {
    inner: Arc<RwLock<Value>>,
}

impl MemoryConfigStorage {
    pub fn new(config: Value) -> Self {
        Self {
            inner: Arc::new(RwLock::new(config)),
        }
    }

    pub fn snapshot(&self) -> Value {
        self.inner
            .read()
            .map(|value| value.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }
}

impl RaConfigStorage for MemoryConfigStorage {
    fn read_config(&self) -> Value {
        self.snapshot()
    }

    fn write_config_patch(&self, patch: &Value) -> Result<Value, String> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = guard.as_object().cloned().unwrap_or_default();
        if let Some(patch) = patch.as_object() {
            for (key, value) in patch {
                next.insert(key.clone(), value.clone());
            }
        }
        *guard = Value::Object(next.clone());
        Ok(Value::Object(next))
    }
}

/// Estado devolvido por `retroachievements:status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaStatus {
    pub connected: bool,
    pub username: String,
}

impl RaStatus {
    /// Envelope exato do handler: `{ ok, connected, username }`.
    pub fn to_value(&self) -> Value {
        json!({
            "ok": true,
            "connected": self.connected,
            "username": self.username,
        })
    }
}

/// Credenciais prontas para `arcadia_emulators::configure_emulator_credentials`.
///
/// O `Debug` redige o token para não vazar em logs.
#[derive(Clone, PartialEq, Eq)]
pub struct RaCredentials {
    pub username: String,
    pub token: String,
}

impl fmt::Debug for RaCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RaCredentials")
            .field("username", &self.username)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Fachada de sessão sobre o cliente HTTP e o config injetado.
#[derive(Clone)]
pub struct RaSession {
    client: RetroAchievementsClient,
    storage: Arc<dyn RaConfigStorage>,
}

impl RaSession {
    pub fn new(client: RetroAchievementsClient, storage: impl RaConfigStorage + 'static) -> Self {
        Self {
            client,
            storage: Arc::new(storage),
        }
    }

    pub fn with_storage(
        client: RetroAchievementsClient,
        storage: Arc<dyn RaConfigStorage>,
    ) -> Self {
        Self { client, storage }
    }

    pub fn client(&self) -> &RetroAchievementsClient {
        &self.client
    }

    /// Config bruta (inclui token); use só para o adaptador de config.
    pub fn config_snapshot(&self) -> Value {
        self.storage.read_config()
    }

    /// `retroachievements:status`.
    pub fn status(&self) -> RaStatus {
        let config = self.storage.read_config();
        let username = js_field_string(&config, KEY_USERNAME);
        let has_token = config.get(KEY_TOKEN).map(js_truthy).unwrap_or(false);
        RaStatus {
            connected: !username.is_empty() && has_token,
            username,
        }
    }

    /// Usuário+token salvos, se ambos existirem (para aplicar no emulador).
    pub fn credentials(&self) -> Option<RaCredentials> {
        let config = self.storage.read_config();
        let username = js_field_string(&config, KEY_USERNAME);
        let token = js_field_string(&config, KEY_TOKEN);
        if username.is_empty() || token.is_empty() {
            return None;
        }
        Some(RaCredentials { username, token })
    }

    /// Web API Key salva, se houver.
    pub fn web_api_key(&self) -> Option<String> {
        let config = self.storage.read_config();
        let key = js_field_string(&config, KEY_WEB_API_KEY);
        if key.is_empty() {
            None
        } else {
            Some(key)
        }
    }

    /// `retroachievements:login`.
    ///
    /// Troca usuário+senha pelo token e persiste **somente**
    /// `retroachievements_username` + `retroachievements_token`. Em falha de
    /// rede/credencial, o config fica intocado.
    pub async fn login(&self, username: &str, password: &str) -> RaResult<LoginResult> {
        let result = self.client.login(username, password).await?;
        self.write_patch(json!({
            KEY_USERNAME: result.username,
            KEY_TOKEN: result.token,
        }))?;
        Ok(result)
    }

    /// `retroachievements:logout`: apaga usuário+token (strings vazias).
    pub fn logout(&self) -> RaResult<()> {
        self.write_patch(json!({
            KEY_USERNAME: "",
            KEY_TOKEN: "",
        }))
    }

    /// `retroachievements:setApiKey`.
    ///
    /// Exige usuário conectado, valida a chave em `API_GetUserSummary.php` e
    /// só então grava a versão com `trim()` (nunca a chave crua com espaços).
    pub async fn set_api_key(&self, api_key: &str) -> RaResult<()> {
        let config = self.storage.read_config();
        let username = js_field_string(&config, KEY_USERNAME);
        if username.is_empty() {
            return Err(RaError::NotConnected);
        }
        let key = js_trim(api_key);
        self.client.verify_api_key(&username, key).await?;
        self.write_patch(json!({ KEY_WEB_API_KEY: key }))?;
        Ok(())
    }

    /// `retroachievements:gameProgress`.
    ///
    /// Resolve o console RA pelo `systemId`, busca o jogo por título e lê o
    /// progresso. Devolve `Ok(None)` quando nenhum jogo casa (o handler
    /// responde `{ ok: true, game: null, achievements: [] }`).
    pub async fn game_progress(
        &self,
        title: &str,
        system_id: &str,
    ) -> RaResult<Option<GameProgress>> {
        let config = self.storage.read_config();
        let username = js_field_string(&config, KEY_USERNAME);
        let api_key = js_field_string(&config, KEY_WEB_API_KEY);
        if username.is_empty() || api_key.is_empty() {
            return Err(RaError::MissingApiKey);
        }
        let found =
            find_game_for_system(&self.client, &username, &api_key, title, system_id).await?;
        let Some(game) = found else {
            return Ok(None);
        };
        self.client
            .get_game_progress(&username, &api_key, game.id)
            .await
            .map(Some)
    }

    fn write_patch(&self, patch: Value) -> RaResult<()> {
        self.storage
            .write_config_patch(&patch)
            .map(|_| ())
            .map_err(RaError::Storage)
    }
}
