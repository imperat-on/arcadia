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
}

impl SysinfoCache {
    pub fn new() -> Self {
        Self::default()
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
        cache.put("custom:x".into(), serde_json::json!({ "ok": true, "info": null }));
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
