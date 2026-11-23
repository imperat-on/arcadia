//! Cache em memória das consultas de catálogo por jogo.
//!
//! ## O problema que resolve
//!
//! Abrir a página de um jogo disparava três canais caros, todos indo à rede
//! toda vez, sem guardar nada:
//!
//! ```text
//! game:sysinfo    1,4–2,2 s   store.suggest + GET /catalog/v1/sysinfo
//! game:stats      1,48 s      GET /catalog/v1/stats + /catalog/v1/reviews
//! game:protondb   348 ms      GET de compatibilidade
//! ```
//!
//! Voltar ao mesmo jogo custava o mesmo de novo. O cache é chaveado com
//! prefixo (`sysinfo:`, `stats:`, `protondb:`) para os três dividirem um mapa
//! sem colidir no appid.
//!
//! ## O que NÃO entra
//!
//! `achievements:get` e `community:reviews`: são por conta. Cachear misturaria
//! o progresso de um usuário com o de outro.
//!
//! ## Por que em memória e não em disco
//!
//! Requisitos, stats e notas de compatibilidade não mudam entre sessões do app,
//! mas mudam quando o jogo recebe patch ou a comunidade reporta. Um TTL curto
//! cobre a sessão (é onde o usuário clica repetido) e expira sozinho.
//!
//! ## Negativos também são cacheados
//!
//! `ok: true, info: null` — jogo sem appid Steam, catálogo fora do ar — é
//! resposta legítima e igualmente cara. Sem cachear o negativo, todo clique num
//! jogo fora da Steam pagava a rede de novo.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Tempo de vida de uma entrada. Longo o bastante para cobrir uma sessão de
/// navegação, curto o bastante para refletir um patch no mesmo dia.
pub const SYSINFO_TTL: Duration = Duration::from_secs(600);

/// Limite de entradas. Cada uma é pequena (um objeto de requisitos), mas o app
/// fica aberto por horas: sem teto, uma biblioteca de 500 jogos guardaria 500
/// entradas para sempre.
const MAX_ENTRADAS: usize = 256;

#[derive(Default)]
pub struct SysinfoCache {
    entradas: Mutex<HashMap<String, (Instant, serde_json::Value)>>,
    em_voo: Mutex<HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
}

impl SysinfoCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot apenas de metadados públicos. Não inclui conquistas/sessão.
    pub async fn read_disk(root: std::path::PathBuf, key: String) -> Option<serde_json::Value> {
        tokio::task::spawn_blocking(move || {
            use sha2::{Digest, Sha256};
            let path = root.join(format!("{:x}.json", Sha256::digest(key.as_bytes())));
            let meta = std::fs::metadata(&path).ok()?;
            if meta.len() > 4 * 1024 * 1024
                || meta.modified().ok()?.elapsed().ok()? > Duration::from_secs(3600)
            {
                return None;
            }
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
            value.get("info").filter(|info| info.is_object())?;
            Some(value)
        })
        .await
        .ok()
        .flatten()
    }

    pub async fn write_disk(root: std::path::PathBuf, key: String, value: serde_json::Value) {
        if !value.get("info").is_some_and(|info| info.is_object()) {
            return;
        }
        let _ = tokio::task::spawn_blocking(move || {
            use sha2::{Digest, Sha256};
            std::fs::create_dir_all(&root).ok()?;
            let path = root.join(format!("{:x}.json", Sha256::digest(key.as_bytes())));
            static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let temporary = path.with_extension(format!("{}.{serial}.part", std::process::id()));
            let bytes = serde_json::to_vec(&value).ok()?;
            if bytes.len() > 4 * 1024 * 1024 {
                return None;
            }
            std::fs::write(&temporary, bytes).ok()?;
            std::fs::rename(temporary, path).ok()?;
            let mut files: Vec<_> = std::fs::read_dir(&root)
                .ok()?
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                .collect();
            if files.len() > MAX_ENTRADAS {
                files.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
                let excess = files.len() - MAX_ENTRADAS;
                for file in files.into_iter().take(excess) {
                    let _ = std::fs::remove_file(file.path());
                }
            }
            Some(())
        })
        .await;
    }

    /// Cliques/prefetch simultâneos do mesmo jogo dividem uma única consulta.
    /// O guard async é liberado inclusive se a tarefa for cancelada.
    pub async fn lock_request(&self, chave: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let mutex = {
            let mut guard = self.em_voo.lock().unwrap_or_else(|e| e.into_inner());
            guard.retain(|_, mutex| mutex.strong_count() > 0);
            match guard.get(chave).and_then(std::sync::Weak::upgrade) {
                Some(mutex) => mutex,
                None => {
                    let mutex = std::sync::Arc::new(tokio::sync::Mutex::new(()));
                    guard.insert(chave.to_string(), std::sync::Arc::downgrade(&mutex));
                    mutex
                }
            }
        };
        mutex.lock_owned().await
    }

    /// Devolve a entrada se ainda estiver dentro do TTL.
    pub fn get(&self, chave: &str) -> Option<serde_json::Value> {
        let guard = self.entradas.lock().unwrap_or_else(|e| e.into_inner());
        let (quando, valor) = guard.get(chave)?;
        if quando.elapsed() < SYSINFO_TTL {
            Some(valor.clone())
        } else {
            None
        }
    }

    /// Guarda uma resposta. Poda as mais antigas quando passa do limite.
    pub fn put(&self, chave: String, valor: serde_json::Value) {
        let mut guard = self.entradas.lock().unwrap_or_else(|e| e.into_inner());
        if guard.len() >= MAX_ENTRADAS {
            // Descarta as expiradas primeiro; se ainda estiver cheio, derruba a
            // metade mais antiga. Poda barata: acontece uma vez a cada 256.
            guard.retain(|_, (quando, _)| quando.elapsed() < SYSINFO_TTL);
            if guard.len() >= MAX_ENTRADAS {
                let mut ordenadas: Vec<(String, Instant)> = guard
                    .iter()
                    .map(|(k, (quando, _))| (k.clone(), *quando))
                    .collect();
                ordenadas.sort_by_key(|(_, quando)| *quando);
                for (chave, _) in ordenadas.into_iter().take(MAX_ENTRADAS / 2) {
                    guard.remove(&chave);
                }
            }
        }
        guard.insert(chave, (Instant::now(), valor));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_requests_only_fetch_once_and_other_keys_do_not_wait() {
        let cache = std::sync::Arc::new(SysinfoCache::new());
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let cache = cache.clone();
            let calls = calls.clone();
            tasks.push(tokio::spawn(async move {
                let _request = cache.lock_request("steam:730").await;
                if cache.get("steam:730").is_none() {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    cache.put(
                        "steam:730".into(),
                        serde_json::json!({"ok":true,"info":{"about":"public"}}),
                    );
                }
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let _one = cache.lock_request("steam:730").await;
        let _two =
            tokio::time::timeout(Duration::from_millis(100), cache.lock_request("steam:570"))
                .await
                .expect("chaves diferentes foram serializadas");
    }

    #[tokio::test]
    async fn public_metadata_survives_restart_but_negative_results_do_not() {
        let root =
            std::env::temp_dir().join(format!("arcadia-sysinfo-disk-{}", std::process::id()));
        let value =
            serde_json::json!({"ok":true,"info":{"about":"Actual description","movies":[]}});
        SysinfoCache::write_disk(root.clone(), "steam:730".into(), value.clone()).await;
        assert_eq!(
            SysinfoCache::read_disk(root.clone(), "steam:730".into()).await,
            Some(value)
        );
        assert!(SysinfoCache::read_disk(root.clone(), "steam:570".into())
            .await
            .is_none());
        SysinfoCache::write_disk(
            root.clone(),
            "custom:none".into(),
            serde_json::json!({"ok":true,"info":null}),
        )
        .await;
        assert!(SysinfoCache::read_disk(root.clone(), "custom:none".into())
            .await
            .is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn guarda_e_devolve() {
        let cache = SysinfoCache::new();
        cache.put("steam:730".into(), serde_json::json!({ "nome": "CS2" }));
        assert_eq!(
            cache.get("steam:730"),
            Some(serde_json::json!({ "nome": "CS2" }))
        );
    }

    #[test]
    fn chave_ausente_devolve_none() {
        let cache = SysinfoCache::new();
        assert_eq!(cache.get("nada"), None);
    }

    #[test]
    fn negativo_tambem_e_cacheado() {
        // `ok: true, info: null` é resposta legítima (jogo sem appid). Se não
        // fosse cacheada, cada clique pagaria duas idas à rede de novo.
        let cache = SysinfoCache::new();
        cache.put(
            "custom:x".into(),
            serde_json::json!({ "ok": true, "info": null }),
        );
        assert!(cache.get("custom:x").is_some());
    }

    #[test]
    fn poda_respeita_o_teto() {
        let cache = SysinfoCache::new();
        for i in 0..(MAX_ENTRADAS + 40) {
            cache.put(format!("steam:{i}"), serde_json::json!({ "n": i }));
        }
        let guard = cache.entradas.lock().unwrap();
        assert!(
            guard.len() <= MAX_ENTRADAS,
            "cache passou do teto: {}",
            guard.len()
        );
    }

    #[test]
    fn chave_diferente_nao_colide() {
        let cache = SysinfoCache::new();
        cache.put("steam:730".into(), serde_json::json!({ "a": 1 }));
        cache.put("steam:570".into(), serde_json::json!({ "b": 2 }));
        assert_eq!(cache.get("steam:730"), Some(serde_json::json!({ "a": 1 })));
        assert_eq!(cache.get("steam:570"), Some(serde_json::json!({ "b": 2 })));
    }
}
