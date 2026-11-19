//! Payloads dos eventos do runtime de conquistas.
//!
//! O oráculo Electron entrega ao renderer `achievement:unlocked` com o payload
//! do watcher (`steam_bin.js`/`cracked_watcher.js`) acrescido de `done`/`total`
//! pelo `onUnlockAchievement` do `main.js`. A UI (`AchievementToast.tsx`) lê
//! `appid`, `key`, `title`, `desc`, `icon`, `percent` e `unlock`.
//!
//! Aqui o evento carrega também os campos normalizados do schema
//! (`apiname`, `name`, `description`, `hidden`, `unlockTime`) para que o host
//! possa alimentar tanto o toast quanto o painel/sync sem reler o
//! `achievements.json`. [`UnlockEvent::to_value`] devolve a união dos dois
//! formatos; [`UnlockEvent::toast_value`] devolve exatamente o formato que o
//! renderer espera hoje (com `apiname`/`provider` quando existirem).
//!
//! Revogação não tem canal de renderer no oráculo: o callback `onRevoke` do
//! `main.js` recebe `{ appid, apiname }` e chama `deleteAchievement` no sync.
//! [`RevokeEvent::callback_value`] reproduz exatamente esse payload;
//! [`RevokeEvent::to_value`] acrescenta contexto para log/telemetria.

use serde_json::{Map, Value};

use crate::loader::normalize_steam_icon_url;

/// Canal IPC do renderer (`main.js` → `webContents.send`).
pub const ACHIEVEMENT_UNLOCKED_CHANNEL: &str = "achievement:unlocked";
/// Canal sugerido para revogação. O oráculo não envia isto ao renderer; o
/// callback de revogação é local ao main process. Fica exposto para hosts que
/// queiram observar o evento.
pub const ACHIEVEMENT_REVOKED_CHANNEL: &str = "achievement:revoked";

/// `JANELA_TOAST_S` do `main.js`: só um desbloqueio com timestamp dentro desta
/// janela vira toast. O evento continua sendo emitido (o estado local e o sync
/// são atualizados); o host decide se mostra o aviso.
pub const TOAST_WINDOW_SECONDS: i64 = 300;

fn number_value(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or_else(|| Value::from(0))
}

/// Desbloqueio novo detectado pelo runtime (bin da Steam, crack ou emulador).
#[derive(Debug, Clone, PartialEq)]
pub struct UnlockEvent {
    pub appid: String,
    /// Chave `block|bit` do `achievements.json`; quando o item não tem
    /// block/bit, é o apiname (mesma regra de `payloadParaDesbloqueio`).
    pub key: String,
    pub apiname: String,
    /// `steam` para o vigia de bin; nome do grupo (`goldberg`, `upc`, ...) para
    /// os parsers de crack — igual a `provider` no JS.
    pub provider: String,
    /// `title` do payload JS / `name` do schema.
    pub title: String,
    /// `desc` do payload JS / `description` do schema.
    pub desc: String,
    pub icon: String,
    pub icongray: String,
    pub hidden: bool,
    pub percent: f64,
    /// Epoch em SEGUNDOS, como `unlock` no JS.
    pub unlock: i64,
    pub local_unlock: bool,
    pub done: usize,
    pub total: usize,
}

impl UnlockEvent {
    /// `name` do schema (alias de `title`).
    pub fn name(&self) -> &str {
        &self.title
    }

    /// `description` do schema (alias de `desc`).
    pub fn description(&self) -> &str {
        &self.desc
    }

    /// `unlockTime` (epoch em segundos, alias de `unlock`).
    pub fn unlock_time(&self) -> i64 {
        self.unlock
    }

    /// Mesma regra do `main.js`: `unlock > 0 && agora - unlock <= janela`.
    /// Timestamp no futuro (relógio torto) também passa, como no JS.
    pub fn is_recent(&self, now_seconds: i64, window_seconds: i64) -> bool {
        self.unlock > 0 && now_seconds.saturating_sub(self.unlock) <= window_seconds
    }

    /// `isPlatinum` do `main.js`: todas as conquistas do jogo desbloqueadas.
    pub fn is_platinum(&self) -> bool {
        self.total > 0 && self.done == self.total
    }

    /// Formato exato consumido pelo renderer hoje: payload do watcher +
    /// `done`/`total`. `apiname`/`provider` só entram quando existem (o payload
    /// do bin da Steam não os tem no JS).
    pub fn toast_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("appid".into(), Value::String(self.appid.clone()));
        object.insert("key".into(), Value::String(self.key.clone()));
        if !self.apiname.is_empty() {
            object.insert("apiname".into(), Value::String(self.apiname.clone()));
        }
        if !self.provider.is_empty() {
            object.insert("provider".into(), Value::String(self.provider.clone()));
        }
        object.insert("title".into(), Value::String(self.title.clone()));
        object.insert("desc".into(), Value::String(self.desc.clone()));
        let icon = normalize_steam_icon_url(&self.icon).unwrap_or_else(|| self.icon.clone());
        object.insert("icon".into(), Value::String(icon));
        object.insert("percent".into(), number_value(self.percent));
        object.insert("unlock".into(), Value::from(self.unlock));
        object.insert("done".into(), Value::from(self.done as u64));
        object.insert("total".into(), Value::from(self.total as u64));
        Value::Object(object)
    }

    /// Payload canônico do evento: união do toast legado com os campos do
    /// schema (`name`/`description`/`hidden`/`unlockTime`/`icongray`).
    pub fn to_value(&self) -> Value {
        let mut object = match self.toast_value() {
            Value::Object(object) => object,
            _ => Map::new(),
        };
        object.insert("name".into(), Value::String(self.title.clone()));
        object.insert("description".into(), Value::String(self.desc.clone()));
        let icongray =
            normalize_steam_icon_url(&self.icongray).unwrap_or_else(|| self.icongray.clone());
        object.insert("icongray".into(), Value::String(icongray));
        object.insert("hidden".into(), Value::Bool(self.hidden));
        object.insert("unlockTime".into(), Value::from(self.unlock));
        object.insert(
            "unlockTimeMs".into(),
            Value::from(self.unlock.saturating_mul(1000)),
        );
        object.insert("localUnlock".into(), Value::Bool(self.local_unlock));
        Value::Object(object)
    }
}

/// Revogação do runtime local (UPC) — o item estava `achieved`/`localUnlock`
/// no store, mas o arquivo do emulador diz `earned=0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevokeEvent {
    pub appid: String,
    pub apiname: String,
    /// `block|bit` quando o item tem as duas chaves; senão vazio.
    pub key: String,
    pub title: String,
    pub provider: String,
    /// Motivo legível (`upc-runtime`).
    pub reason: String,
}

impl RevokeEvent {
    /// Payload exato do `onRevokeAchievement(payload)` do `main.js`.
    pub fn callback_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("appid".into(), Value::String(self.appid.clone()));
        object.insert("apiname".into(), Value::String(self.apiname.clone()));
        Value::Object(object)
    }

    /// Payload com contexto para log/event bus.
    pub fn to_value(&self) -> Value {
        let mut object = match self.callback_value() {
            Value::Object(object) => object,
            _ => Map::new(),
        };
        object.insert("key".into(), Value::String(self.key.clone()));
        object.insert("title".into(), Value::String(self.title.clone()));
        object.insert("provider".into(), Value::String(self.provider.clone()));
        object.insert("reason".into(), Value::String(self.reason.clone()));
        object.insert("achieved".into(), Value::Bool(false));
        object.insert("unlock".into(), Value::from(0));
        Value::Object(object)
    }
}

/// Evento de uma passada do runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum AchievementEvent {
    Unlocked(UnlockEvent),
    Revoked(RevokeEvent),
}

impl AchievementEvent {
    /// Canal sugerido para o host publicar o evento.
    pub fn channel(&self) -> &'static str {
        match self {
            Self::Unlocked(_) => ACHIEVEMENT_UNLOCKED_CHANNEL,
            Self::Revoked(_) => ACHIEVEMENT_REVOKED_CHANNEL,
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Self::Unlocked(event) => event.to_value(),
            Self::Revoked(event) => event.to_value(),
        }
    }

    pub fn appid(&self) -> &str {
        match self {
            Self::Unlocked(event) => &event.appid,
            Self::Revoked(event) => &event.appid,
        }
    }

    pub fn apiname(&self) -> &str {
        match self {
            Self::Unlocked(event) => &event.apiname,
            Self::Revoked(event) => &event.apiname,
        }
    }

    pub fn as_unlock(&self) -> Option<&UnlockEvent> {
        match self {
            Self::Unlocked(event) => Some(event),
            Self::Revoked(_) => None,
        }
    }

    pub fn as_revoke(&self) -> Option<&RevokeEvent> {
        match self {
            Self::Unlocked(_) => None,
            Self::Revoked(event) => Some(event),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn unlock() -> UnlockEvent {
        UnlockEvent {
            appid: "42".into(),
            key: "1|0".into(),
            apiname: "ACH01".into(),
            provider: "steam".into(),
            title: "Primeiros passos".into(),
            desc: "Descrição".into(),
            icon: "file:///icon.jpg".into(),
            icongray: "file:///gray.jpg".into(),
            hidden: true,
            percent: 12.5,
            unlock: 1_700_000_000,
            local_unlock: true,
            done: 3,
            total: 10,
        }
    }

    #[test]
    fn unlock_toast_matches_the_renderer_payload() {
        let value = unlock().toast_value();
        assert_eq!(
            value,
            json!({
                "appid": "42",
                "key": "1|0",
                "apiname": "ACH01",
                "provider": "steam",
                "title": "Primeiros passos",
                "desc": "Descrição",
                "icon": "file:///icon.jpg",
                "percent": 12.5,
                "unlock": 1_700_000_000i64,
                "done": 3,
                "total": 10,
            })
        );
    }

    #[test]
    fn unlock_canonical_payload_carries_schema_fields() {
        let value = unlock().to_value();
        assert_eq!(value["name"], json!("Primeiros passos"));
        assert_eq!(value["description"], json!("Descrição"));
        assert_eq!(value["icongray"], json!("file:///gray.jpg"));
        assert_eq!(value["hidden"], json!(true));
        assert_eq!(value["unlockTime"], json!(1_700_000_000i64));
        assert_eq!(value["unlockTimeMs"], json!(1_700_000_000_000i64));
        assert_eq!(value["localUnlock"], json!(true));
    }

    #[test]
    fn recent_window_matches_the_main_process_rule() {
        let event = unlock();
        assert!(event.is_recent(1_700_000_100, TOAST_WINDOW_SECONDS));
        assert!(event.is_recent(1_700_000_300, TOAST_WINDOW_SECONDS));
        assert!(!event.is_recent(1_700_000_301, TOAST_WINDOW_SECONDS));
        assert!(!event.is_recent(1_700_000_100, 50));
        let mut old = event;
        old.unlock = 0;
        assert!(!old.is_recent(1_700_000_100, TOAST_WINDOW_SECONDS));
    }

    #[test]
    fn platinum_is_all_items_unlocked_but_never_an_empty_game() {
        let mut event = unlock();
        assert!(!event.is_platinum(), "3 de 10");
        event.done = 10;
        assert!(event.is_platinum());
        event.total = 0;
        event.done = 0;
        assert!(!event.is_platinum());
    }

    #[test]
    fn revoke_payloads_match_the_callback_and_carry_context() {
        let event = RevokeEvent {
            appid: "42".into(),
            apiname: "ACH02".into(),
            key: "1|1".into(),
            title: "Segunda".into(),
            provider: "upc".into(),
            reason: "upc-runtime".into(),
        };
        assert_eq!(
            event.callback_value(),
            json!({ "appid": "42", "apiname": "ACH02" })
        );
        let value = event.to_value();
        assert_eq!(value["key"], json!("1|1"));
        assert_eq!(value["achieved"], json!(false));
        assert_eq!(value["unlock"], json!(0));
        assert_eq!(value["reason"], json!("upc-runtime"));
    }

    #[test]
    fn event_enum_exposes_channel_and_typed_access() {
        let event = AchievementEvent::Unlocked(unlock());
        assert_eq!(event.channel(), ACHIEVEMENT_UNLOCKED_CHANNEL);
        assert_eq!(event.appid(), "42");
        assert_eq!(event.apiname(), "ACH01");
        assert!(event.as_unlock().is_some());
        assert!(event.as_revoke().is_none());
        assert_eq!(event.to_value()["key"], json!("1|0"));

        let event = AchievementEvent::Revoked(RevokeEvent {
            appid: "42".into(),
            apiname: "ACH02".into(),
            key: "1|1".into(),
            title: "Segunda".into(),
            provider: "upc".into(),
            reason: "upc-runtime".into(),
        });
        assert_eq!(event.channel(), ACHIEVEMENT_REVOKED_CHANNEL);
        assert!(event.as_revoke().is_some());
        assert_eq!(event.to_value()["apiname"], json!("ACH02"));
    }
}
