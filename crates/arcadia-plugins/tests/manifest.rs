mod common;

use std::fs;

use arcadia_plugins::manifest::*;
use common::TempDir;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn base(id: &str) -> serde_json::Map<String, Value> {
    let mut manifest = serde_json::Map::new();
    manifest.insert("manifestVersion".into(), json!(1));
    manifest.insert("apiVersion".into(), json!(1));
    manifest.insert("id".into(), json!(id));
    manifest.insert("name".into(), json!("Demo"));
    manifest.insert("version".into(), json!("1.2.3"));
    manifest.insert("entry".into(), json!("index.js"));
    manifest
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn canonicalizes_legacy_aliases_and_permissions() {
    let result = validate_manifest(&json!({
        "manifest_version": 1,
        "api_version": "1",
        "id": "com.arcadia.demo",
        "name": "Demo",
        "version": "1.2.3",
        "entrypoint": "index.js",
        "permissions": ["library:read", "events:subscribe"]
    }));
    assert!(result.ok, "{:?}", result.errors);
    let manifest = result.manifest.unwrap();
    assert_eq!(manifest.manifest_version, 1);
    assert_eq!(manifest.api_version, 1);
    assert_eq!(manifest.entry, "index.js");
    assert_eq!(manifest.description, "");
    assert_eq!(manifest.permissions, ["library:read", "events:subscribe"]);
    let value = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        value,
        json!({
            "manifestVersion": 1,
            "apiVersion": 1,
            "id": "com.arcadia.demo",
            "name": "Demo",
            "version": "1.2.3",
            "description": "",
            "entry": "index.js",
            "permissions": ["library:read", "events:subscribe"]
        })
    );
    assert!(value.get("manifest_version").is_none());
    assert!(value.get("entrypoint").is_none());
}

#[test]
fn accepts_every_digest_alias_and_canonicalizes_case() {
    for alias in ENTRY_DIGEST_ALIASES {
        let digest = "AB".repeat(32);
        let mut manifest = base("digest.demo");
        manifest.insert(alias.to_string(), json!(digest));
        let result = validate_manifest(&Value::Object(manifest));
        assert!(result.ok, "{alias}: {:?}", result.errors);
        let manifest = result.manifest.unwrap();
        assert_eq!(
            manifest.entry_sha256.as_deref(),
            Some("ab".repeat(32).as_str())
        );
        assert_eq!(manifest.entry_digest(), manifest.entry_sha256.as_deref());
        let value = serde_json::to_value(&manifest).unwrap();
        assert!(value.get("entryDigest").is_none());
        assert!(value.get("entry_digest").is_none());
    }

    let mut duplicate = base("digest.demo");
    duplicate.insert("entrySha256".into(), json!("a".repeat(64)));
    duplicate.insert("entryDigest".into(), json!("b".repeat(64)));
    let result = validate_manifest(&Value::Object(duplicate));
    assert!(!result.ok);
    assert!(result
        .errors
        .contains(&"campo_duplicado:entrySha256".to_string()));
}

#[test]
fn rejects_invalid_digest_values() {
    for value in [
        json!(""),
        json!("not-hex"),
        json!("a".repeat(63)),
        json!("g".repeat(64)),
        json!(42),
        json!(null),
    ] {
        let mut manifest = base("digest.demo");
        manifest.insert("entrySha256".into(), value.clone());
        let result = validate_manifest(&Value::Object(manifest));
        assert!(!result.ok, "{value}");
        assert!(
            result.errors.contains(&"entry_digest_invalido".to_string()),
            "{value}: {:?}",
            result.errors
        );
    }
}

#[test]
fn rejects_version_permissions_and_unknown_fields() {
    let mut manifest = base("demo");
    manifest.insert("manifestVersion".into(), json!(2));
    manifest.insert(
        "permissions".into(),
        json!(["filesystem:*", PLUGIN_PERMISSIONS[0], PLUGIN_PERMISSIONS[0]]),
    );
    manifest.insert("execute".into(), json!("node index.js"));
    let result = validate_manifest(&Value::Object(manifest));
    assert!(!result.ok);
    assert!(result
        .errors
        .contains(&"manifest_version_invalida".to_string()));
    assert!(result
        .errors
        .iter()
        .any(|error| error.starts_with("permission_nao_suportada:")));
    assert!(result
        .errors
        .iter()
        .any(|error| error.starts_with("permission_duplicada:")));
    assert!(result
        .errors
        .iter()
        .any(|error| error.starts_with("campo_desconhecido:")));

    let mut too_many = base("demo");
    too_many.insert("permissions".into(), json!(vec!["library:read"; 33]));
    let result = validate_manifest(&Value::Object(too_many));
    assert!(result.errors.contains(&"permissions_invalido".to_string()));

    let mut not_array = base("demo");
    not_array.insert("permissions".into(), json!("library:read"));
    let result = validate_manifest(&Value::Object(not_array));
    assert!(result.errors.contains(&"permissions_invalido".to_string()));

    let mut invalid_entry = base("demo");
    invalid_entry.insert("permissions".into(), json!([42]));
    let result = validate_manifest(&Value::Object(invalid_entry));
    assert!(result.errors.contains(&"permission_invalida".to_string()));
}

#[test]
fn rejects_invalid_ids_and_entry_paths() {
    for id in [
        "../plugin",
        "Plugin",
        "",
        ".",
        &"a".repeat(65),
        "with space",
        "a/b",
        "a\\b",
    ] {
        let manifest = base(id);
        let result = validate_manifest(&Value::Object(manifest));
        assert!(!result.ok, "{id}");
        assert!(result.errors.contains(&"id_invalido".to_string()), "{id}");
    }

    for entry in [
        "../index.js",
        "/abs/index.js",
        "C:/abs/index.js",
        "C:\\abs\\index.js",
        "a\\b.js",
        "a/../b.js",
        "./index.js",
        "a//b.js",
        "a/",
        ".",
        "index.js\0",
    ] {
        let mut manifest = base("demo");
        manifest.insert("entry".into(), json!(entry));
        let result = validate_manifest(&Value::Object(manifest));
        assert!(!result.ok, "{entry:?}");
        assert!(
            result.errors.contains(&"entry_invalido".to_string()),
            "{entry:?}: {:?}",
            result.errors
        );
    }

    let mut nested = base("demo");
    nested.insert("entry".into(), json!("lib/deep/index.js"));
    assert!(validate_manifest(&Value::Object(nested)).ok);

    let mut too_long = base("demo");
    too_long.insert("entry".into(), json!("a".repeat(241)));
    let result = validate_manifest(&Value::Object(too_long));
    assert!(result.errors.contains(&"entry_muito_longo".to_string()));
}

#[test]
fn mirrors_js_dotdot_manifest_quirk_but_resolves_outside() {
    let temp = TempDir::new("arcadia-plugin-quirk");
    let mut manifest = base("demo");
    manifest.insert("entry".into(), json!(".."));
    // O validador puro do JS aceita ".."; a resolução do pacote rejeita.
    assert!(validate_manifest(&Value::Object(manifest)).ok);
    let resolution = resolve_plugin_entry(temp.path(), "..");
    assert!(!resolution.ok);
    assert_eq!(resolution.error, "entry_fora_do_plugin");
}

#[test]
fn compute_hash_reads_only_bytes_and_detects_external_symlink() {
    let root = TempDir::new("arcadia-plugin-hash");
    let outside = TempDir::new("arcadia-plugin-hash-outside");
    let entry = root.join("index.js");
    let source = b"globalThis.__arcadia_hash_must_not_run = true\n";
    fs::write(&entry, source).unwrap();

    let result = compute_entry_sha256(&entry, Some(root.path()));
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(result.digest, sha256(source));

    let external = outside.join("index.js");
    fs::write(&external, source).unwrap();
    fs::remove_file(&entry).unwrap();
    if common::symlink_file(&external, &entry) {
        let linked = compute_entry_sha256(&entry, Some(root.path()));
        assert!(!linked.ok);
        assert_eq!(linked.error, "entry_fora_do_plugin");
    }

    let missing = compute_entry_sha256(&root.join("missing.js"), Some(root.path()));
    assert!(!missing.ok);
    assert_eq!(missing.error, "entry_nao_verificavel");
}

#[test]
fn resolve_entry_rejects_symlink_escape_missing_and_root_symlink() {
    let root = TempDir::new("arcadia-plugin-entry");
    let outside = TempDir::new("arcadia-plugin-entry-outside");
    let plugin = root.join("plugin");
    fs::create_dir_all(&plugin).unwrap();
    fs::write(plugin.join("index.js"), "// ok\n").unwrap();
    let valid = resolve_plugin_entry(&plugin, "index.js");
    assert!(valid.ok, "{}", valid.error);
    assert_eq!(valid.path.unwrap(), plugin.join("index.js"));

    let missing = resolve_plugin_entry(&plugin, "missing.js");
    assert!(!missing.ok);
    assert_eq!(missing.error, "entry_nao_encontrado");

    let external = outside.join("entry.js");
    fs::write(&external, "").unwrap();
    let link = plugin.join("entry.js");
    if common::symlink_file(&external, &link) {
        let escaped = resolve_plugin_entry(&plugin, "entry.js");
        assert!(!escaped.ok);
        assert!(matches!(
            escaped.error.as_str(),
            "entry_invalido" | "entry_fora_do_plugin"
        ));
    }

    let root_link = root.join("plugin-link");
    if common::symlink_dir(&plugin, &root_link) {
        let linked_root = resolve_plugin_entry(&root_link, "index.js");
        assert!(!linked_root.ok);
    }

    let absolute = resolve_plugin_entry(&plugin, "/index.js");
    assert!(!absolute.ok);
    assert_eq!(absolute.error, "entry_invalido");
}

#[test]
fn read_manifest_rejects_symlinks_and_reports_missing() {
    let root = TempDir::new("arcadia-plugin-read");
    let file = root.join("plugin.json");
    fs::write(
        &file,
        serde_json::to_string(&Value::Object(base("read.demo"))).unwrap(),
    )
    .unwrap();
    let result = read_manifest(&file);
    assert!(result.ok);

    let outside = TempDir::new("arcadia-plugin-read-outside");
    let external = outside.join("plugin.json");
    fs::write(&external, "{}").unwrap();
    let link = root.join("linked.json");
    if common::symlink_file(&external, &link) {
        let linked = read_manifest(&link);
        assert!(!linked.ok);
        assert_eq!(linked.errors, ["manifest_invalido"]);
    }

    let missing = read_manifest(&root.join("nope.json"));
    assert_eq!(missing.errors, ["manifest_nao_encontrado"]);
    let directory = read_manifest(root.path());
    assert_eq!(directory.errors, ["manifest_invalido"]);
    let empty = read_manifest(std::path::Path::new(""));
    assert_eq!(empty.errors, ["caminho_invalido"]);
}

#[test]
fn enforces_field_lengths_required_fields_and_nul() {
    let missing_name = {
        let mut manifest = base("demo");
        manifest.remove("name");
        manifest
    };
    let result = validate_manifest(&Value::Object(missing_name));
    assert!(result.errors.contains(&"name_invalido".to_string()));

    let mut empty_name = base("demo");
    empty_name.insert("name".into(), json!(""));
    let result = validate_manifest(&Value::Object(empty_name));
    assert!(result.errors.contains(&"name_obrigatorio".to_string()));

    let mut long_name = base("demo");
    long_name.insert("name".into(), json!("a".repeat(121)));
    let result = validate_manifest(&Value::Object(long_name));
    assert!(result.errors.contains(&"name_muito_longo".to_string()));

    let mut bad_description = base("demo");
    bad_description.insert("description".into(), json!(null));
    let result = validate_manifest(&Value::Object(bad_description));
    assert!(result.errors.contains(&"description_invalido".to_string()));

    let mut long_description = base("demo");
    long_description.insert("description".into(), json!("a".repeat(1001)));
    let result = validate_manifest(&Value::Object(long_description));
    assert!(result
        .errors
        .contains(&"description_muito_longo".to_string()));

    let mut bad_version = base("demo");
    bad_version.insert("version".into(), json!("01.0.0"));
    let result = validate_manifest(&Value::Object(bad_version));
    assert!(result.errors.contains(&"version_invalida".to_string()));

    let mut long_version = base("demo");
    long_version.insert("version".into(), json!(format!("{}.0.0", "1".repeat(62))));
    let result = validate_manifest(&Value::Object(long_version));
    assert!(result.errors.contains(&"version_muito_longo".to_string()));

    let mut nul_name = base("demo");
    nul_name.insert("name".into(), json!("bad\0name"));
    let result = validate_manifest(&Value::Object(nul_name));
    assert!(result.errors.contains(&"name_invalido".to_string()));

    let mut nul_entry = base("demo");
    nul_entry.insert("entry".into(), json!("index\0.js"));
    let result = validate_manifest(&Value::Object(nul_entry));
    assert!(result.errors.contains(&"entry_invalido".to_string()));
}

#[test]
fn validates_signing_pair_strictly() {
    let mut missing_signature = base("signed.demo");
    missing_signature.insert("signingKeyId".into(), json!("vendor-key"));
    let result = validate_manifest(&Value::Object(missing_signature));
    assert!(!result.ok);
    assert!(result.errors.contains(&"assinatura_incompleta".to_string()));
    assert!(result.errors.contains(&"signature_obrigatoria".to_string()));

    let mut missing_key = base("signed.demo");
    missing_key.insert("signature".into(), json!("A".repeat(88)));
    let result = validate_manifest(&Value::Object(missing_key));
    assert!(!result.ok);
    assert!(result.errors.contains(&"assinatura_incompleta".to_string()));
    assert!(result
        .errors
        .contains(&"signing_key_id_obrigatorio".to_string()));

    let mut bad_key = base("signed.demo");
    bad_key.insert("signingKeyId".into(), json!("vendor/key"));
    bad_key.insert("signature".into(), json!("not-a-signature"));
    let result = validate_manifest(&Value::Object(bad_key));
    assert!(result
        .errors
        .contains(&"signing_key_id_invalido".to_string()));
    assert!(result.errors.contains(&"signature_invalida".to_string()));

    let valid_signature = {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine as _;
        STANDARD.encode([0_u8; 64])
    };
    let mut valid = base("signed.demo");
    valid.insert("signingKeyId".into(), json!("vendor-key"));
    valid.insert("signature".into(), json!(valid_signature));
    let result = validate_manifest(&Value::Object(valid));
    assert!(result.ok, "{:?}", result.errors);
    let manifest = result.manifest.unwrap();
    assert!(manifest.is_signed());
    assert_eq!(
        manifest.signature.as_deref(),
        Some(valid_signature.as_str())
    );

    let mut spaced_signature = base("signed.demo");
    spaced_signature.insert("signingKeyId".into(), json!("vendor-key"));
    spaced_signature.insert("signature".into(), json!(format!(" {valid_signature}")));
    assert!(!validate_manifest(&Value::Object(spaced_signature)).ok);
}

#[test]
fn parses_json_and_rejects_prototype_and_duplicate_fields() {
    assert_eq!(parse_manifest("not-json").errors, ["json_invalido"]);

    let mut prototype = base("demo");
    prototype.insert("__proto__".into(), json!({}));
    prototype.insert("constructor".into(), json!("x"));
    let result = validate_manifest(&Value::Object(prototype));
    assert!(result.errors.contains(&"campo_invalido".to_string()));

    let mut duplicate_entry = base("demo");
    duplicate_entry.insert("entrypoint".into(), json!("other.js"));
    let result = validate_manifest(&Value::Object(duplicate_entry));
    assert!(result.errors.contains(&"campo_duplicado:entry".to_string()));

    let mut duplicate_version = base("demo");
    duplicate_version.insert("schemaVersion".into(), json!(1));
    let result = validate_manifest(&Value::Object(duplicate_version));
    assert!(result
        .errors
        .contains(&"campo_duplicado:manifestVersion".to_string()));

    let mut bad_api = base("demo");
    bad_api.insert("apiVersion".into(), json!(2));
    let result = validate_manifest(&Value::Object(bad_api));
    assert!(result.errors.contains(&"api_version_invalida".to_string()));

    let mut bad_manifest_version = base("demo");
    bad_manifest_version.remove("manifestVersion");
    let result = validate_manifest(&Value::Object(bad_manifest_version));
    assert!(result
        .errors
        .contains(&"manifest_version_invalida".to_string()));
}

#[test]
fn public_manifest_has_no_private_fields_and_copies_permissions() {
    let mut manifest = base("public.demo");
    manifest.insert("permissions".into(), json!(["library:read"]));
    let manifest = validate_manifest(&Value::Object(manifest))
        .manifest
        .unwrap();
    let public = public_manifest(&manifest);
    let value = serde_json::to_value(&public).unwrap();
    assert!(value.get("path").is_none());
    assert_eq!(value["permissions"], json!(["library:read"]));
    assert!(value.get("signingKeyId").is_none());
    assert!(value.get("signature").is_none());
    assert_eq!(public, manifest.public());
}

#[test]
fn validate_id_and_key_id_helpers_match_js_rules() {
    assert_eq!(normalize_id("  demo.plugin  "), "demo.plugin");
    assert_eq!(normalize_id("Demo"), "");
    assert_eq!(normalize_id(&"a".repeat(64)), "a".repeat(64));
    assert_eq!(normalize_id(&"a".repeat(65)), "");
    assert!(validate_id("a").ok);
    assert!(!validate_id("").ok);
    assert_eq!(normalize_signing_key_id(" vendor-key "), "vendor-key");
    assert_eq!(normalize_signing_key_id("vendor/key"), "");
    assert_eq!(normalize_signing_key_id(""), "");
}

#[test]
fn builtin_validation_allows_missing_entry() {
    let value = json!({
        "manifestVersion": 1,
        "apiVersion": 1,
        "id": "builtin.demo",
        "name": "Builtin",
        "version": "0.0.0",
        "permissions": ["library:read"]
    });
    assert!(!validate_manifest(&value).ok);
    let result = validate_manifest_with(&value, ValidateManifestOptions::without_entry());
    assert!(result.ok, "{:?}", result.errors);
    assert_eq!(result.manifest.unwrap().entry, "");
}
