//! Plugins locais: manifest, registro, confiança e SDK (sem executar
//! entrypoints).
//!
//! Port 1:1 dos módulos puros do cliente Electron:
//! `manifest.js`, `registry.js`, `trust.js` e `sdk.js`. Nenhum código de plugin
//! é importado ou executado; o registro apenas valida metadados e calcula o
//! SHA-256 do entry.

pub mod ipc;
pub mod manifest;
pub mod registry;
pub mod sdk;
pub mod trust;

pub use ipc::{
    plugin_id_from_ipc, plugin_path_from_ipc, GetEnvelope, ListEnvelope, PluginIpc,
    PLUGIN_IPC_CHANNELS,
};
pub use manifest::{
    compute_entry_sha256, is_inside, normalize_id, normalize_signing_key_id, parse_manifest,
    parse_manifest_with, public_manifest, read_manifest, read_manifest_with, resolve_path,
    resolve_plugin_entry, validate_id, validate_manifest, validate_manifest_with, EntryDigest,
    EntryResolution, IdValidation, ManifestValidation, PluginManifest, ValidateManifestOptions,
    ENTRY_DIGEST_ALIASES, ENTRY_DIGEST_LENGTH, MANIFEST_FILE, MANIFEST_VERSION,
    MAX_DESCRIPTION_LENGTH, MAX_ENTRY_LENGTH, MAX_ID_LENGTH, MAX_NAME_LENGTH, MAX_PERMISSIONS,
    MAX_VERSION_LENGTH, PLUGIN_PERMISSIONS, SDK_API_VERSION, SIGNATURE_LENGTH,
    SIGNING_KEY_ID_MAX_LENGTH,
};
pub use registry::{
    as_timestamp, digest_matches, normalize_stored_digest, package_is_inside, parse_legacy_state,
    parse_state, safe_state_record, standard_builtins, stored_entry_digest, BuiltinDefinition,
    MutationResult, NowFn, PackageInfo, PluginDescriptor, PluginDetailedItem, PluginListItem,
    PluginRecord, PluginRegistry, RegisterResult, RegistryError, RegistryOptions, RegistryPaths,
    RegistryState, VerificationResult, LEGACY_REGISTRY_RELATIVE, MAX_REGISTRY_ENTRIES,
    REGISTRY_DIRNAME, REGISTRY_FILENAME, REGISTRY_VERSION,
};
pub use sdk::{PluginPermissionError, PluginSdk, SdkError};
pub use trust::{
    atomic_write_json, canonical_payload, canonical_payload_for_manifest, canonical_payload_value,
    decode_signature, decode_signature_bytes, export_public_key, import_public_key,
    looks_like_signature, normalize_key_id, parse_store, read_json_file, resolve_store_path,
    store_path_for_data_dir, verify_manifest_signature, verify_signature, verify_signature_bytes,
    TrustOpResult, TrustProvider, TrustStore, TrustStoreState, TrustedKey, MAX_KEY_ID_LENGTH,
    MAX_TRUSTED_KEYS, TRUST_STORE_FILE, TRUST_STORE_FILENAME, TRUST_STORE_VERSION,
};

pub fn crate_ready() -> bool {
    true
}
