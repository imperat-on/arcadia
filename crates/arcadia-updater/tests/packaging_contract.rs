//! Verifica os recursos e o tarball da distribuição nativa.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("raiz do repositório")
}

const TORRENT_WORKER_FILES: [&str; 2] = [
    "resources/torrent_rpc/main.py",
    "resources/torrent_rpc/torrent_downloader.py",
];

#[test]
fn native_package_resources_are_available_in_the_repository() {
    let root = repo_root();
    for resource in TORRENT_WORKER_FILES
        .into_iter()
        .chain(["boot.mp4", "crates/arcadia-native/assets/icons/icon.png"])
    {
        let path = root.join(resource);
        assert!(path.is_file(), "recurso ausente: {}", path.display());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_tarball_contains_the_app_and_resources_and_matches_its_checksum() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDirectory(PathBuf);
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    let scratch = TestDirectory(std::env::temp_dir().join(format!(
        "arcadia-native-package-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    let release = scratch.0.join("target/release");
    fs::create_dir_all(&release).unwrap();
    for name in ["arcadia-native", "arcadia-gamepad"] {
        let path = release.join(name);
        fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let output = scratch.0.join("output");
    let package = Command::new("bash")
        .arg(repo_root().join("scripts/package-native-linux.sh"))
        .args(["--skip-build", "--output"])
        .arg(&output)
        .env("CARGO_TARGET_DIR", scratch.0.join("target"))
        .env_remove("ARCADIA_NATIVE_BIN")
        .env_remove("ARCADIA_NATIVE_ICON")
        .env_remove("ARCADIA_NATIVE_VERSION")
        .env_remove("ARCADIA_GAMEPAD_BIN")
        .output()
        .expect("executar empacotamento Linux");
    assert!(
        package.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&package.stdout),
        String::from_utf8_lossy(&package.stderr)
    );

    let name = format!(
        "arcadia-native-{}-linux-{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH
    );
    let entries = Command::new("tar")
        .arg("-tzf")
        .arg(output.join(format!("{name}.tar.gz")))
        .output()
        .expect("listar conteúdo do tarball");
    assert!(
        entries.status.success(),
        "{}",
        String::from_utf8_lossy(&entries.stderr)
    );
    let entries = std::str::from_utf8(&entries.stdout).unwrap();
    for entry in TORRENT_WORKER_FILES.into_iter().chain([
        "arcadia-native",
        "arcadia-gamepad",
        "boot.mp4",
        "arcadia-native.desktop",
        "arcadia-native.png",
        "README.md",
    ]) {
        let path = format!("{name}/{entry}");
        assert!(
            entries.lines().any(|line| line == path),
            "tarball sem {path}"
        );
    }

    let checksum = Command::new("sha256sum")
        .args(["-c", "SHA256SUMS"])
        .current_dir(output)
        .output()
        .expect("verificar SHA256SUMS");
    assert!(
        checksum.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&checksum.stdout),
        String::from_utf8_lossy(&checksum.stderr)
    );
}
