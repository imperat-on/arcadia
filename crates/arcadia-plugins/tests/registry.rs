mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arcadia_plugins::registry::*;
use arcadia_plugins::trust::{canonical_payload, TrustStore};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use common::TempDir;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn base_manifest(id: &str) -> Map<String, Value> {
    let mut manifest = Map::new();
    manifest.insert("manifestVersion".into(), json!(1));
    manifest.insert("apiVersion".into(), json!(1));
    manifest.insert("id".into(), json!(id));
    manifest.insert("name".into(), json!("Demo"));
    manifest.insert("version".into(), json!("1.0.0"));
    manifest.insert("entry".into(), json!("index.js"));
    manifest.insert("permissions".into(), json!(["library:read"]));
    manifest
}

fn write_package(root: &Path, id: &str, source: &str, extra: Map<String, Value>) -> PathBuf {
    let directory = root.join(id);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("index.js"), source).unwrap();
    let mut manifest = base_manifest(id);
    for (key, value) in extra {
        manifest.insert(key, value);
    }
    fs::write(
        directory.join("plugin.json"),
        serde_json::to_string(&Value::Object(manifest)).unwrap(),
    )
    .unwrap();
    directory
}

fn sign(key: &SigningKey, payload: &str) -> String {
    STANDARD.encode(key.sign(payload.as_bytes()).to_bytes())
}

fn keypair(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn builtin(id: &str, installed: bool) -> BuiltinDefinition {
    BuiltinDefinition::from_value(
        &json!({
            "id": id,
            "manifest": {
                "manifestVersion": 1,
                "apiVersion": 1,
                "id": id,
                "name": id,
                "version": "1.0.0",
                "permissions": ["library:read"]
            },
            "installed": installed
        }),
        installed,
    )
    .unwrap()
}

#[test]
fn register_persists_metadata_and_survives_reopen() {
    let data = TempDir::new("arcadia-plugin-registry");
    let packages = TempDir::new("arcadia-plugin-packages");
    let registry = PluginRegistry::new(data.path()).unwrap();
    let source = "// test plugin\n";
    let path = write_package(packages.path(), "demo.plugin", source, Map::new());

    let registered = registry.register(&path);
    assert!(registered.ok, "{:?}", registered.error);
    let plugin = registered.plugin.clone().unwrap();
    assert!(serde_json::to_value(&plugin).unwrap().get("path").is_none());
    assert_eq!(plugin.source, "local");
    assert!(plugin.installed);
    assert!(!plugin.enabled);

    let item = registry.get("demo.plugin").unwrap();
    assert!(item.valid);
    assert_eq!(item.manifest.unwrap().permissions, ["library:read"]);
    assert!(!registry.has_permission("demo.plugin", "library:read"));
    assert_eq!(registry.enable("demo.plugin"), MutationResult::success());
    assert!(registry.has_permission("demo.plugin", "library:read"));
    assert!(!registry.has_permission("demo.plugin", "filesystem:write"));

    let state_file = data.join("plugins/registry.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
    assert_eq!(state["version"], json!(1));
    assert_eq!(state["plugins"]["demo.plugin"]["enabled"], json!(true));
    assert_eq!(
        state["plugins"]["demo.plugin"]["entrySha256"],
        json!(sha256(source.as_bytes()))
    );
    assert_eq!(
        state["plugins"]["demo.plugin"]["path"],
        json!(path.to_string_lossy())
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&state_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let reopened = PluginRegistry::new(data.path()).unwrap();
    assert!(reopened.get("demo.plugin").unwrap().enabled);
    assert_eq!(
        reopened.unregister("demo.plugin"),
        MutationResult::success()
    );
    assert!(reopened.get("demo.plugin").is_none());
    // Desregistrar nunca apaga o pacote do usuário.
    assert!(path.join("plugin.json").exists());
}

#[test]
fn rejects_invalid_packages_and_symlink_roots() {
    let data = TempDir::new("arcadia-plugin-invalid");
    let packages = TempDir::new("arcadia-plugin-invalid-pkg");
    let registry = PluginRegistry::new(data.path()).unwrap();

    let missing = registry.register(packages.join("missing"));
    assert!(!missing.ok);
    assert_eq!(missing.error, "diretorio_nao_encontrado");

    let bad = packages.join("bad");
    fs::create_dir_all(&bad).unwrap();
    let mut manifest = base_manifest("bad");
    manifest.insert("entry".into(), json!("../outside.js"));
    fs::write(
        bad.join("plugin.json"),
        serde_json::to_string(&Value::Object(manifest)).unwrap(),
    )
    .unwrap();
    let rejected = registry.register(&bad);
    assert!(!rejected.ok);
    assert_eq!(rejected.error, "entry_invalido");

    let link = packages.join("link");
    if common::symlink_dir(&bad, &link) {
        let result = registry.register(&link);
        assert!(!result.ok);
        assert_eq!(result.error, "diretorio_invalido");
    }

    // Aceita o próprio plugin.json como entrada para file pickers.
    let good = write_package(packages.path(), "direct.file", "// ok\n", Map::new());
    let result = registry.register(good.join("plugin.json"));
    assert!(result.ok, "{:?}", result.error);
}

#[test]
fn legacy_flags_migrate_only_for_builtins() {
    let data = TempDir::new("arcadia-plugin-legacy");
    fs::create_dir_all(data.join("bin")).unwrap();
    fs::write(
        data.join("bin/plugins.json"),
        serde_json::to_string(&json!({
            "built": {"enabled": true, "updatedAt": 20},
            "mystery": {"enabled": true, "updatedAt": 30}
        }))
        .unwrap(),
    )
    .unwrap();
    let options = RegistryOptions::new(data.path()).builtins(vec![builtin("built", true)]);
    let registry = PluginRegistry::with_options(options).unwrap();
    let item = registry.get("built").unwrap();
    assert!(item.enabled);
    assert!(item.installed);
    assert_eq!(item.source, "builtin");
    assert!(registry.get("mystery").is_none());
    assert!(Path::new(&registry.paths().registry_path)
        .ends_with(Path::new("plugins").join("registry.json")));

    // Legacy mais novo que o registro canônico atualiza somente a flag.
    let data = TempDir::new("arcadia-plugin-legacy-newer");
    fs::create_dir_all(data.join("plugins")).unwrap();
    fs::write(
        data.join("plugins/registry.json"),
        serde_json::to_string(&json!({
            "version": 1,
            "plugins": {
                "built": {"id": "built", "path": "", "enabled": false,
                          "registeredAt": 5, "updatedAt": 10}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::create_dir_all(data.join("bin")).unwrap();
    fs::write(
        data.join("bin/plugins.json"),
        serde_json::to_string(&json!({"built": {"enabled": true, "updatedAt": 20}})).unwrap(),
    )
    .unwrap();
    let registry = PluginRegistry::with_options(
        RegistryOptions::new(data.path()).builtins(vec![builtin("built", true)]),
    )
    .unwrap();
    assert!(registry.get("built").unwrap().enabled);
}

#[test]
fn builtin_redetection_preserves_saved_enabled_choice() {
    let data=TempDir::new("arcadia-plugin-redetect");
    let mut registry=PluginRegistry::with_standard_builtins(data.path(),false,false).unwrap();
    registry.set_builtin_installed("slssteam",true);
    assert!(registry.get("slssteam").unwrap().enabled);
    assert!(registry.disable("slssteam").ok);
    registry.set_builtin_installed("slssteam",false);
    assert!(!registry.get("slssteam").unwrap().installed);
    registry.set_builtin_installed("slssteam",true);
    let restored=registry.get("slssteam").unwrap();
    assert!(restored.installed);assert!(!restored.enabled);
    assert!(registry.enable("slssteam").ok);
    registry.set_builtin_installed("slssteam",false);
    assert!(!registry.get("slssteam").unwrap().enabled);
    registry.set_builtin_installed("slssteam",true);
    assert!(registry.get("slssteam").unwrap().enabled);
}

#[test]
fn standard_builtins_are_flags_and_cannot_be_replaced() {
    let data = TempDir::new("arcadia-plugin-builtins");
    let packages = TempDir::new("arcadia-plugin-builtins-pkg");
    let registry = PluginRegistry::with_options(
        RegistryOptions::new(data.path()).builtins(standard_builtins(false, true)),
    )
    .unwrap();

    let slssteam = registry.get("slssteam").unwrap();
    assert!(!slssteam.installed);
    assert!(!slssteam.enabled);
    assert_eq!(slssteam.source, "builtin");
    let luatools = registry.get("luatools-fixes").unwrap();
    assert!(luatools.installed);
    assert!(luatools.enabled);
    assert_eq!(luatools.manifest.unwrap().name, "LuaTools fixes");

    let ids: Vec<String> = registry
        .list_detailed()
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert_eq!(ids, ["slssteam", "luatools-fixes"]);

    let package = write_package(packages.path(), "slssteam", "// nope\n", Map::new());
    let result = registry.register(&package);
    assert!(!result.ok);
    assert_eq!(result.error, "id_reservado");

    assert_eq!(registry.enable("slssteam").error, "not_detected");
    assert_eq!(
        registry.set_enabled_with("slssteam", true, true),
        MutationResult::success()
    );
    // O registro guarda a escolha, mas um built-in não detectado continua
    // efetivamente desativado (mesma regra do JS).
    assert!(registry.read_state().get("slssteam").unwrap().enabled);
    assert!(!registry.get("slssteam").unwrap().enabled);
    assert_eq!(
        registry.disable("luatools-fixes"),
        MutationResult::success()
    );
    assert!(!registry.get("luatools-fixes").unwrap().enabled);
    assert_eq!(registry.unregister("slssteam").error, "plugin_embutido");
    let verified = registry.verify("slssteam");
    assert!(verified.ok);
    assert_eq!(verified.source, "builtin");
    assert_eq!(verified.expected_digest, "");

    // O caminho informativo de um built-in nunca é lido.
    let malicious = BuiltinDefinition::from_value(
        &json!({
            "id": "never.load",
            "manifest": {
                "manifestVersion": 1,
                "apiVersion": 1,
                "id": "never.load",
                "name": "Never",
                "version": "0.0.0",
                "permissions": []
            },
            "path": "C:/does/not/exist/plugin.json"
        }),
        true,
    )
    .unwrap()
    .with_display(None, "", "C:/does/not/exist/plugin.json");
    let registry =
        PluginRegistry::with_options(RegistryOptions::new(data.path()).builtins(vec![malicious]))
            .unwrap();
    assert!(registry.get("never.load").unwrap().valid);

    // `??` do JS: null cai para os defaults, inclusive permissions/version.
    let coalesced = BuiltinDefinition::from_value(
        &json!({
            "id": "coalesce.builtin",
            "manifest": {
                "manifestVersion": null,
                "apiVersion": null,
                "id": "coalesce.builtin",
                "name": null,
                "version": null,
                "description": null,
                "permissions": null
            }
        }),
        true,
    )
    .unwrap();
    assert_eq!(coalesced.manifest.name, "coalesce.builtin");
    assert_eq!(coalesced.manifest.version, "0.0.0");
    assert_eq!(coalesced.manifest.description, "");
    assert!(coalesced.manifest.permissions.is_empty());
    assert_eq!(coalesced.manifest.manifest_version, 1);
}

#[test]
fn digest_snapshot_detects_tampering_and_blocks_activation() {
    let data = TempDir::new("arcadia-plugin-integrity");
    let packages = TempDir::new("arcadia-plugin-integrity-pkg");
    let source = "// original\n";
    let digest = sha256(source.as_bytes());
    let registry = PluginRegistry::new(data.path()).unwrap();
    let path = write_package(
        packages.path(),
        "integrity.intact",
        source,
        Map::from_iter([("entryDigest".to_string(), json!(digest.to_uppercase()))]),
    );
    let registered = registry.register(&path);
    assert!(registered.ok, "{:?}", registered.error);

    let checked = registry.verify("integrity.intact");
    assert!(checked.ok && checked.valid && checked.verified);
    assert_eq!(checked.algorithm, "sha256");
    assert_eq!(checked.expected_digest, digest);
    assert_eq!(checked.actual_digest, digest);
    assert_eq!(checked.digest, digest);
    assert_eq!(checked.source, "registry");
    assert!(checked.declared);
    assert!(serde_json::to_value(&checked)
        .unwrap()
        .get("path")
        .is_none());

    fs::write(path.join("index.js"), "// tampered\n").unwrap();
    let tampered = registry.verify("integrity.intact");
    assert!(!tampered.ok && !tampered.valid);
    assert_eq!(tampered.error, "entry_digest_mismatch");
    let item = registry.get("integrity.intact").unwrap();
    assert!(!item.valid);
    assert_eq!(item.error, "entry_digest_mismatch");
    assert_eq!(
        registry.set_enabled("integrity.intact", true).error,
        "entry_digest_mismatch"
    );

    // Manifest sem digest: o snapshot do registro continua detectando tamper.
    let legacy_source = "// v1 sem digest\n";
    let legacy_path = write_package(
        packages.path(),
        "integrity.legacy",
        legacy_source,
        Map::new(),
    );
    assert!(registry.register(&legacy_path).ok);
    let legacy = registry.verify("integrity.legacy");
    assert!(legacy.ok && legacy.valid);
    assert!(!legacy.declared);
    assert!(legacy.verified);
    assert_eq!(legacy.source, "registry");
    fs::write(legacy_path.join("index.js"), "// changed\n").unwrap();
    assert_eq!(
        registry.verify("integrity.legacy").error,
        "entry_digest_mismatch"
    );

    let wrong_path = write_package(
        packages.path(),
        "integrity.wrong",
        source,
        Map::from_iter([("entrySha256".to_string(), json!("0".repeat(64)))]),
    );
    let rejected = registry.register(&wrong_path);
    assert!(!rejected.ok);
    assert_eq!(rejected.error, "entry_digest_mismatch");
    assert!(registry.get("integrity.wrong").is_none());
}

#[test]
fn entry_symlink_escape_is_rejected_without_leaking_paths() {
    let data = TempDir::new("arcadia-plugin-symlink");
    let packages = TempDir::new("arcadia-plugin-symlink-pkg");
    let outside = TempDir::new("arcadia-plugin-symlink-outside");
    let external = outside.join("entry.js");
    fs::write(&external, "// external\n").unwrap();
    let directory = packages.join("linked.plugin");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("plugin.json"),
        serde_json::to_string(&Value::Object(base_manifest("linked.plugin"))).unwrap(),
    )
    .unwrap();
    if !common::symlink_file(&external, &directory.join("index.js")) {
        return;
    }
    let registry = PluginRegistry::new(data.path()).unwrap();
    let result = registry.register(&directory);
    assert!(!result.ok);
    assert!(
        matches!(
            result.error.as_str(),
            "entry_invalido" | "entry_fora_do_plugin"
        ),
        "{:?}",
        result.error
    );
    let verified = registry.verify_package(&directory);
    assert!(!verified.ok);
    assert!(serde_json::to_value(&verified)
        .unwrap()
        .get("path")
        .is_none());
}

#[test]
fn signed_plugins_require_a_trusted_key_and_valid_signature() {
    let data = TempDir::new("arcadia-plugin-signed");
    let packages = TempDir::new("arcadia-plugin-signed-pkg");
    let key = keypair(41);
    let trust = TrustStore::for_data_dir(data.path());
    assert!(trust.add("vendor-key", &key.verifying_key()).ok);
    let registry = PluginRegistry::new(data.path()).unwrap();

    let source = "// signed\n";
    let digest = sha256(source.as_bytes());
    let signature = sign(&key, &canonical_payload("signed.plugin", "1.0.0", &digest));
    let path = write_package(
        packages.path(),
        "signed.plugin",
        source,
        Map::from_iter([
            ("entrySha256".to_string(), json!(digest)),
            ("signingKeyId".to_string(), json!("vendor-key")),
            ("signature".to_string(), json!(signature)),
        ]),
    );
    let registered = registry.register(&path);
    assert!(registered.ok, "{:?}", registered.error);
    let verified = registry.verify("signed.plugin");
    assert!(verified.ok);
    assert_eq!(verified.signature_algorithm.as_deref(), Some("ed25519"));
    assert_eq!(verified.signature_verified, Some(true));
    assert_eq!(verified.signature_valid, Some(true));
    assert_eq!(verified.signing_key_id.as_deref(), Some("vendor-key"));
    assert!(serde_json::to_value(&verified)
        .unwrap()
        .get("path")
        .is_none());

    fs::write(path.join("index.js"), "// tampered\n").unwrap();
    let tampered = registry.verify("signed.plugin");
    assert!(!tampered.ok);
    assert!(matches!(
        tampered.error.as_str(),
        "entry_digest_mismatch" | "assinatura_invalida"
    ));

    let unknown_digest = sha256(b"// unknown\n");
    let unknown_signature = sign(
        &key,
        &canonical_payload("unknown.plugin", "1.0.0", &unknown_digest),
    );
    let unknown = write_package(
        packages.path(),
        "unknown.plugin",
        "// unknown\n",
        Map::from_iter([
            ("entrySha256".to_string(), json!(unknown_digest)),
            ("signingKeyId".to_string(), json!("missing-key")),
            ("signature".to_string(), json!(unknown_signature)),
        ]),
    );
    let rejected = registry.register(&unknown);
    assert!(!rejected.ok);
    assert_eq!(rejected.error, "assinatura_chave_nao_confiavel");

    let bad_digest = sha256(b"// bad signature\n");
    let bad_signature = sign(
        &key,
        &canonical_payload("bad.signature", "9.9.9", &bad_digest),
    );
    let bad = write_package(
        packages.path(),
        "bad.signature",
        "// bad signature\n",
        Map::from_iter([
            ("entrySha256".to_string(), json!(bad_digest)),
            ("signingKeyId".to_string(), json!("vendor-key")),
            ("signature".to_string(), json!(bad_signature)),
        ]),
    );
    let rejected = registry.register(&bad);
    assert!(!rejected.ok);
    assert_eq!(rejected.error, "assinatura_invalida");
    assert!(registry.get("bad.signature").is_none());
}

#[test]
fn preserves_unknown_state_fields_and_writes_atomically() {
    let data = TempDir::new("arcadia-plugin-unknown");
    let packages = TempDir::new("arcadia-plugin-unknown-pkg");
    let registry = PluginRegistry::new(data.path()).unwrap();
    let path = write_package(packages.path(), "keep.plugin", "// keep\n", Map::new());
    assert!(registry.register(&path).ok);

    let file = data.join("plugins/registry.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    value["futureTopLevel"] = json!({"kept": true});
    value["plugins"]["keep.plugin"]["recordExtra"] = json!(42);
    fs::write(&file, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    registry.invalidate();
    assert_eq!(registry.enable("keep.plugin"), MutationResult::success());
    let rewritten: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(rewritten["futureTopLevel"], json!({"kept": true}));
    assert_eq!(
        rewritten["plugins"]["keep.plugin"]["recordExtra"],
        json!(42)
    );
    assert!(fs::read_to_string(&file).unwrap().ends_with('\n'));

    let leftovers: Vec<_> = fs::read_dir(data.join("plugins"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn registry_write_failure_is_reported_and_does_not_mutate_state() {
    let data = TempDir::new("arcadia-plugin-writefail");
    let packages = TempDir::new("arcadia-plugin-writefail-pkg");
    let path = write_package(packages.path(), "fail.plugin", "// fail\n", Map::new());
    fs::create_dir_all(data.join("plugins/registry.json")).unwrap();
    let registry = PluginRegistry::new(data.path()).unwrap();
    let result = registry.register(&path);
    assert!(!result.ok);
    assert_eq!(result.error, "registro_nao_gravavel");
    assert!(registry.get("fail.plugin").is_none());
    let leftovers: Vec<_> = fs::read_dir(data.join("plugins"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn invalid_state_is_ignored_and_state_limits_are_respected() {
    let data = TempDir::new("arcadia-plugin-state");
    fs::create_dir_all(data.join("plugins")).unwrap();
    fs::write(data.join("plugins/registry.json"), "not-json").unwrap();
    let registry = PluginRegistry::new(data.path()).unwrap();
    assert!(registry.list_detailed().is_empty());

    fs::write(
        data.join("plugins/registry.json"),
        serde_json::to_string(&json!({"version": 2, "plugins": {"x": {}}})).unwrap(),
    )
    .unwrap();
    registry.invalidate();
    assert!(registry.list_detailed().is_empty());

    let mut plugins = Map::new();
    plugins.insert("BAD".into(), json!({"path": ""}));
    plugins.insert("nulpath".into(), json!({"path": "bad\0path"}));
    for index in 0..300 {
        let id = format!("p{index}");
        plugins.insert(
            id.clone(),
            json!({"id": id, "path": "", "enabled": true, "registeredAt": 1, "updatedAt": 1}),
        );
    }
    fs::write(
        data.join("plugins/registry.json"),
        serde_json::to_string(&json!({"version": 1, "plugins": plugins})).unwrap(),
    )
    .unwrap();
    let registry = PluginRegistry::new(data.path()).unwrap();
    let state = registry.read_state();
    // Dois registros inválidos no começo e o limite de 256 entradas.
    assert_eq!(state.plugins.len(), 254);
    assert!(state.plugins.len() <= MAX_REGISTRY_ENTRIES);
    assert!(!state.plugins.iter().any(|record| record.id == "BAD"));
    assert!(!state.plugins.iter().any(|record| record.id == "nulpath"));

    let legacy = parse_legacy_state(&json!({
        "ok": {"enabled": true, "updatedAt": "20"},
        "BAD": {"enabled": true},
        "nope": 3
    }));
    assert_eq!(legacy.len(), 1);
    assert_eq!(legacy[0].id, "ok");
    assert!(legacy[0].enabled);
    assert_eq!(legacy[0].updated_at, 20);
}

#[test]
fn manifest_id_divergence_invalidates_the_registration() {
    let data = TempDir::new("arcadia-plugin-divergent");
    let packages = TempDir::new("arcadia-plugin-divergent-pkg");
    let registry = PluginRegistry::new(data.path()).unwrap();
    let path = write_package(packages.path(), "identity.one", "// one\n", Map::new());
    assert!(registry.register(&path).ok);

    let mut manifest = base_manifest("identity.two");
    manifest.insert("entry".into(), json!("index.js"));
    fs::write(
        path.join("plugin.json"),
        serde_json::to_string(&Value::Object(manifest)).unwrap(),
    )
    .unwrap();

    let item = registry.get("identity.one").unwrap();
    assert!(!item.valid);
    assert!(!item.installed);
    assert_eq!(item.error, "id_divergente");
    assert!(item.manifest.is_none());
    assert_eq!(registry.enable("identity.one").error, "id_divergente");
}

#[test]
fn registry_state_symlink_is_ignored() {
    let data = TempDir::new("arcadia-plugin-state-symlink");
    let outside = TempDir::new("arcadia-plugin-state-symlink-outside");
    fs::create_dir_all(data.join("plugins")).unwrap();
    let external = outside.join("registry.json");
    fs::write(
        &external,
        serde_json::to_string(&json!({
            "version": 1,
            "plugins": {
                "ghost.plugin": {
                    "id": "ghost.plugin", "path": "", "enabled": true,
                    "registeredAt": 1, "updatedAt": 1
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    if !common::symlink_file(&external, &data.join("plugins/registry.json")) {
        return;
    }
    let registry = PluginRegistry::new(data.path()).unwrap();
    assert!(registry.list_detailed().is_empty());
}

#[test]
fn duplicate_ids_and_reenrollment_keep_registration_time() {
    let data = TempDir::new("arcadia-plugin-duplicate");
    let packages = TempDir::new("arcadia-plugin-duplicate-pkg");
    let now = Arc::new(|| 1000_i64);
    let registry =
        PluginRegistry::with_options(RegistryOptions::new(data.path()).now(now)).unwrap();
    let first = write_package(packages.path(), "dup.plugin", "// first\n", Map::new());
    assert!(registry.register(&first).ok);
    let other_packages = TempDir::new("arcadia-plugin-duplicate-pkg-2");
    let second = write_package(
        other_packages.path(),
        "dup.plugin",
        "// second\n",
        Map::new(),
    );
    let duplicate = registry.register(&second);
    assert!(!duplicate.ok);
    assert_eq!(duplicate.error, "id_ja_registrado");

    let again = registry.register_with_enabled(&first, true);
    assert!(again.ok, "{:?}", again.error);
    assert!(again.plugin.unwrap().enabled);
    let state: Value =
        serde_json::from_str(&fs::read_to_string(data.join("plugins/registry.json")).unwrap())
            .unwrap();
    assert_eq!(state["plugins"]["dup.plugin"]["registeredAt"], json!(1000));
    assert_eq!(state["plugins"]["dup.plugin"]["updatedAt"], json!(1000));
}

#[test]
fn verify_reports_exact_errors_and_never_leaks_paths() {
    let data = TempDir::new("arcadia-plugin-verify");
    let packages = TempDir::new("arcadia-plugin-verify-pkg");
    let registry = PluginRegistry::with_options(
        RegistryOptions::new(data.path()).builtins(standard_builtins(true, false)),
    )
    .unwrap();

    assert_eq!(registry.verify("").error, "plugin_invalido");
    assert_eq!(
        registry.verify("unknown.plugin").error,
        "plugin_desconhecido"
    );
    assert_eq!(
        registry.verify_package(packages.join("nope")).error,
        "diretorio_nao_encontrado"
    );
    let serialized = serde_json::to_value(registry.verify("unknown.plugin")).unwrap();
    assert!(serialized.get("path").is_none());
    assert_eq!(serialized["algorithm"], json!("sha256"));
    assert_eq!(serialized["source"], json!("none"));

    let builtin = registry.verify("slssteam");
    assert!(builtin.ok);
    assert_eq!(builtin.source, "builtin");
    assert!(!builtin.verified);

    let path = write_package(packages.path(), "direct.verify", "// direct\n", Map::new());
    let direct = registry.verify_package(&path);
    assert!(direct.ok);
    // Sem digest declarado nem snapshot de registro, o JS reporta source
    // "none" e verified=false, mas ainda devolve o digest calculado.
    assert_eq!(direct.source, "none");
    assert!(!direct.declared);
    assert!(!direct.verified);
    assert!(direct.expected_digest.is_empty());
    assert!(!direct.actual_digest.is_empty());
}

#[test]
fn list_and_detailed_shapes_match_the_ipc_facade() {
    let data = TempDir::new("arcadia-plugin-shapes");
    let packages = TempDir::new("arcadia-plugin-shapes-pkg");
    let registry = PluginRegistry::with_options(
        RegistryOptions::new(data.path()).builtins(standard_builtins(true, false)),
    )
    .unwrap();
    let path = write_package(packages.path(), "shape.plugin", "// shape\n", Map::new());
    assert!(registry.register(&path).ok);
    assert!(registry.enable("shape.plugin").ok);

    let list = registry.list();
    let builtin = list.iter().find(|item| item.id == "slssteam").unwrap();
    assert_eq!(builtin.name, "plugins.slssteam_nome");
    assert_eq!(builtin.desc_key, "plugins.slssteam_desc");
    let local = list.iter().find(|item| item.id == "shape.plugin").unwrap();
    assert_eq!(local.name, "Demo");
    assert_eq!(local.desc_key, "");
    assert!(local.installed && local.enabled);
    let value = serde_json::to_value(local).unwrap();
    assert_eq!(
        value,
        json!({
            "id": "shape.plugin",
            "name": "Demo",
            "descKey": "",
            "installed": true,
            "enabled": true
        })
    );

    let detailed = registry.detailed_items();
    let local = detailed
        .iter()
        .find(|item| item.id == "shape.plugin")
        .unwrap();
    let value = serde_json::to_value(local).unwrap();
    assert_eq!(value["manifest"]["id"], json!("shape.plugin"));
    assert_eq!(value["valid"], json!(true));
    assert_eq!(value["source"], json!("local"));
    assert!(registry.get_detailed("shape.plugin").is_some());
    assert!(registry.get_detailed("").is_none());
}

#[test]
fn mutation_errors_and_temp_isolation() {
    let first = TempDir::new("arcadia-plugin-isolation-a");
    let second = TempDir::new("arcadia-plugin-isolation-b");
    let packages = TempDir::new("arcadia-plugin-isolation-pkg");
    let registry_a = PluginRegistry::new(first.path()).unwrap();
    let registry_b = PluginRegistry::new(second.path()).unwrap();
    let path = write_package(packages.path(), "iso.plugin", "// iso\n", Map::new());
    assert!(registry_a.register(&path).ok);
    assert!(registry_b.get("iso.plugin").is_none());

    assert_eq!(registry_a.enable("").error, "plugin_invalido");
    assert_eq!(registry_a.enable("missing").error, "plugin_desconhecido");
    assert_eq!(registry_a.unregister("").error, "plugin_invalido");
    assert_eq!(
        registry_a.unregister("missing").error,
        "plugin_desconhecido"
    );
}

#[test]
fn public_descriptors_never_contain_private_paths() {
    let data = TempDir::new("arcadia-plugin-nopath");
    let packages = TempDir::new("arcadia-plugin-nopath-pkg");
    let registry = PluginRegistry::new(data.path()).unwrap();
    let path = write_package(packages.path(), "nopath.plugin", "// nopath\n", Map::new());
    let result = registry.register(&path);
    let value = serde_json::to_value(&result).unwrap();
    let text = value.to_string();
    assert!(!text.contains(&path.to_string_lossy().to_string()));
    assert!(value.get("path").is_none());
    assert!(value["plugin"].get("path").is_none());
}
