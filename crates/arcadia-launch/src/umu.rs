//! UMU runtime discovery/validation.
//!
//! Port of `app/electron/umu-runtime.js`.  The official archive format and
//! digest checks are preserved.  Network I/O and writing the managed copy are
//! deliberately injected (`install`) so this crate stays testable and never
//! downloads during tests.

use std::collections::HashSet;

use crate::platform::Platform;

pub const UMU_VERSION: &str = "1.4.4";
pub const UMU_ARCHIVE_URL: &str = "https://github.com/Open-Wine-Components/umu-launcher/releases/download/1.4.4/umu-launcher-1.4.4-zipapp.tar";
pub const UMU_ARCHIVE_SHA256: &str =
    "eb590691841f7fad3fc3ad8fd5db4ccb87849fe7948e62b28ece7a4ee48cc851";
pub const UMU_EXECUTABLE_SHA256: &str =
    "d0005a58602041229cc467dab03dc0c0b9e8cce09a8145b16b7683244cf17804";
pub const MAX_ARCHIVE_BYTES: usize = 2 * 1024 * 1024;

pub trait UmuFs {
    fn is_executable(&self, path: &str) -> bool;
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemUmuFs;

impl UmuFs for SystemUmuFs {
    fn is_executable(&self, path: &str) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(path)
                .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            false
        }
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UmuSpec {
    pub version: String,
    pub archive_url: String,
    pub archive_sha256: String,
    pub executable_sha256: String,
    pub max_archive_bytes: usize,
}

impl Default for UmuSpec {
    fn default() -> Self {
        Self {
            version: UMU_VERSION.to_string(),
            archive_url: UMU_ARCHIVE_URL.to_string(),
            archive_sha256: UMU_ARCHIVE_SHA256.to_string(),
            executable_sha256: UMU_EXECUTABLE_SHA256.to_string(),
            max_archive_bytes: MAX_ARCHIVE_BYTES,
        }
    }
}

impl UmuSpec {
    pub fn official() -> Self {
        Self::default()
    }
}

pub fn managed_umu_path(data_dir: &str) -> String {
    Platform::Linux.join(&[data_dir, "bin", "umu", UMU_VERSION, "umu-run"])
}

pub fn is_managed_umu_valid(fs: &dyn UmuFs, file: &str) -> bool {
    is_managed_umu_valid_with(fs, file, UMU_EXECUTABLE_SHA256)
}

pub fn is_managed_umu_valid_with(fs: &dyn UmuFs, file: &str, expected_sha256: &str) -> bool {
    if !fs.is_executable(file) {
        return false;
    }
    match fs.read(file) {
        Some(bytes) => sha256_hex(&bytes) == expected_sha256,
        None => false,
    }
}

pub fn external_candidates(home: &str, env_path: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    for dir in env_path.split(':') {
        if !dir.is_empty() {
            candidates.push(Platform::Linux.join(&[dir, "umu-run"]));
        }
    }
    candidates.extend([
        Platform::Linux.join(&[
            home, ".local", "share", "lutris", "runtime", "umu", "umu-run",
        ]),
        Platform::Linux.join(&[
            home, ".config", "heroic", "tools", "runtimes", "umu", "umu-run",
        ]),
        Platform::Linux.join(&[
            home,
            ".config",
            "heroic",
            "tools",
            "runtimes",
            "umu",
            "umu_run.py",
        ]),
        "/usr/local/share/umu/umu-run".to_string(),
        "/usr/share/umu/umu-run".to_string(),
        "/opt/umu/umu-run".to_string(),
    ]);
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| seen.insert(candidate.clone()))
        .collect()
}

pub fn find_umu_launcher(
    platform: Platform,
    data_dir: &str,
    home: &str,
    env_path: &str,
    fs: &dyn UmuFs,
) -> Option<String> {
    if platform == Platform::Windows {
        return None;
    }
    let managed = managed_umu_path(data_dir);
    if is_managed_umu_valid(fs, &managed) {
        return Some(managed);
    }
    external_candidates(home, env_path)
        .into_iter()
        .find(|candidate| fs.is_executable(candidate))
}

pub fn parse_tar_size(block: &[u8]) -> usize {
    let raw: String = block
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .map(char::from)
        .collect();
    let raw = raw.trim();
    if raw.is_empty() || !raw.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return 0;
    }
    usize::from_str_radix(raw, 8).unwrap_or(0)
}

pub fn extract_umu_from_tar(archive: &[u8]) -> Result<Vec<u8>, String> {
    let mut offset = 0usize;
    while offset + 512 <= archive.len() {
        let header = &archive[offset..offset + 512];
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let name_bytes = &header[..100];
        let name_end = name_bytes.iter().position(|byte| *byte == 0).unwrap_or(100);
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();
        let size = parse_tar_size(&header[124..136]);
        let kind = header[156];
        let start = offset + 512;
        let end = start.checked_add(size).ok_or("pacote UMU truncado")?;
        if end > archive.len() {
            return Err("pacote UMU truncado".to_string());
        }
        if name == "umu/umu-run" && (kind == 0 || kind == 48) {
            return Ok(archive[start..end].to_vec());
        }
        offset = start + size.div_ceil(512) * 512;
    }
    Err("umu-run não encontrado no pacote oficial".to_string())
}

/// Run every official integrity check and return the executable bytes.
pub fn verify_umu_archive(archive: &[u8]) -> Result<Vec<u8>, String> {
    verify_umu_archive_with(archive, &UmuSpec::official())
}

pub fn verify_umu_archive_with(archive: &[u8], spec: &UmuSpec) -> Result<Vec<u8>, String> {
    if archive.len() > spec.max_archive_bytes {
        return Err("pacote UMU excedeu o tamanho esperado".to_string());
    }
    if sha256_hex(archive) != spec.archive_sha256 {
        return Err("SHA-256 do pacote UMU inválido".to_string());
    }
    let executable = extract_umu_from_tar(archive)?;
    if sha256_hex(&executable) != spec.executable_sha256 {
        return Err("SHA-256 do executável UMU inválido".to_string());
    }
    Ok(executable)
}

/// Result shape kept close to the Electron object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnsureUmu {
    pub ok: bool,
    pub path: String,
    pub managed: bool,
    pub warning: Option<String>,
    pub error: Option<String>,
}

/// Prepare the managed runtime.  `install` is the download/write step; when it
/// fails, a valid external installation is still accepted with a warning.
pub fn ensure_umu_launcher(
    platform: Platform,
    data_dir: &str,
    home: &str,
    env_path: &str,
    fs: &dyn UmuFs,
    install: &dyn Fn() -> Result<String, String>,
) -> EnsureUmu {
    if platform == Platform::Windows {
        return EnsureUmu {
            ok: false,
            path: String::new(),
            managed: false,
            warning: None,
            error: Some("UMU is not available on Windows".to_string()),
        };
    }
    let managed = managed_umu_path(data_dir);
    if is_managed_umu_valid(fs, &managed) {
        return EnsureUmu {
            ok: true,
            path: managed,
            managed: true,
            warning: None,
            error: None,
        };
    }
    match install() {
        Ok(path) => EnsureUmu {
            ok: true,
            path,
            managed: true,
            warning: None,
            error: None,
        },
        Err(error) => match find_umu_launcher(platform, data_dir, home, env_path, fs) {
            Some(fallback) => EnsureUmu {
                ok: true,
                path: fallback,
                managed: false,
                warning: Some(error),
                error: None,
            },
            None => EnsureUmu {
                ok: false,
                path: String::new(),
                managed: false,
                warning: None,
                error: Some(format!("Não foi possível preparar o UMU: {error}")),
            },
        },
    }
}

pub fn umu_run_command(umu_path: &str, executable: &str) -> Vec<String> {
    vec![umu_path.to_string(), executable.to_string()]
}

// ---------------------------------------------------------------------------
// SHA-256 (kept local to avoid a dependency for one digest check)
// ---------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256_hex(data: &[u8]) -> String {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks(64) {
        let mut w = [0u32; 64];
        for (index, word) in w.iter_mut().take(16).enumerate() {
            let base = index * 4;
            *word = u32::from_be_bytes([
                chunk[base],
                chunk[base + 1],
                chunk[base + 2],
                chunk[base + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];

        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    state.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeFs {
        executables: HashSet<String>,
        files: HashMap<String, Vec<u8>>,
    }

    impl UmuFs for FakeFs {
        fn is_executable(&self, path: &str) -> bool {
            self.executables.contains(path)
        }

        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.files.get(path).cloned()
        }
    }

    fn tar_with_umu(executable: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..11].copy_from_slice(b"umu/umu-run");
        let size = format!("{:011o}\0", executable.len());
        header[124..136].copy_from_slice(size.as_bytes());
        header[156] = b'0';
        let mut archive = Vec::new();
        archive.extend_from_slice(&header);
        archive.extend_from_slice(executable);
        let padding = (512 - (executable.len() % 512)) % 512;
        archive.extend(std::iter::repeat_n(0, padding));
        archive.extend(std::iter::repeat_n(0, 1024));
        archive
    }

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn extracts_umu_from_tar() {
        let archive = tar_with_umu(b"#!/bin/sh\n");
        assert_eq!(extract_umu_from_tar(&archive).unwrap(), b"#!/bin/sh\n");
        assert_eq!(
            extract_umu_from_tar(&tar_with_umu(b"x")[..512]).unwrap_err(),
            "pacote UMU truncado"
        );
        assert_eq!(
            extract_umu_from_tar(&[0u8; 1024]).unwrap_err(),
            "umu-run não encontrado no pacote oficial"
        );
    }

    #[test]
    fn archive_validation_uses_both_digests() {
        let executable = b"#!/bin/sh\n";
        let archive = tar_with_umu(executable);
        let spec = UmuSpec {
            archive_sha256: sha256_hex(&archive),
            executable_sha256: sha256_hex(executable),
            ..UmuSpec::official()
        };
        assert_eq!(
            verify_umu_archive_with(&archive, &spec).unwrap(),
            executable
        );
        let bad_archive = UmuSpec {
            archive_sha256: "00".repeat(32),
            ..spec.clone()
        };
        assert_eq!(
            verify_umu_archive_with(&archive, &bad_archive).unwrap_err(),
            "SHA-256 do pacote UMU inválido"
        );
        let bad_executable = UmuSpec {
            executable_sha256: "00".repeat(32),
            ..spec
        };
        assert_eq!(
            verify_umu_archive_with(&archive, &bad_executable).unwrap_err(),
            "SHA-256 do executável UMU inválido"
        );
    }

    #[test]
    fn managed_copy_is_validated_then_external_fallback_is_used() {
        let data_dir = "/data/arcadia";
        let managed = managed_umu_path(data_dir);
        let external = "/opt/umu/umu-run";
        let fs = FakeFs {
            executables: HashSet::from([managed.clone(), external.to_string()]),
            files: HashMap::from([(managed.clone(), b"adulterado".to_vec())]),
        };
        assert_eq!(
            find_umu_launcher(Platform::Linux, data_dir, "/nonexistent", "/usr/bin", &fs),
            Some(external.to_string())
        );
        assert_eq!(
            find_umu_launcher(Platform::Windows, data_dir, "/nonexistent", "", &fs),
            None
        );
    }

    #[test]
    fn ensure_reports_windows_immediately() {
        let fs = FakeFs::default();
        let result = ensure_umu_launcher(Platform::Windows, "/data", "/home", "", &fs, &|| {
            panic!("must not install on Windows")
        });
        assert!(!result.ok);
        assert_eq!(
            result.error.as_deref(),
            Some("UMU is not available on Windows")
        );
    }

    #[test]
    fn ensure_accepts_external_fallback_with_warning() {
        let external_dir = "/home/z/bin";
        let external = "/home/z/bin/umu-run";
        let fs = FakeFs {
            executables: HashSet::from([external.to_string()]),
            files: HashMap::new(),
        };
        let result = ensure_umu_launcher(
            Platform::Linux,
            "/data",
            "/home/z",
            external_dir,
            &fs,
            &|| Err("download falhou".to_string()),
        );
        assert!(result.ok);
        assert!(!result.managed);
        assert_eq!(result.path, external);
        assert_eq!(result.warning.as_deref(), Some("download falhou"));
    }

    #[test]
    fn ensure_reports_terminal_error_without_fallback() {
        let fs = FakeFs::default();
        let result = ensure_umu_launcher(Platform::Linux, "/data", "/home/z", "", &fs, &|| {
            Err("SHA-256 do pacote UMU inválido".to_string())
        });
        assert!(!result.ok);
        assert_eq!(
            result.error.as_deref(),
            Some("Não foi possível preparar o UMU: SHA-256 do pacote UMU inválido")
        );
    }

    #[test]
    fn umu_command_is_argv_not_shell() {
        assert_eq!(
            umu_run_command("/data/umu-run", "/games/demo.exe"),
            vec!["/data/umu-run", "/games/demo.exe"]
        );
    }
}
