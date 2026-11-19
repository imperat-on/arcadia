//! Envelopes IPC da superfície versionada de plugins.
//!
//! Os shapes abaixo reproduzem `app/electron/main.js`/`plugins.js`:
//!
//! - `plugins:list`     -> `{ ok: true, plugins: [...shape histórico...] }`
//! - `plugins:details`  -> `{ ok: true, plugins: [...shape detalhado...] }`
//! - `plugins:get`      -> `{ ok: true, plugin }` ou `{ ok: false, error }`
//! - `plugins:register` -> resultado do registro (`plugin` ou `error/errors`)
//! - `plugins:unregister/enable/disable` -> `{ ok }` ou `{ ok: false, error }`
//! - `plugins:verify`   -> resultado de verificação sem caminho privado
//!
//! `plugins:install`/`plugins:remove` são a API legada de flags de built-ins e
//! ficam fora do escopo deste crate; a fachada Rust expõe `enable`/`disable`
//! com a mesma semântica de estado.

use serde::Serialize;
use serde_json::Value;

use crate::registry::{
    MutationResult, PluginDetailedItem, PluginListItem, PluginRegistry, RegisterResult,
    VerificationResult,
};

pub const PLUGIN_IPC_CHANNELS: [&str; 8] = [
    "plugins:list",
    "plugins:details",
    "plugins:get",
    "plugins:register",
    "plugins:unregister",
    "plugins:enable",
    "plugins:disable",
    "plugins:verify",
];

/// `pluginIdFromIpc`: somente strings são aceitas.
pub fn plugin_id_from_ipc(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => String::new(),
    }
}

/// `pluginPathFromIpc`: string direta ou objeto `{ path: string }`.
pub fn plugin_path_from_ipc(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Object(map) => map
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEnvelope<T> {
    pub ok: bool,
    pub plugins: Vec<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetEnvelope {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginDetailedItem>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// Fachada fina que reproduz os handlers IPC atuais.
pub struct PluginIpc<'a> {
    registry: &'a PluginRegistry,
}

impl<'a> PluginIpc<'a> {
    pub fn new(registry: &'a PluginRegistry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &PluginRegistry {
        self.registry
    }

    pub fn list(&self) -> ListEnvelope<PluginListItem> {
        ListEnvelope {
            ok: true,
            plugins: self.registry.list(),
        }
    }

    pub fn details(&self) -> ListEnvelope<PluginDetailedItem> {
        ListEnvelope {
            ok: true,
            plugins: self.registry.detailed_items(),
        }
    }

    pub fn get(&self, id: &str) -> GetEnvelope {
        match self.registry.get_detailed(id) {
            Some(plugin) => GetEnvelope {
                ok: true,
                plugin: Some(plugin),
                error: String::new(),
            },
            None => GetEnvelope {
                ok: false,
                plugin: None,
                error: "plugin_desconhecido".to_string(),
            },
        }
    }

    pub fn get_value(&self, value: &Value) -> GetEnvelope {
        self.get(&plugin_id_from_ipc(value))
    }

    pub fn register(&self, path: &str) -> RegisterResult {
        self.registry.register(std::path::Path::new(path))
    }

    pub fn register_value(&self, value: &Value) -> RegisterResult {
        self.register(&plugin_path_from_ipc(value))
    }

    pub fn unregister(&self, id: &str) -> MutationResult {
        self.registry.unregister(id)
    }

    pub fn unregister_value(&self, value: &Value) -> MutationResult {
        self.unregister(&plugin_id_from_ipc(value))
    }

    pub fn enable(&self, id: &str) -> MutationResult {
        self.registry.enable(id)
    }

    pub fn enable_value(&self, value: &Value) -> MutationResult {
        self.enable(&plugin_id_from_ipc(value))
    }

    pub fn disable(&self, id: &str) -> MutationResult {
        self.registry.disable(id)
    }

    pub fn disable_value(&self, value: &Value) -> MutationResult {
        self.disable(&plugin_id_from_ipc(value))
    }

    pub fn verify(&self, id: &str) -> VerificationResult {
        self.registry.verify(id)
    }

    pub fn verify_value(&self, value: &Value) -> VerificationResult {
        self.verify(&plugin_id_from_ipc(value))
    }
}
