//! Resolvedor DNS do backend — espelho de `resolveBackendIpv4` de
//! `app/electron/supabase/client.js`.
//!
//! ## Por que existe
//!
//! O resolvedor do sistema é normalmente mais rápido que DoH. Mas há duas
//! redes em que ele falha de um jeito que trava o app:
//!
//! 1. **Resolvedor local morto** (o DNS do roteador, tipicamente). Cada
//!    conexão nova paga o timeout inteiro — no Electron isso somava 3,4 s por
//!    requisição.
//! 2. **Tailscale Funnel** (`.ts.net`): tem A e AAAA públicos, mas alguns
//!    resolvedores locais entregam só AAAA. Esperar o IPv6 falhar acrescenta
//!    30 s ao boot e a tela fica preta no splash.
//!
//! A resposta é a mesma do Electron: pergunta ao sistema primeiro com um
//! limite curto (500 ms) e, se ele não entregar IPv4, pergunta ao DoH da
//! Cloudflare em JSON. O resultado vai para um cache de 5 minutos.
//!
//! ## Diferenças assumidas
//!
//! O JS guarda uma lista de IPs por host e tenta cada um em sequência porque
//! monta a conexão na mão com `https.request`. Aqui quem conecta é o
//! reqwest/hyper, e a trait [`Resolve`] devolve *todos* os endereços de uma
//! vez — o hyper faz a própria rotação. Então "o IP que respondeu por último"
//! vira uma **ordenação da lista**: o preferido vai primeiro e o resto
//! continua como reserva. Mesma intenção, cobertura melhor.
//!
//! ## Silêncio por design
//!
//! Nenhuma falha aqui derruba requisição: se o DoH não responder, devolve o
//! que o sistema deu. Se os dois falharem, devolve erro de resolução e o
//! fallback de endereço do [`HttpClient`](crate::http::HttpClient) assume.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// Limite do resolvedor do sistema. Igual ao `SYSTEM_DNS_TIMEOUT_MS`.
pub const SYSTEM_DNS_TIMEOUT: Duration = Duration::from_millis(500);
/// TTL do cache de IPs. Igual ao `5 * 60 * 1000`.
pub const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
/// Limite da consulta DoH. Igual ao `AbortSignal.timeout(8000)`.
pub const DOH_TIMEOUT: Duration = Duration::from_secs(8);

const DOH_ENDPOINT: &str = "https://cloudflare-dns.com/dns-query";

/// Entrada do cache de um host.
#[derive(Debug, Clone)]
struct Entrada {
    enderecos: Vec<Ipv4Addr>,
    expira: Instant,
}

/// Estado compartilhado. Fica atrás de `Arc` para o futuro `'static` que a
/// trait [`Resolve`] exige.
struct Estado {
    cache: Mutex<HashMap<String, Entrada>>,
    /// Host → IP que respondeu por último. Ordena a lista devolvida.
    preferido: Mutex<HashMap<String, Ipv4Addr>>,
    /// Cliente separado para o DoH: precisa resolver `cloudflare-dns.com`
    /// antes de existir resposta, então usa o resolvedor padrão.
    doh: reqwest::Client,
}

/// Resolvedor com cache, DoH e preferência por endereço.
#[derive(Clone)]
pub struct BackendResolver {
    estado: Arc<Estado>,
}

impl BackendResolver {
    pub fn new() -> Self {
        // `json` não é feature habilitada; o corpo é lido como bytes e
        // parseado com serde_json.
        let doh = reqwest::Client::builder()
            .timeout(DOH_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            estado: Arc::new(Estado {
                cache: Mutex::new(HashMap::new()),
                preferido: Mutex::new(HashMap::new()),
                doh,
            }),
        }
    }

    /// Resolve com cache. Ordem: cache → sistema → DoH.
    ///
    /// Público para teste e para chamadas que precisam do IP antes de abrir
    /// socket (o `warm` do boot, por exemplo).
    pub async fn resolver(&self, host: &str, porta: u16) -> Result<Vec<Ipv4Addr>, String> {
        let chave = host.to_ascii_lowercase();

        // 1. Cache válido responde sem tocar na rede.
        if let Ok(cache) = self.estado.cache.lock() {
            if let Some(entrada) = cache.get(&chave) {
                if entrada.expira > Instant::now() && !entrada.enderecos.is_empty() {
                    let enderecos = entrada.enderecos.clone();
                    drop(cache);
                    return Ok(self.ordenar(&chave, enderecos));
                }
            }
        }

        // 2. Sistema com limite curto.
        let mut enderecos = Self::consultar_sistema(&chave, porta).await;

        // 3. DoH cobre tanto timeout quanto resolvedor morto.
        if enderecos.is_none() {
            enderecos = self.consultar_doh(&chave).await;
        }

        let enderecos = enderecos.ok_or_else(|| format!("DNS sem IPv4 para {host}"))?;

        if let Ok(mut cache) = self.estado.cache.lock() {
            cache.insert(
                chave.clone(),
                Entrada {
                    enderecos: enderecos.clone(),
                    expira: Instant::now() + CACHE_TTL,
                },
            );
        }

        Ok(self.ordenar(&chave, enderecos))
    }

    /// Pergunta ao resolvedor do sistema com limite curto.
    ///
    /// `tokio::net::lookup_host` não aceita timeout, então a corrida é feita
    /// com `tokio::time::timeout`.
    async fn consultar_sistema(host: &str, porta: u16) -> Option<Vec<Ipv4Addr>> {
        let alvo = format!("{host}:{porta}");
        match tokio::time::timeout(SYSTEM_DNS_TIMEOUT, tokio::net::lookup_host(alvo)).await {
            Ok(Ok(enderecos)) => {
                let mut vistos: Vec<Ipv4Addr> = Vec::new();
                for endereco in enderecos {
                    if let IpAddr::V4(ip) = endereco.ip() {
                        if !vistos.contains(&ip) {
                            vistos.push(ip);
                        }
                    }
                }
                (!vistos.is_empty()).then_some(vistos)
            }
            // Timeout ou erro: o DoH cobre os dois.
            _ => None,
        }
    }

    /// Consulta o DoH da Cloudflare em JSON (`application/dns-json`).
    async fn consultar_doh(&self, host: &str) -> Option<Vec<Ipv4Addr>> {
        let resposta = self
            .estado
            .doh
            .get(DOH_ENDPOINT)
            .query(&[("name", host), ("type", "A")])
            .header("Accept", "application/dns-json")
            .header("User-Agent", "arcadia")
            .send()
            .await
            .ok()?;
        if !resposta.status().is_success() {
            return None;
        }
        let bytes = resposta.bytes().await.ok()?;
        let corpo: serde_json::Value = serde_json::from_slice(&bytes).ok()?;

        // Só registros A (`type == 1`) e só IPv4 válido — filtro do JS.
        let mut vistos: Vec<Ipv4Addr> = Vec::new();
        for item in corpo.get("Answer")?.as_array()? {
            if item.get("type").and_then(serde_json::Value::as_u64) != Some(1) {
                continue;
            }
            let Some(dado) = item.get("data").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Ok(ip) = dado.parse::<Ipv4Addr>() else {
                continue;
            };
            if !vistos.contains(&ip) {
                vistos.push(ip);
            }
        }
        (!vistos.is_empty()).then_some(vistos)
    }

    /// Põe o IP preferido na frente, mantendo o resto como reserva.
    fn ordenar(&self, host: &str, enderecos: Vec<Ipv4Addr>) -> Vec<Ipv4Addr> {
        let Ok(preferido) = self.estado.preferido.lock() else {
            return enderecos;
        };
        let Some(&ip) = preferido.get(host) else {
            return enderecos;
        };
        if !enderecos.contains(&ip) {
            return enderecos;
        }
        let mut ordenados = Vec::with_capacity(enderecos.len());
        ordenados.push(ip);
        ordenados.extend(enderecos.into_iter().filter(|candidato| *candidato != ip));
        ordenados
    }

    /// Registra qual endereço respondeu, para as próximas consultas.
    ///
    /// O [`HttpClient`](crate::http::HttpClient) chama isto após um sucesso.
    /// Equivale ao `ipv4Preferred.set(hostname, address)` do JS.
    pub fn marcar_preferido(&self, host: &str, ip: Ipv4Addr) {
        if let Ok(mut preferido) = self.estado.preferido.lock() {
            preferido.insert(host.to_ascii_lowercase(), ip);
        }
    }

    /// Limpa o cache (teste e troca de rede).
    pub fn limpar_cache(&self) {
        if let Ok(mut cache) = self.estado.cache.lock() {
            cache.clear();
        }
    }

    /// Quantos hosts estão em cache (para teste e diagnóstico).
    pub fn cache_len(&self) -> usize {
        self.estado.cache.lock().map(|c| c.len()).unwrap_or(0)
    }
}

impl Default for BackendResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for BackendResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendResolver")
            .field("cache", &self.cache_len())
            .finish_non_exhaustive()
    }
}

impl Resolve for BackendResolver {
    fn resolve(&self, name: Name) -> Resolving {
        // A trait exige futuro 'static: o `Arc` interno vai junto e o trabalho
        // roda no próprio future, sem thread extra.
        let estado = self.clone();
        let host = name.as_str().to_string();
        Box::pin(async move {
            let enderecos = estado.resolver(&host, 443).await.map_err(
                |erro| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::other(erro))
                },
            )?;
            let iter: Vec<SocketAddr> = enderecos
                .into_iter()
                .map(|ip| SocketAddr::new(IpAddr::V4(ip), 443))
                .collect();
            let saida: Addrs = Box::new(iter.into_iter());
            Ok(saida)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordenar_poe_o_preferido_na_frente_sem_perder_os_outros() {
        let resolver = BackendResolver::new();
        let a = Ipv4Addr::new(10, 0, 0, 1);
        let b = Ipv4Addr::new(10, 0, 0, 2);
        let c = Ipv4Addr::new(10, 0, 0, 3);
        resolver.marcar_preferido("Exemplo.COM", b);

        // O host é normalizado para minúsculas em ambas as pontas.
        let ordenados = resolver.ordenar("exemplo.com", vec![a, b, c]);
        assert_eq!(ordenados, vec![b, a, c]);
    }

    #[test]
    fn ordenar_sem_preferido_nao_muda_nada() {
        let resolver = BackendResolver::new();
        let enderecos = vec![Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(1, 0, 0, 1)];
        assert_eq!(resolver.ordenar("x.com", enderecos.clone()), enderecos);
    }

    #[test]
    fn ordenar_ignora_preferido_que_saiu_da_lista() {
        let resolver = BackendResolver::new();
        resolver.marcar_preferido("x.com", Ipv4Addr::new(9, 9, 9, 9));
        let enderecos = vec![Ipv4Addr::new(1, 1, 1, 1)];
        assert_eq!(resolver.ordenar("x.com", enderecos.clone()), enderecos);
    }

    #[test]
    fn limpar_cache_esvazia() {
        let resolver = BackendResolver::new();
        resolver.estado.cache.lock().unwrap().insert(
            "x.com".into(),
            Entrada {
                enderecos: vec![Ipv4Addr::new(1, 1, 1, 1)],
                expira: Instant::now() + CACHE_TTL,
            },
        );
        assert_eq!(resolver.cache_len(), 1);
        resolver.limpar_cache();
        assert_eq!(resolver.cache_len(), 0);
    }

    #[test]
    fn clone_compartilha_o_mesmo_cache() {
        // O clone é usado pela trait Resolve; precisa apontar para o mesmo
        // estado, senão cada resolução perdia o cache.
        let resolver = BackendResolver::new();
        let clone = resolver.clone();
        resolver.estado.cache.lock().unwrap().insert(
            "x.com".into(),
            Entrada {
                enderecos: vec![Ipv4Addr::new(1, 1, 1, 1)],
                expira: Instant::now() + CACHE_TTL,
            },
        );
        assert_eq!(clone.cache_len(), 1);
    }
}
