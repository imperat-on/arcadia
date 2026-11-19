mod common;

use std::fs;
use std::path::Path;

use arcadia_plugins::ipc::{
    plugin_id_from_ipc, plugin_path_from_ipc, PluginIpc, PLUGIN_IPC_CHANNELS,
};
use arcadia_plugins::registry::{standard_builtins, PluginRegistry, RegistryOptions};
use common::TempDir;
use serde_json::{json, Map, Value};

fn write_package(root: &Path, id: &str) -> std::path::PathBuf {
    let directory = root.join(id);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("index.js"), "// ipc\n").unwrap();
    let mut manifest = Map::new();
    manifest.insert("manifestVersion".into(), json!(1));
    manifest.insert("apiVersion".into(), json!(1));
    manifest.insert("id".into(), json!(id));
    manifest.insert("name".into(), json!("IPC demo"));
    manifest.insert("version".into(), json!("1.0.0"));
    manifest.insert("entry".into(), json!("index.js"));
    manifest.insert("permissions".into(), json!(["library:read"]));
    fs::write(
        directory.join("plugin.json"),
        serde_json::to_string(&Value::Object(manifest)).unwrap(),
    )
    .unwrap();
    directory
}

#[test]
fn list_details_and_get_match_the_current_ipc_envelopes() {
    let data = TempDir::new("arcadia-plugin-ipc-list");
    let packages = TempDir::new("arcadia-plugin-ipc-list-pkg");
    let registry = PluginRegistry::with_options(
        RegistryOptions::new(data.path()).builtins(standard_builtins(true, false)),
    )
    .unwrap();
    let path = write_package(packages.path(), "ipc.demo");
    assert!(registry.register(&path).ok);
    assert!(registry.enable("ipc.demo").ok);
    let ipc = PluginIpc::new(&registry);

    let list = serde_json::to_value(ipc.list()).unwrap();
    assert_eq!(list["ok"], json!(true));
    let local = list["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == json!("ipc.demo"))
        .unwrap();
    assert_eq!(
        local,
        &json!({
            "id": "ipc.demo",
            "name": "IPC demo",
            "descKey": "",
            "installed": true,
            "enabled": true
        })
    );

    let details = serde_json::to_value(ipc.details()).unwrap();
    assert_eq!(details["ok"], json!(true));
    let local = details["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == json!("ipc.demo"))
        .unwrap();
    assert_eq!(local["manifest"]["permissions"], json!(["library:read"]));
    assert_eq!(local["valid"], json!(true));
    assert_eq!(local["source"], json!("local"));

    let get = serde_json::to_value(ipc.get("ipc.demo")).unwrap();
    assert_eq!(get["ok"], json!(true));
    assert_eq!(get["plugin"]["id"], json!("ipc.demo"));
    assert!(get.get("error").is_none());
    let missing = serde_json::to_value(ipc.get("ipc.missing")).unwrap();
    assert_eq!(
        missing,
        json!({"ok": false, "error": "plugin_desconhecido"})
    );
    assert_eq!(plugin_id_from_ipc(&json!(42)), String::new());
    assert_eq!(ipc.get_value(&json!(42)).error, "plugin_desconhecido");
}

#[test]
fn register_and_mutations_use_the_ipc_input_normalizers() {
    let data = TempDir::new("arcadia-plugin-ipc-mut");
    let packages = TempDir::new("arcadia-plugin-ipc-mut-pkg");
    let registry = PluginRegistry::new(data.path()).unwrap();
    let path = write_package(packages.path(), "ipc.mut");
    let ipc = PluginIpc::new(&registry);

    assert_eq!(
        plugin_path_from_ipc(&json!({"path": path.to_string_lossy()})),
        path.to_string_lossy()
    );
    assert_eq!(plugin_path_from_ipc(&json!(42)), "");
    assert_eq!(plugin_path_from_ipc(&json!({"path": 3})), "");

    let registered = ipc.register_value(&json!({"path": path.to_string_lossy()}));
    assert!(registered.ok, "{:?}", registered.error);
    let serialized = serde_json::to_value(&registered).unwrap();
    assert_eq!(serialized["ok"], json!(true));
    assert_eq!(serialized["plugin"]["id"], json!("ipc.mut"));
    assert!(serialized["plugin"].get("path").is_none());

    assert_eq!(
        ipc.enable_value(&json!("ipc.mut")),
        arcadia_plugins::registry::MutationResult::success()
    );
    assert_eq!(
        ipc.disable("ipc.mut"),
        arcadia_plugins::registry::MutationResult::success()
    );
    assert_eq!(ipc.enable_value(&json!(42)).error, "plugin_invalido");
    let verified = serde_json::to_value(ipc.verify_value(&json!("ipc.mut"))).unwrap();
    assert_eq!(verified["ok"], json!(true));
    assert!(verified.get("path").is_none());
    assert_eq!(
        ipc.unregister_value(&json!("ipc.mut")),
        arcadia_plugins::registry::MutationResult::success()
    );
    assert!(registry.get("ipc.mut").is_none());
}

#[test]
fn exposes_the_versioned_channel_list_without_legacy_install_remove() {
    assert_eq!(PLUGIN_IPC_CHANNELS.len(), 8);
    assert!(PLUGIN_IPC_CHANNELS.contains(&"plugins:list"));
    assert!(PLUGIN_IPC_CHANNELS.contains(&"plugins:details"));
    assert!(PLUGIN_IPC_CHANNELS.contains(&"plugins:verify"));
    assert!(!PLUGIN_IPC_CHANNELS.contains(&"plugins:install"));
    assert!(!PLUGIN_IPC_CHANNELS.contains(&"plugins:remove"));
}
