mod common;

use std::fs;
use std::path::Path;

use arcadia_plugins::manifest::PLUGIN_PERMISSIONS;
use arcadia_plugins::registry::PluginRegistry;
use arcadia_plugins::sdk::{PluginPermissionError, PluginSdk, SdkError};
use common::TempDir;
use serde_json::{json, Map, Value};

fn write_package(root: &Path, id: &str, permissions: &[&str]) -> std::path::PathBuf {
    let directory = root.join(id);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("index.js"), "// sdk\n").unwrap();
    let mut manifest = Map::new();
    manifest.insert("manifestVersion".into(), json!(1));
    manifest.insert("apiVersion".into(), json!(1));
    manifest.insert("id".into(), json!(id));
    manifest.insert("name".into(), json!("SDK demo"));
    manifest.insert("version".into(), json!("1.0.0"));
    manifest.insert("entry".into(), json!("index.js"));
    manifest.insert("permissions".into(), json!(permissions));
    fs::write(
        directory.join("plugin.json"),
        serde_json::to_string(&Value::Object(manifest)).unwrap(),
    )
    .unwrap();
    directory
}

#[test]
fn capability_rechecks_permission_on_every_call() {
    let data = TempDir::new("arcadia-plugin-sdk");
    let packages = TempDir::new("arcadia-plugin-sdk-pkg");
    let registry = PluginRegistry::new(data.path()).unwrap();
    let path = write_package(packages.path(), "sdk.demo", &["library:read"]);
    assert!(registry.register(&path).ok);

    let sdk = PluginSdk::new(&registry, "sdk.demo").unwrap();
    assert_eq!(sdk.sdk_version(), 1);
    assert_eq!(sdk.plugin_id(), "sdk.demo");
    assert_eq!(sdk.permissions(), &PLUGIN_PERMISSIONS);
    assert_eq!(sdk.manifest().unwrap().name, "SDK demo");
    assert!(!sdk.has_permission("library:read"));

    let read = sdk.capability("library:read", || 42);
    let denied = read().unwrap_err();
    assert_eq!(denied, PluginPermissionError::new("library:read"));
    assert_eq!(denied.code, "permission_denied");
    assert_eq!(denied.to_string(), "permissão não declarada: library:read");

    assert!(registry.enable("sdk.demo").ok);
    assert_eq!(read().unwrap(), 42);
    assert!(sdk.has_permission("library:read"));

    let add = sdk.capability_with("library:read", |value: i32| value + 1);
    assert_eq!(add(1).unwrap(), 2);

    assert!(registry.disable("sdk.demo").ok);
    assert_eq!(read().unwrap_err().code, "permission_denied");
    assert_eq!(add(1).unwrap_err().code, "permission_denied");
    assert_eq!(
        sdk.run("library:read", || "side effect").unwrap_err().code,
        "permission_denied"
    );
    assert!(registry.enable("sdk.demo").ok);
    assert_eq!(sdk.run("library:read", || "ok").unwrap(), "ok");
}

#[test]
fn sdk_rejects_invalid_plugin_ids() {
    let data = TempDir::new("arcadia-plugin-sdk-invalid");
    let registry = PluginRegistry::new(data.path()).unwrap();
    assert_eq!(
        PluginSdk::new(&registry, "").err().unwrap(),
        SdkError::PluginIdInvalid
    );
    assert_eq!(
        PluginSdk::new(&registry, "Not Valid").err().unwrap(),
        SdkError::PluginIdInvalid
    );
    assert_eq!(
        PluginSdk::new(&registry, &"a".repeat(65)).err().unwrap(),
        SdkError::PluginIdInvalid
    );
    assert_eq!(SdkError::PluginIdInvalid.to_string(), "pluginId inválido");
    assert_eq!(
        SdkError::RegistryRequired.to_string(),
        "registry é obrigatório"
    );
}

#[test]
fn sdk_denies_undeclared_and_unknown_plugins() {
    let data = TempDir::new("arcadia-plugin-sdk-denied");
    let packages = TempDir::new("arcadia-plugin-sdk-denied-pkg");
    let registry = PluginRegistry::new(data.path()).unwrap();
    let path = write_package(packages.path(), "sdk.denied", &["library:read"]);
    assert!(registry.register(&path).ok);
    assert!(registry.enable("sdk.denied").ok);

    let sdk = PluginSdk::new(&registry, "sdk.denied").unwrap();
    assert!(!sdk.has_permission("filesystem:write"));
    assert_eq!(
        sdk.assert_permission("filesystem:write").unwrap_err().code,
        "permission_denied"
    );

    let unknown = PluginSdk::new(&registry, "sdk.unknown").unwrap();
    assert!(unknown.manifest().is_none());
    assert!(!unknown.has_permission("library:read"));
    let serialized = serde_json::to_value(unknown.manifest()).unwrap();
    assert_eq!(serialized, Value::Null);
    assert!(sdk.manifest().unwrap().public().entry_sha256.is_none());
}
