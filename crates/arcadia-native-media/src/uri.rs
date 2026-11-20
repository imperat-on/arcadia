//! Classificação e normalização de URIs.
//!
//! A classificação é pura (não toca a rede) e serve para decidir se o
//! GStreamer precisa de `hlsdemux` antes de abrir o pipeline. O `playbin`
//! continua sendo quem escolhe os elementos de fato: para `.m3u8`, o typefind
//! entrega `application/x-hls` ao `hlsdemux`.

use std::path::Path;

use crate::error::{Error, Result};

/// Tipo de origem de mídia, derivado somente da URI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// Manifesto HLS (`.m3u8`, normalmente HTTP(S)).
    Hls,
    /// Arquivo MP4 progressivo via HTTP(S).
    Mp4,
    /// Caminho local ou `file://`.
    LocalFile,
    /// Outra URL HTTP(S) sem extensão reconhecível (o typefind decide).
    Other,
}

impl MediaKind {
    /// Classifica uma URI sem acessar a rede nem o disco.
    ///
    /// Consultas (`?`) e fragmentos (`#`) são ignorados; a extensão é
    /// comparada sem diferenciar maiúsculas de minúsculas.
    pub fn classify(uri: &str) -> Self {
        let trimmed = uri.trim();
        let lower = trimmed.to_ascii_lowercase();

        if lower.starts_with("http://") || lower.starts_with("https://") {
            let path = http_path(trimmed).to_ascii_lowercase();
            if path.ends_with(".m3u8") {
                Self::Hls
            } else if path.ends_with(".mp4") || path.ends_with(".m4v") {
                Self::Mp4
            } else {
                Self::Other
            }
        } else if lower.starts_with("file://") {
            Self::LocalFile
        } else if scheme(trimmed).is_some() {
            Self::Other
        } else {
            Self::LocalFile
        }
    }
}

/// URI pronta para o `playbin` e o tipo detectado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSource {
    pub(crate) uri: String,
    pub(crate) kind: MediaKind,
}

/// Valida e normaliza a URI recebida por [`Player::open`](crate::Player::open).
///
/// HTTP(S) e `file://` passam como estão. Um caminho local é canonicalizado e
/// convertido para `file://` (com escape correto de espaços e acentos). Nada
/// aqui abre o pipeline; é seguro chamar antes de qualquer conexão.
pub(crate) fn resolve(input: &str) -> Result<ResolvedSource> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidUri("URI vazia".to_string()));
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok(ResolvedSource {
            uri: trimmed.to_string(),
            kind: MediaKind::classify(trimmed),
        });
    }

    if lower.starts_with("file://") {
        // Valida a forma e, para arquivo local, a existência. URIs com host
        // (file://servidor/...) ficam a cargo do GIO/GStreamer.
        match gstreamer::glib::filename_from_uri(trimmed) {
            Ok((path, None)) if !path.exists() => Err(Error::InvalidUri(format!(
                "arquivo local não encontrado: {}",
                path.display()
            ))),
            Ok(_) => Ok(ResolvedSource {
                uri: trimmed.to_string(),
                kind: MediaKind::LocalFile,
            }),
            Err(error) => Err(Error::InvalidUri(format!(
                "file:// inválido ({trimmed}): {error}"
            ))),
        }
    } else if let Some(scheme) = scheme(trimmed) {
        Err(Error::InvalidUri(format!(
            "esquema não suportado: {scheme} (use http, https, file ou um caminho local)"
        )))
    } else {
        let path = Path::new(trimmed);
        let absolute = std::fs::canonicalize(path).map_err(|error| {
            Error::InvalidUri(format!(
                "arquivo local inacessível ({}): {error}",
                path.display()
            ))
        })?;
        let uri = gstreamer::glib::filename_to_uri(&absolute, None).map_err(|error| {
            Error::InvalidUri(format!(
                "não foi possível converter {} em URI: {error}",
                absolute.display()
            ))
        })?;

        Ok(ResolvedSource {
            uri: uri.into(),
            kind: MediaKind::LocalFile,
        })
    }
}

fn http_path(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let end = after_scheme.find(['?', '#']).unwrap_or(after_scheme.len());
    &after_scheme[..end]
}

fn scheme(input: &str) -> Option<&str> {
    let (scheme, _) = input.split_once("://")?;
    let valid = !scheme.is_empty()
        && scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        });
    valid.then_some(scheme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifica_hls_mp4_e_local_sem_rede() {
        assert_eq!(
            MediaKind::classify("https://cdn.exemplo/trailer.m3u8?token=abc"),
            MediaKind::Hls
        );
        assert_eq!(
            MediaKind::classify("HTTPS://cdn.exemplo/TRAILER.M3U8#t=0"),
            MediaKind::Hls
        );
        assert_eq!(
            MediaKind::classify("https://cdn.exemplo/trailer.mp4"),
            MediaKind::Mp4
        );
        assert_eq!(
            MediaKind::classify("https://cdn.exemplo/trailer.MP4?x=1"),
            MediaKind::Mp4
        );
        assert_eq!(
            MediaKind::classify("https://cdn.exemplo/stream"),
            MediaKind::Other
        );
        assert_eq!(
            MediaKind::classify("file:///tmp/trailer.mp4"),
            MediaKind::LocalFile
        );
        assert_eq!(
            MediaKind::classify("/tmp/trailer.mp4"),
            MediaKind::LocalFile
        );
        assert_eq!(MediaKind::classify("boot.mp4"), MediaKind::LocalFile);
    }

    #[test]
    fn resolve_rejeita_esquema_desconhecido_sem_tocar_na_rede() {
        let error = resolve("ftp://exemplo/trailer.mp4").unwrap_err();
        assert!(matches!(error, Error::InvalidUri(_)), "veio: {error:?}");
        assert!(resolve("   ").is_err());
    }

    #[test]
    fn resolve_aceita_http_e_mantem_a_uri() {
        let source = resolve("https://cdn.exemplo/trailer.m3u8?token=1").unwrap();
        assert_eq!(source.kind, MediaKind::Hls);
        assert_eq!(source.uri, "https://cdn.exemplo/trailer.m3u8?token=1");
    }

    #[test]
    fn resolve_converte_caminho_local_em_file_uri() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let source = resolve(manifest.join("..").join("..").to_str().unwrap()).unwrap();
        assert_eq!(source.kind, MediaKind::LocalFile);
        assert!(source.uri.starts_with("file://"), "veio: {}", source.uri);
    }

    #[test]
    fn resolve_rejeita_file_inexistente() {
        let error = resolve("file:///caminho/que/nao/existe/trailer.mp4").unwrap_err();
        assert!(matches!(error, Error::InvalidUri(_)), "veio: {error:?}");
    }
}
