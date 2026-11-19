mod common;

use std::fs;

use arcadia_plugins::manifest::validate_manifest;
use arcadia_plugins::trust::*;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use common::TempDir;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;

fn keypair(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn sign(key: &SigningKey, payload: &str) -> String {
    STANDARD.encode(key.sign(payload.as_bytes()).to_bytes())
}

#[test]
fn canonical_payload_is_exact_and_verifies_ed25519() {
    let key = keypair(7);
    let payload = canonical_payload("demo.plugin", "1.2.3", &"ab".repeat(32));
    assert_eq!(
        payload,
        format!(
            "{{\"id\":\"demo.plugin\",\"version\":\"1.2.3\",\"entrySha256\":\"{}\"}}",
            "ab".repeat(32)
        )
    );
    let signature = sign(&key, &payload);
    assert!(verify_signature(&payload, &signature, &key.verifying_key()));
    let mut bad = signature.clone();
    bad.replace_range(0..1, if bad.starts_with('A') { "B" } else { "A" });
    assert!(!verify_signature(&payload, &bad, &key.verifying_key()));
    assert!(!verify_signature(
        &canonical_payload("demo.plugin", "9.9.9", &"ab".repeat(32)),
        &signature,
        &key.verifying_key()
    ));
    assert!(!verify_signature(
        &payload,
        "not-a-signature",
        &key.verifying_key()
    ));
    assert!(decode_signature(&signature).is_some());
    assert!(looks_like_signature(&signature));
}

#[test]
fn canonical_payload_accepts_legacy_digest_alias_and_manifest() {
    let value = json!({"id": "demo", "version": "1.0.0", "entry_digest": "cd".repeat(32)});
    assert_eq!(
        canonical_payload_value(&value),
        canonical_payload("demo", "1.0.0", &"cd".repeat(32))
    );
    let manifest = validate_manifest(&json!({
        "manifestVersion": 1,
        "id": "demo",
        "name": "Demo",
        "version": "1.0.0",
        "entry": "index.js",
        "entrySha256": "ef".repeat(32)
    }))
    .manifest
    .unwrap();
    assert_eq!(
        canonical_payload_for_manifest(&manifest),
        canonical_payload("demo", "1.0.0", &"ef".repeat(32))
    );
}

#[test]
fn trust_store_adds_lists_gets_and_removes_without_paths() {
    let data = TempDir::new("arcadia-plugin-trust");
    let key = keypair(11);
    let store = TrustStore::for_data_dir(data.path());
    let added = store.add("vendor-key", &key.verifying_key());
    assert_eq!(
        added,
        TrustOpResult {
            ok: true,
            key_id: Some("vendor-key".into()),
            error: String::new()
        }
    );
    let serialized = serde_json::to_value(&added).unwrap();
    assert_eq!(serialized, json!({"ok": true, "keyId": "vendor-key"}));
    assert!(serialized.get("path").is_none());

    let keys = store.list();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].key_id, "vendor-key");
    assert!(keys[0].public_key.starts_with("-----BEGIN PUBLIC KEY-----"));
    let serialized = serde_json::to_value(&keys[0]).unwrap();
    assert!(serialized.get("path").is_none());

    let reopened = TrustStore::for_data_dir(data.path());
    assert!(reopened.has("vendor-key"));
    assert_eq!(
        reopened.get("vendor-key"),
        Some(export_public_key(&key.verifying_key()))
    );
    assert_eq!(
        reopened.get_public_key("vendor-key"),
        Some(key.verifying_key())
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = data.join("plugins/trusted-keys.json");
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    assert_eq!(
        store.remove("vendor-key"),
        TrustOpResult {
            ok: true,
            key_id: Some("vendor-key".into()),
            error: String::new()
        }
    );
    assert!(store.list().is_empty());
    assert_eq!(store.remove("vendor-key").error, "chave_desconhecida");
}

#[test]
fn trust_store_accepts_pem_and_base64_der_and_rejects_bad_input() {
    let data = TempDir::new("arcadia-plugin-trust-pem");
    let key = keypair(13);
    let store = TrustStore::for_data_dir(data.path());
    let pem = export_public_key(&key.verifying_key());
    let der_base64 = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect::<String>();

    assert!(store.add_pem("pem-key", &pem).ok);
    assert!(store.add_pem("der-key", &der_base64).ok);
    assert_eq!(store.list().len(), 2);
    assert_eq!(store.get("pem-key"), store.get("der-key"));

    assert_eq!(store.add_pem("bad/key", &pem).error, "chave_invalida");
    assert_eq!(
        store.add_pem("good-key", "not-a-key").error,
        "chave_invalida"
    );
    assert!(!store.verify("missing", "{}", &sign(&key, "{}")));

    // Sobrescrever a mesma chave não duplica a entrada.
    assert!(store.add_pem("pem-key", &pem).ok);
    assert_eq!(store.list().len(), 2);

    let verify_payload = canonical_payload("demo", "1.0.0", &"12".repeat(32));
    let signature = sign(&key, &verify_payload);
    assert!(store.verify("pem-key", &verify_payload, &signature));
    assert!(!store.verify("pem-key", &verify_payload, "bad"));
}

#[test]
fn trust_store_verifies_manifest_signature() {
    let data = TempDir::new("arcadia-plugin-trust-manifest");
    let key = keypair(17);
    let store = TrustStore::for_data_dir(data.path());
    store.add("vendor-key", &key.verifying_key());

    let digest = "34".repeat(32);
    let signature = sign(&key, &canonical_payload("signed.demo", "1.0.0", &digest));
    let manifest = validate_manifest(&json!({
        "manifestVersion": 1,
        "id": "signed.demo",
        "name": "Signed",
        "version": "1.0.0",
        "entry": "index.js",
        "entrySha256": digest,
        "signingKeyId": "vendor-key",
        "signature": signature
    }));
    let manifest = manifest.manifest.unwrap();
    assert!(store.verify_manifest("vendor-key", &manifest, &digest));
    assert!(!store.verify_manifest("vendor-key", &manifest, &"00".repeat(32)));
    assert!(!store.verify_manifest("unknown", &manifest, &digest));
}

#[test]
fn parse_store_accepts_envelopes_aliases_and_preserves_unknown_fields() {
    let key = keypair(19);
    let pem = export_public_key(&key.verifying_key());

    let state = parse_store(&json!({"version": 1, "keys": {"k1": pem}}));
    assert_eq!(state.keys.len(), 1);
    assert_eq!(state.keys[0].key_id, "k1");
    assert_eq!(state.keys[0].public_key, pem);

    let state = parse_store(&json!({
        "version": "1",
        "trustedKeys": [{"keyId": "k2", "publicKey": pem}]
    }));
    assert_eq!(state.keys[0].key_id, "k2");

    let state = parse_store(&json!({
        "trusted_keys": {"k3": {"pem": pem}},
        "future": {"kept": true}
    }));
    assert_eq!(state.keys[0].key_id, "k3");
    assert_eq!(state.extra["future"], json!({"kept": true}));

    let state = parse_store(&json!({"version": 2, "keys": {"k1": pem}}));
    assert!(state.keys.is_empty());
    assert!(state.extra.is_empty());

    let state = parse_store(&json!({
        "version": 1,
        "keys": {"bad/id": pem, "ok": "not-a-key", "k4": pem}
    }));
    assert_eq!(state.keys.len(), 1);
    assert_eq!(state.keys[0].key_id, "k4");
}

#[test]
fn atomic_write_replaces_without_leftover_temp_and_uses_newline() {
    let data = TempDir::new("arcadia-plugin-trust-atomic");
    let file = data.join("plugins/trusted-keys.json");
    atomic_write_json(
        &file,
        &json!({"version": 1, "keys": {}}),
        "trust_store_diretorio_invalido",
    )
    .unwrap();
    atomic_write_json(
        &file,
        &json!({"version": 1, "keys": {"a": "b"}}),
        "trust_store_diretorio_invalido",
    )
    .unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.ends_with('\n'));
    assert!(text.contains("\"a\": \"b\""));
    let leftovers: Vec<_> = fs::read_dir(data.join("plugins"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn trust_store_preserves_unknown_fields_when_rewriting() {
    let data = TempDir::new("arcadia-plugin-trust-extra");
    let key = keypair(23);
    let store = TrustStore::for_data_dir(data.path());
    store.add("first", &key.verifying_key());
    let file = data.join("plugins/trusted-keys.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    value["future"] = json!({"kept": true});
    fs::write(&file, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    store.add("second", &key.verifying_key());
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(value["future"], json!({"kept": true}));
    assert!(value["keys"].get("first").is_some());
    assert!(value["keys"].get("second").is_some());
}
