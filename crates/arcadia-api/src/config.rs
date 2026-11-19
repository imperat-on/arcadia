//! Configuração do backend do Arcadia — espelho de
//! `app/electron/supabase/config.js`.
//!
//! `ARCADIA_API_URL` é o nome canônico; `ARCADIA_SUPABASE_URL` e
//! `SUPABASE_URL` continuam aceitos apenas como compatibilidade com instalações
//! antigas. Com qualquer override a lista de endereços contém somente ele: um
//! servidor próprio não deve cair no backend oficial.

/// Endereço público oficial (Worker do Cloudflare).
pub const DEFAULT_API_URL: &str = "https://arcadiaserver.zesmehentperu.workers.dev";
/// Reserva usada quando o endereço público falha por rede (Funnel do Tailscale).
pub const FALLBACK_API_URL: &str = "https://zes.tail6e748d.ts.net";
/// Chave anônima usada quando `SUPABASE_ANON_KEY` não está definida.
pub const DEFAULT_ANON_KEY: &str = "arcadia-dummy-key";

/// Configuração resolvida do backend.
///
/// `url` é sempre o endereço primário que o cliente usa ao montar caminhos;
/// `urls` é a lista de tentativas em falha de rede. Os campos são públicos para
/// permitir configuração programática (por exemplo, testes com servidores
/// locais), mas [`ApiConfig::resolve`] e [`ApiConfig::from_env`] aplicam as
/// mesmas regras de normalização do cliente Electron.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiConfig {
    pub url: String,
    pub urls: Vec<String>,
    pub anon_key: String,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self::resolve(None, None, None, None)
    }
}

impl ApiConfig {
    /// Resolve a configuração a partir do ambiente do processo.
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Resolve a configuração a partir de uma função de consulta.
    ///
    /// Existe para testes determinísticos: nenhum teste precisa mutar o
    /// ambiente global (e disputar com testes paralelos).
    pub fn from_lookup<F>(mut lookup: F) -> Self
    where
        F: FnMut(&str) -> Option<String>,
    {
        Self::resolve(
            lookup("ARCADIA_API_URL").as_deref(),
            lookup("ARCADIA_SUPABASE_URL").as_deref(),
            lookup("SUPABASE_URL").as_deref(),
            lookup("SUPABASE_ANON_KEY").as_deref(),
        )
    }

    /// Aplica a precedência canônica:
    /// `ARCADIA_API_URL` → `ARCADIA_SUPABASE_URL` → `SUPABASE_URL`.
    ///
    /// Valores vazios (ou só espaços) são ignorados. A anon key vazia cai no
    /// valor padrão, como o `||` do JavaScript.
    pub fn resolve(
        arcadia_api_url: Option<&str>,
        arcadia_supabase_url: Option<&str>,
        supabase_url: Option<&str>,
        anon_key: Option<&str>,
    ) -> Self {
        let override_url = normalize_url(arcadia_api_url.unwrap_or_default())
            .or_else(|| normalize_url(arcadia_supabase_url.unwrap_or_default()))
            .or_else(|| normalize_url(supabase_url.unwrap_or_default()));

        let (url, urls) = match override_url {
            Some(url) => (url.clone(), vec![url]),
            None => (
                DEFAULT_API_URL.to_string(),
                dedupe(vec![
                    DEFAULT_API_URL.to_string(),
                    FALLBACK_API_URL.to_string(),
                ]),
            ),
        };

        let anon_key = anon_key
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_ANON_KEY)
            .to_string();

        Self {
            url,
            urls,
            anon_key,
        }
    }
}

/// Normaliza uma URL como `normalizeUrl` do JS: `trim`, remove todas as barras
/// finais e devolve `None` quando o resultado fica vazio.
pub fn normalize_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let without_trailing = trimmed.trim_end_matches('/');
    if without_trailing.is_empty() {
        None
    } else {
        Some(without_trailing.to_string())
    }
}

fn dedupe(urls: Vec<String>) -> Vec<String> {
    let mut unique = Vec::with_capacity(urls.len());
    for url in urls {
        if !unique.contains(&url) {
            unique.push(url);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_urls_are_primary_plus_fallback() {
        let config = ApiConfig::resolve(None, None, None, None);
        assert_eq!(config.url, DEFAULT_API_URL);
        assert_eq!(config.urls, vec![DEFAULT_API_URL, FALLBACK_API_URL]);
        assert_eq!(config.anon_key, DEFAULT_ANON_KEY);
    }

    #[test]
    fn canonical_override_wins_over_legacy_names() {
        let config = ApiConfig::resolve(
            Some(" https://canonical.example/ "),
            Some("https://legacy.example"),
            Some("https://old.example"),
            Some("key"),
        );
        assert_eq!(config.url, "https://canonical.example");
        assert_eq!(config.urls, vec!["https://canonical.example"]);
        assert_eq!(config.anon_key, "key");
    }

    #[test]
    fn legacy_names_follow_precedence_and_empty_values_are_ignored() {
        let config = ApiConfig::resolve(
            Some("   "),
            Some("https://legacy.example///"),
            Some("https://old.example"),
            Some(""),
        );
        assert_eq!(config.url, "https://legacy.example");
        assert_eq!(config.urls, vec!["https://legacy.example"]);
        assert_eq!(config.anon_key, DEFAULT_ANON_KEY);
    }

    #[test]
    fn supabase_url_is_the_last_alias() {
        let config = ApiConfig::resolve(None, None, Some("http://127.0.0.1:3000/"), None);
        assert_eq!(config.url, "http://127.0.0.1:3000");
        assert_eq!(config.urls, vec!["http://127.0.0.1:3000"]);
    }

    #[test]
    fn from_lookup_reads_all_names_without_touching_process_env() {
        let config = ApiConfig::from_lookup(|name| match name {
            "ARCADIA_SUPABASE_URL" => Some("  https://alias.example/  ".to_string()),
            "SUPABASE_ANON_KEY" => Some("anon".to_string()),
            _ => None,
        });
        assert_eq!(config.url, "https://alias.example");
        assert_eq!(config.urls, vec!["https://alias.example"]);
        assert_eq!(config.anon_key, "anon");
    }

    #[test]
    fn normalize_matches_javascript_semantics() {
        assert_eq!(normalize_url("  https://x/  "), Some("https://x".into()));
        assert_eq!(normalize_url("https://x///"), Some("https://x".into()));
        assert_eq!(normalize_url("   "), None);
        assert_eq!(normalize_url("///"), None);
        assert_eq!(
            normalize_url("http://127.0.0.1:3000"),
            Some("http://127.0.0.1:3000".into())
        );
    }
}
