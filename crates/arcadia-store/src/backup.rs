//! Backup automático antes da primeira escrita do cliente Rust no `dataDir`.
//!
//! O Electron e o Rust compartilham os mesmos arquivos. Antes que o Rust
//! regrave qualquer estado, copiamos o estado pequeno do usuário para
//! `backup-pre-rust-<timestamp>/` — config, sessão, biblioteca, overrides e os
//! arquivos por conta. Jogos, prefixos e caches grandes ficam de fora.
//!
//! A cópia nunca é apagada pelo app; a existência de qualquer diretório
//! `backup-pre-rust-*` impede um segundo backup.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const ROOT_FILES: &[&str] = &[
    "config.json",
    "session.json",
    "library.json",
    "overrides.json",
    "owned_games.json",
    "custom_games.json",
    "pending_games.json",
    "game_settings.json",
    "achievements.json",
    "profile_cache.json",
    "sync_queue.json",
    "friends_cache.json",
    "hltb_cache.json",
];

/// Mesma lista de `conta.js` (`ARQS_CONTA`).
const ACCOUNT_FILES: &[&str] = &[
    "achievements.json",
    "overrides.json",
    "custom_games.json",
    "pending_games.json",
    "game_settings.json",
    "profile_cache.json",
    "sync_queue.json",
    "friends_cache.json",
    "owned_games.json",
];

pub const BACKUP_PREFIX: &str = "backup-pre-rust-";

fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Cria o backup se ainda não existir. Devolve `Some(path)` quando cria.
pub fn ensure_pre_rust_backup(data_dir: &Path) -> io::Result<Option<PathBuf>> {
    if !data_dir.exists() {
        return Ok(None);
    }

    for entry in fs::read_dir(data_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with(BACKUP_PREFIX)
        {
            return Ok(None);
        }
    }

    let mut sources: Vec<(PathBuf, PathBuf)> = Vec::new();
    for name in ROOT_FILES {
        let source = data_dir.join(name);
        if source.is_file() {
            sources.push((source, PathBuf::from(name)));
        }
    }

    let accounts_dir = data_dir.join("contas");
    if accounts_dir.is_dir() {
        for account in fs::read_dir(&accounts_dir)? {
            let account = account?;
            if !account.file_type()?.is_dir() {
                continue;
            }
            let user = account.file_name();
            for name in ACCOUNT_FILES {
                let source = account.path().join(name);
                if source.is_file() {
                    sources.push((source, PathBuf::from("contas").join(&user).join(name)));
                }
            }
        }
    }

    if sources.is_empty() {
        return Ok(None);
    }

    let backup_dir = data_dir.join(format!("{BACKUP_PREFIX}{}", epoch_seconds()));
    fs::create_dir_all(&backup_dir)?;
    for (source, relative) in &sources {
        let target = backup_dir.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, &target)?;
    }
    fs::write(
        backup_dir.join(".done"),
        format!("arcadia-rust backup {}\n", epoch_seconds()),
    )?;

    Ok(Some(backup_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-backup-{tag}-{}-{}",
            std::process::id(),
            epoch_seconds()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_backup_once_with_root_and_account_files() {
        let dir = temp_dir("once");
        fs::write(dir.join("config.json"), r#"{"language":"pt-BR"}"#).unwrap();
        fs::write(dir.join("library.json"), r#"{"version":1,"games":[]}"#).unwrap();
        fs::create_dir_all(dir.join("contas").join("zyon")).unwrap();
        fs::write(
            dir.join("contas").join("zyon").join("overrides.json"),
            r#"{"1":{"title":"x"}}"#,
        )
        .unwrap();

        let backup = ensure_pre_rust_backup(&dir).unwrap().expect("backup criado");
        assert!(backup.join("config.json").is_file());
        assert!(backup.join("library.json").is_file());
        assert!(backup
            .join("contas")
            .join("zyon")
            .join("overrides.json")
            .is_file());

        assert!(ensure_pre_rust_backup(&dir).unwrap().is_none(), "segundo backup deve ser no-op");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_or_missing_data_dir_is_noop() {
        let missing = std::env::temp_dir().join("arcadia-backup-missing-does-not-exist");
        assert!(ensure_pre_rust_backup(&missing).unwrap().is_none());

        let empty = temp_dir("empty");
        assert!(ensure_pre_rust_backup(&empty).unwrap().is_none());
        let _ = fs::remove_dir_all(&empty);
    }

    #[test]
    fn backup_preserves_bytes_before_write() {
        let dir = temp_dir("bytes");
        fs::write(dir.join("config.json"), r#"{"language":"pt-BR"}"#).unwrap();
        let backup = ensure_pre_rust_backup(&dir).unwrap().expect("backup");
        fs::write(dir.join("config.json"), r#"{"language":"en-US"}"#).unwrap();
        assert_eq!(
            fs::read_to_string(backup.join("config.json")).unwrap(),
            r#"{"language":"pt-BR"}"#
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
