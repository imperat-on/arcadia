//! Superfície mínima do SDK do host — port de `app/electron/plugins/sdk.js`.
//!
//! O contexto é intencionalmente pequeno e capability-based: um plugin só
//! recebe uma operação depois que a permissão declarada no manifest foi
//! validada pelo registro. Não há execução de JavaScript; o host Rust entrega
//! closures já guardadas pelas mesmas checagens.

use std::fmt;

use crate::manifest::{normalize_id, PluginManifest, PLUGIN_PERMISSIONS, SDK_API_VERSION};
use crate::registry::PluginRegistry;

/// Erro devolvido quando a capability é chamada sem permissão vigente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPermissionError {
    pub code: &'static str,
    pub permission: String,
}

impl PluginPermissionError {
    pub fn new(permission: &str) -> Self {
        Self {
            code: "permission_denied",
            permission: permission.to_string(),
        }
    }
}

impl fmt::Display for PluginPermissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "permissão não declarada: {}", self.permission)
    }
}

impl std::error::Error for PluginPermissionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdkError {
    RegistryRequired,
    PluginIdInvalid,
}

impl fmt::Display for SdkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdkError::RegistryRequired => write!(formatter, "registry é obrigatório"),
            SdkError::PluginIdInvalid => write!(formatter, "pluginId inválido"),
        }
    }
}

impl std::error::Error for SdkError {}

/// Contexto de SDK de um plugin registrado. As checagens são feitas no momento
/// de cada chamada; desativar o plugin ou revogar a permissão invalida
/// capabilities já criadas.
pub struct PluginSdk<'a> {
    registry: &'a PluginRegistry,
    plugin_id: String,
}

impl<'a> PluginSdk<'a> {
    pub fn new(registry: &'a PluginRegistry, plugin_id: &str) -> Result<Self, SdkError> {
        let id = normalize_id(plugin_id);
        if id.is_empty() {
            return Err(SdkError::PluginIdInvalid);
        }
        Ok(Self {
            registry,
            plugin_id: id,
        })
    }

    pub fn sdk_version(&self) -> u64 {
        SDK_API_VERSION
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// Allowlist completa de permissões v1 (não é o conjunto concedido).
    pub fn permissions(&self) -> &'static [&'static str] {
        &PLUGIN_PERMISSIONS
    }

    pub fn manifest(&self) -> Option<PluginManifest> {
        self.registry
            .get(&self.plugin_id)
            .and_then(|item| item.manifest)
            .map(|manifest| manifest.public())
    }

    pub fn get_manifest(&self) -> Option<PluginManifest> {
        self.manifest()
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        self.registry.has_permission(&self.plugin_id, permission)
    }

    pub fn assert_permission(&self, permission: &str) -> Result<(), PluginPermissionError> {
        if self.has_permission(permission) {
            Ok(())
        } else {
            Err(PluginPermissionError::new(permission))
        }
    }

    /// Cria uma operação sem argumentos que revalida a permissão a cada
    /// chamada.
    pub fn capability<'b, F, T>(
        &'b self,
        permission: &str,
        implementation: F,
    ) -> impl Fn() -> Result<T, PluginPermissionError> + 'b
    where
        F: Fn() -> T + 'b,
    {
        let permission = permission.to_string();
        move || {
            self.assert_permission(&permission)?;
            Ok(implementation())
        }
    }

    /// Variante com um argumento, equivalente ao spread do JS para o caso mais
    /// comum dos adaptadores do host.
    pub fn capability_with<'b, A, F, T>(
        &'b self,
        permission: &str,
        implementation: F,
    ) -> impl Fn(A) -> Result<T, PluginPermissionError> + 'b
    where
        F: Fn(A) -> T + 'b,
    {
        let permission = permission.to_string();
        move |argument: A| {
            self.assert_permission(&permission)?;
            Ok(implementation(argument))
        }
    }

    /// Executa uma operação única sob checagem de permissão.
    pub fn run<T>(
        &self,
        permission: &str,
        operation: impl FnOnce() -> T,
    ) -> Result<T, PluginPermissionError> {
        self.assert_permission(permission)?;
        Ok(operation())
    }
}
