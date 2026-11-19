//! Posse de jogos por conta — espelho de `app/electron/owned.js`.
//!
//! O `library.json` é global; `owned_games.json` diz quais ids a conta ativa
//! registrou como dela. Guest sempre vê tudo (raiz = comportamento legado) e
//! nunca cria `owned_games.json` na raiz.
//!
//! Arquivo ausente ou corrompido = conta ainda não migrou para posse: a
//! primeira leitura materializa os ids atuais e devolve todos (senão a conta
//! perderia acesso ao que já tinha). `owned_set` devolve `None` nesse caso —
//! o chamador (push) deve tratar como posse desconhecida e não remover nada.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use serde_json::Value;

use crate::accounts::AccountScope;
use crate::atomic::write_atomic;

pub const OWNED_GAMES_NAME: &str = "owned_games.json";

fn owned_path(scope: &AccountScope) -> PathBuf {
    scope.account_file(OWNED_GAMES_NAME)
}

/// Lê o array bruto de `owned_games.json`. `None` para arquivo ausente,
/// inválido ou que não seja um array — exatamente o `null` do `readOwned`.
pub fn read_owned(scope: &AccountScope) -> Option<Vec<Value>> {
    let raw = std::fs::read_to_string(owned_path(scope)).ok()?;
    match serde_json::from_str::<Value>(&raw).ok()? {
        Value::Array(items) => Some(items),
        _ => None,
    }
}

/// Grava a posse inicial (`ids` dos jogos atuais). Best-effort no fluxo de
/// leitura: `filter_by_ownership` ignora a falha e continua devolvendo tudo.
///
/// Guest não tem arquivo de posse; a chamada é no-op.
pub fn materialize_owned(scope: &AccountScope, games: &[Value]) -> io::Result<()> {
    if scope.is_guest() {
        return Ok(());
    }
    let ids: Vec<Value> = games
        .iter()
        .map(|game| game.get("id").cloned().unwrap_or(Value::Null))
        .collect();
    write_owned(scope, &ids).map(|_| ())
}

/// Filtra a biblioteca global pelos ids que a conta possui. Guest vê tudo.
pub fn filter_by_ownership(scope: &AccountScope, globals: &[Value]) -> Vec<Value> {
    if scope.is_guest() {
        return globals.to_vec();
    }

    let Some(ids) = read_owned(scope) else {
        // Conveniência de migração, nunca requisito da leitura.
        let _ = materialize_owned(scope, globals);
        return globals.to_vec();
    };

    let owned: HashSet<&Value> = ids.iter().collect();
    globals
        .iter()
        .filter(|game| game.get("id").map(|id| owned.contains(id)).unwrap_or(false))
        .cloned()
        .collect()
}

/// Set dos ids possuídos. Guest devolve `Some(vazio)`; arquivo ausente ou
/// corrompido devolve `None` (posse desconhecida — não remova nada no push).
pub fn owned_set(scope: &AccountScope) -> Option<HashSet<Value>> {
    if scope.is_guest() {
        return Some(HashSet::new());
    }
    read_owned(scope).map(|ids| ids.into_iter().collect())
}

/// Registra o id como possuído. No-op para guest. Devolve `true` se gravou.
pub fn owned_add(scope: &AccountScope, id: impl Into<Value>) -> io::Result<bool> {
    if scope.is_guest() {
        return Ok(false);
    }
    let id = id.into();
    let mut ids = read_owned(scope).unwrap_or_default();
    if ids.contains(&id) {
        return Ok(false);
    }
    ids.push(id);
    write_owned(scope, &ids)?;
    Ok(true)
}

/// Remove o id da posse. No-op para guest ou id ausente.
pub fn owned_remove(scope: &AccountScope, id: impl Into<Value>) -> io::Result<bool> {
    if scope.is_guest() {
        return Ok(false);
    }
    let Some(ids) = read_owned(scope) else {
        return Ok(false);
    };
    let id = id.into();
    let next: Vec<Value> = ids
        .iter()
        .filter(|existing| **existing != id)
        .cloned()
        .collect();
    if next.len() == ids.len() {
        return Ok(false);
    }
    write_owned(scope, &next)?;
    Ok(true)
}

/// Grava o array em `owned_games.json` de forma atômica (tmp + rename). Uma
/// queda no meio não pode truncar o arquivo e fazer a conta perder jogos.
/// Guest não cria arquivo na raiz.
pub fn write_owned(scope: &AccountScope, ids: &[Value]) -> io::Result<PathBuf> {
    let path = owned_path(scope);
    if scope.is_guest() {
        return Ok(path);
    }
    let serialized = serde_json::to_vec(ids)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    write_atomic(&path, &serialized)?;
    Ok(path)
}

/// Caminho do `owned_games.json` da conta ativa (raiz para guest).
pub fn owned_games_path(scope: &AccountScope) -> PathBuf {
    owned_path(scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-owned-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn globals() -> Vec<Value> {
        vec![json!({"id":"a"}), json!({"id":"b"}), json!({"id":"c"})]
    }

    fn scope_for(dir: &Path, user: Option<&str>) -> AccountScope {
        AccountScope::with_active(dir, user)
    }

    #[test]
    fn guest_sees_everything_and_never_creates_owned_file() {
        let dir = temp_dir("guest");
        let scope = scope_for(&dir, None);

        assert_eq!(filter_by_ownership(&scope, &globals()), globals());
        assert_eq!(owned_set(&scope), Some(HashSet::new()));
        assert!(!owned_add(&scope, "a").unwrap());
        assert!(!owned_remove(&scope, "a").unwrap());
        assert!(!dir.join(OWNED_GAMES_NAME).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn account_with_owned_filters_by_id() {
        let dir = temp_dir("filter");
        let scope = scope_for(&dir, Some("u1"));
        write_owned(&scope, &[json!("a")]).unwrap();

        let filtered = filter_by_ownership(&scope, &globals());
        assert_eq!(filtered, vec![json!({"id":"a"})]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_owned_file_sees_all_and_materializes() {
        let dir = temp_dir("materialize");
        let scope = scope_for(&dir, Some("u2"));
        let path = owned_games_path(&scope);
        assert!(!path.exists());

        let filtered = filter_by_ownership(&scope, &globals());
        assert_eq!(filtered, globals());
        assert!(path.exists());
        assert_eq!(
            serde_json::from_str::<Value>(&std::fs::read_to_string(&path).unwrap()).unwrap(),
            json!(["a", "b", "c"])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_owned_file_materializes_and_returns_all() {
        let dir = temp_dir("corrupt");
        let scope = scope_for(&dir, Some("u3"));
        let path = owned_games_path(&scope);
        std::fs::write(&path, "{nao e array").unwrap();

        assert_eq!(read_owned(&scope), None);
        assert_eq!(filter_by_ownership(&scope, &globals()), globals());
        assert_eq!(
            serde_json::from_str::<Value>(&std::fs::read_to_string(&path).unwrap()).unwrap(),
            json!(["a", "b", "c"])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn owned_add_appends_without_duplicates() {
        let dir = temp_dir("add");
        let scope = scope_for(&dir, Some("u4"));
        let path = owned_games_path(&scope);

        assert!(owned_add(&scope, "steam:1").unwrap());
        assert!(!owned_add(&scope, "steam:1").unwrap());
        assert!(owned_add(&scope, "epic:2").unwrap());

        assert_eq!(
            serde_json::from_str::<Value>(&std::fs::read_to_string(&path).unwrap()).unwrap(),
            json!(["steam:1", "epic:2"])
        );
        assert_eq!(
            owned_set(&scope).unwrap(),
            HashSet::from([json!("steam:1"), json!("epic:2")])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn owned_remove_deletes_only_the_target() {
        let dir = temp_dir("remove");
        let scope = scope_for(&dir, Some("u6"));
        let path = owned_games_path(&scope);
        write_owned(&scope, &[json!("steam:1"), json!("epic:2")]).unwrap();

        assert!(owned_remove(&scope, "steam:1").unwrap());
        assert!(!owned_remove(&scope, "steam:1").unwrap());
        assert_eq!(
            serde_json::from_str::<Value>(&std::fs::read_to_string(&path).unwrap()).unwrap(),
            json!(["epic:2"])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn owned_set_distinguishes_missing_from_empty() {
        let dir = temp_dir("set");
        let scope = scope_for(&dir, Some("u7"));
        assert_eq!(owned_set(&scope), None);

        write_owned(&scope, &[]).unwrap();
        assert_eq!(owned_set(&scope), Some(HashSet::new()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_is_atomic_without_temp_leftovers() {
        let dir = temp_dir("atomic");
        let scope = scope_for(&dir, Some("u8"));
        write_owned(&scope, &[json!("a")]).unwrap();
        write_owned(&scope, &[json!("a"), json!("b")]).unwrap();

        let account_dir = dir.join("contas").join("u8");
        let leftovers: Vec<_> = std::fs::read_dir(&account_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "sobrou temporário: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
