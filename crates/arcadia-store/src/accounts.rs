//! Escopo por conta dos arquivos locais — espelho de
//! `app/electron/supabase/conta.js`.
//!
//! Logado como `<user>`: arquivos em `dataDir/contas/<user>/`.
//! Guest (sem conta): arquivos na raiz, como sempre foi.
//!
//! Migração: a PRIMEIRA conta que logar numa máquina herda os arquivos da
//! raiz; depois disso `contas/.migrado` existe e as contas seguintes começam
//! vazias. `library.json` não é herdado — é um snapshot global legado.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CONTAS_DIR: &str = "contas";
pub const MIGRATION_MARKER: &str = ".migrado";

/// Arquivos locais escopados por conta. `library.json` NÃO entra: é um
/// snapshot global legado na raiz. Lista idêntica a `ARQS_CONTA` do conta.js.
pub const ARQS_CONTA: [&str; 9] = [
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

pub fn contas_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(CONTAS_DIR)
}

pub fn migration_marker(data_dir: &Path) -> PathBuf {
    contas_dir(data_dir).join(MIGRATION_MARKER)
}

pub fn account_dir(data_dir: &Path, username: &str) -> PathBuf {
    contas_dir(data_dir).join(username)
}

/// Caminho escopado: raiz quando guest, senão `contas/<user>/<basename>`.
/// Assim como `caminhoConta` do JS, cria a pasta da conta na primeira chamada.
pub fn account_file(data_dir: &Path, username: Option<&str>, name: &str) -> PathBuf {
    match normalize_username(username) {
        Some(username) => {
            let dir = account_dir(data_dir, username);
            let _ = fs::create_dir_all(&dir);
            dir.join(basename(name))
        }
        None => data_dir.join(name),
    }
}

/// Migra os dados da raiz para a conta. Só a PRIMEIRA conta da máquina herda;
/// depois do marcador, contas novas começam do zero.
///
/// Se a pasta da conta já existe, nada é feito — inclusive o marcador não é
/// criado. É exatamente o curto-circuito do `migrarConta`.
pub fn migrate_account(data_dir: &Path, username: &str) -> io::Result<()> {
    let username = match normalize_username(Some(username)) {
        Some(username) => username,
        None => return Ok(()),
    };
    let dir = account_dir(data_dir, username);
    if dir.exists() {
        return Ok(());
    }

    fs::create_dir_all(&dir)?;

    let marker = migration_marker(data_dir);
    if marker.exists() {
        return Ok(());
    }

    for name in ARQS_CONTA {
        let source = data_dir.join(name);
        if source.exists() {
            // Best-effort como o try/catch por arquivo do conta.js: um arquivo
            // ilegível não impede a migração dos demais.
            let _ = fs::copy(&source, dir.join(name));
        }
    }

    // Falha ao gravar o marcador é ignorada no JS (o próximo login tenta de
    // novo); o importante é que os dados já foram copiados.
    let _ = fs::write(&marker, iso_utc_now().into_bytes());
    Ok(())
}

/// Estado da conta ativa. `None` = guest (raiz).
#[derive(Debug, Clone)]
pub struct AccountScope {
    data_dir: PathBuf,
    active: Option<String>,
}

impl AccountScope {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            active: None,
        }
    }

    /// Cria o escopo já com a conta informada, rodando a migração.
    pub fn with_active(data_dir: impl Into<PathBuf>, username: Option<&str>) -> Self {
        let mut scope = Self::new(data_dir);
        // A migração é best-effort na construção; `set_active` expõe o erro.
        let _ = scope.set_active(username);
        scope
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn contas_dir(&self) -> PathBuf {
        contas_dir(&self.data_dir)
    }

    pub fn marker_path(&self) -> PathBuf {
        migration_marker(&self.data_dir)
    }

    /// Username da conta ativa; `None` quando deslogado (guest/raiz).
    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    pub fn is_guest(&self) -> bool {
        self.active.is_none()
    }

    /// Troca a conta ativa (`None` = deslogado/guest) e roda a migração.
    pub fn set_active(&mut self, username: Option<&str>) -> io::Result<()> {
        // `contaAtiva = username || null`: string vazia vira guest; o JS não
        // faz trim do username, então preservamos o valor como veio.
        self.active = normalize_username(username).map(str::to_string);
        if let Some(username) = self.active.clone() {
            migrate_account(&self.data_dir, &username)?;
        }
        Ok(())
    }

    pub fn clear_active(&mut self) {
        self.active = None;
    }

    pub fn account_dir(&self) -> PathBuf {
        match self.active.as_deref() {
            Some(username) => account_dir(&self.data_dir, username),
            None => self.data_dir.clone(),
        }
    }

    /// Caminho escopado de um arquivo. Guest devolve a raiz; conta devolve
    /// `contas/<user>/<basename>` e garante que a pasta exista.
    pub fn account_file(&self, name: &str) -> PathBuf {
        account_file(&self.data_dir, self.active.as_deref(), name)
    }

    pub fn migrate(&self) -> io::Result<()> {
        match self.active.as_deref() {
            Some(username) => migrate_account(&self.data_dir, username),
            None => Ok(()),
        }
    }
}

fn normalize_username(username: Option<&str>) -> Option<&str> {
    username.filter(|username| !username.is_empty())
}

fn basename(name: &str) -> &str {
    Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(name)
}

/// `new Date().toISOString()` sem dependência de data/hora.
fn iso_utc_now() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = duration.as_secs();
    let millis = duration.subsec_millis();
    let days = (seconds / 86_400) as i64;
    let second_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Dias desde 1970-01-01 para (ano, mês, dia) — algoritmo civil_from_days.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-accounts-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn first_account_inherits_root_files_and_writes_marker() {
        let dir = temp_dir("inherit");
        fs::write(dir.join("achievements.json"), r#"{"a":1}"#).unwrap();
        fs::write(dir.join("owned_games.json"), r#"["steam:1"]"#).unwrap();
        // library.json é global legado e não deve ser herdado.
        fs::write(dir.join("library.json"), r#"{"version":1,"games":[]}"#).unwrap();

        let mut scope = AccountScope::new(&dir);
        scope.set_active(Some("u1")).unwrap();

        assert_eq!(scope.active(), Some("u1"));
        assert_eq!(
            fs::read_to_string(dir.join("contas").join("u1").join("achievements.json")).unwrap(),
            r#"{"a":1}"#
        );
        assert_eq!(
            fs::read_to_string(dir.join("contas").join("u1").join("owned_games.json")).unwrap(),
            r#"["steam:1"]"#
        );
        assert!(!dir.join("contas").join("u1").join("library.json").exists());
        assert!(migration_marker(&dir).exists());
        let marker = fs::read_to_string(migration_marker(&dir)).unwrap();
        assert!(
            marker.ends_with('Z') && marker.contains('T'),
            "marcador: {marker}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_account_starts_empty() {
        let dir = temp_dir("second");
        fs::write(dir.join("achievements.json"), "da-maquina").unwrap();

        let mut scope = AccountScope::new(&dir);
        scope.set_active(Some("u1")).unwrap();
        scope.set_active(Some("u2")).unwrap();

        assert_eq!(scope.active(), Some("u2"));
        assert!(dir.join("contas").join("u2").is_dir());
        assert!(!dir
            .join("contas")
            .join("u2")
            .join("achievements.json")
            .exists());
        assert_eq!(
            fs::read_to_string(dir.join("contas").join("u1").join("achievements.json")).unwrap(),
            "da-maquina"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn guest_falls_back_to_root_and_never_creates_account_dir() {
        let dir = temp_dir("guest");
        let mut scope = AccountScope::new(&dir);
        scope.set_active(Some("u1")).unwrap();
        scope.set_active(None).unwrap();

        assert!(scope.is_guest());
        assert_eq!(
            scope.account_file("owned_games.json"),
            dir.join("owned_games.json")
        );
        assert!(!dir.join("owned_games.json").exists());

        // String vazia equivale a guest, como `username || null`.
        scope.set_active(Some("")).unwrap();
        assert!(scope.is_guest());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn account_file_scopes_basename_and_creates_directory() {
        let dir = temp_dir("path");
        let scope = AccountScope::with_active(&dir, Some("zyon"));

        let path = scope.account_file("owned_games.json");
        assert_eq!(
            path,
            dir.join("contas").join("zyon").join("owned_games.json")
        );
        assert!(dir.join("contas").join("zyon").is_dir());

        // `caminhoConta` usa path.basename(base).
        let nested = scope.account_file("sub/dir/custom_games.json");
        assert_eq!(
            nested,
            dir.join("contas").join("zyon").join("custom_games.json")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_account_directory_skips_migration() {
        let dir = temp_dir("existing");
        fs::write(dir.join("achievements.json"), "raiz").unwrap();
        let account = dir.join("contas").join("u1");
        fs::create_dir_all(&account).unwrap();

        migrate_account(&dir, "u1").unwrap();

        assert!(!account.join("achievements.json").exists());
        assert!(!migration_marker(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn marker_blocks_inheritance_even_before_any_account_dir() {
        let dir = temp_dir("marker");
        fs::write(dir.join("achievements.json"), "raiz").unwrap();
        fs::create_dir_all(contas_dir(&dir)).unwrap();
        fs::write(migration_marker(&dir), "2026-01-01T00:00:00.000Z").unwrap();

        migrate_account(&dir, "u1").unwrap();

        assert!(dir.join("contas").join("u1").is_dir());
        assert!(!dir
            .join("contas")
            .join("u1")
            .join("achievements.json")
            .exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn arqs_conta_matches_electron_list() {
        assert_eq!(
            ARQS_CONTA,
            [
                "achievements.json",
                "overrides.json",
                "custom_games.json",
                "pending_games.json",
                "game_settings.json",
                "profile_cache.json",
                "sync_queue.json",
                "friends_cache.json",
                "owned_games.json",
            ]
        );
    }

    #[test]
    fn iso_timestamp_has_expected_shape() {
        let value = iso_utc_now();
        assert_eq!(value.len(), 24);
        assert_eq!(&value[4..5], "-");
        assert_eq!(&value[10..11], "T");
        assert!(value.ends_with('Z'));
    }
}
