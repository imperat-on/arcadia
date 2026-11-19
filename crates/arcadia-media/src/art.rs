//! Arte: URLs de pesquisa/download, cache local e cascata de fontes.
//!
//! Reproduz de `app/electron/metadata.js`:
//!
//! - `arteQuadrada` (PSN → Worker → Xbox → SGDB, primeira que responder);
//! - `logoAuto` (PSN → SGDB, com teto de 4096 px para o Chromium decodificar);
//! - `urlCardDe` (reduções de PSN/SGDB para o card do trilho);
//! - `downloadTo`/`extFromUrl` e os nomes de arquivo de `art/`;
//! - `artToDelete` de `app/electron/overrides.js`.
//!
//! O download recebe um [`Fetcher`]; a gravação fica atrás de [`ArtWriter`],
//! então os testes não tocam o disco. [`FsWriter`] é a implementação padrão.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{MediaError, Result};
use crate::http::{encode_uri_component, get_json, value_number, value_truthy, Fetcher, Request};
use crate::providers::psn::{psn_media_de, PsnClient};
use crate::providers::steamgriddb::{SgdbArtOptions, SteamGridDb};
use crate::providers::xbox::{xbox_img, XboxClient, XboxImageOptions, XboxLocalizedProduct};
use crate::providers::ArtKind;
use crate::text::extension_from_url;

/// Teto de pixels de uma logo na SGDB (o Chromium recusa acima disto).
pub const LOGO_MAX_PX: u32 = 4096;
/// Tamanho mínimo da arte quadrada do Xbox.
pub const SQUARE_ART_MIN: u32 = 512;
/// Nome da pasta de artes escolhidas dentro do diretório de dados.
pub const ART_DIR_NAME: &str = "art";
/// Entradas do cache de arte quadrada/logo vencem em 30 dias (`main.js`).
pub const ART_CACHE_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Diretório de arte a partir do diretório de dados (`data_dir/art`).
pub fn art_dir(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join(ART_DIR_NAME)
}
/// Ordem de preferência dos `ImagePurpose` quadrados do catálogo Xbox.
pub const SQUARE_ART_PURPOSES: &[&str] = &[
    "BoxArt",
    "FeaturePromotionalSquareArt",
    "Tile",
    "BrandedKeyArt",
    "Logo",
];

/// Arte 1:1 do trilho (hero do Big Picture).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SquareArt {
    pub url: String,
    #[serde(rename = "urlCard")]
    pub url_card: String,
    pub fonte: String,
    pub w: Option<u32>,
    pub h: Option<u32>,
}

/// Logo transparente do hero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogoArt {
    pub url: String,
    pub fonte: String,
    pub w: Option<u32>,
    pub h: Option<u32>,
}

/// Resultado de `downloadTo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedArt {
    pub path: PathBuf,
    pub bytes: usize,
}

/// Destino de bytes baixados.
pub trait ArtWriter: Send + Sync {
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
}

/// Gravação no sistema de arquivos.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsWriter;

impl ArtWriter for FsWriter {
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
    }
}

/// `safeId` de art: preserva `[a-z0-9._-]` e troca o resto por `_`.
pub fn safe_id(id: &str) -> String {
    id.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// Base do arquivo de arte escolhida: `{art}/{safeId}-{kind}-{timestamp}`.
pub fn art_cache_base(
    art_dir: impl AsRef<Path>,
    id: &str,
    kind: ArtKind,
    timestamp_ms: u64,
) -> PathBuf {
    art_dir.as_ref().join(format!(
        "{}-{}-{}",
        safe_id(id),
        kind.as_str(),
        timestamp_ms
    ))
}

/// `main.js` apaga a entrada quando `agora - quando > 30 dias`; o limite
/// exato ainda é considerado fresco.
pub fn art_cache_is_fresh(cached_at_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(cached_at_ms) <= ART_CACHE_TTL_MS
}

/// Caminho de uma arte importada pelo usuário (`art:pick`).
///
/// `extension` é o `path.extname` do arquivo de origem; vazio cai em `.png`.
pub fn art_pick_path(
    art_dir: impl AsRef<Path>,
    id: &str,
    kind: ArtKind,
    timestamp_ms: u64,
    extension: &str,
) -> PathBuf {
    let base = art_cache_base(art_dir, id, kind, timestamp_ms);
    let extension = if extension.is_empty() {
        ".png"
    } else {
        extension
    };
    let mut path = OsString::from(base.as_os_str());
    path.push(extension);
    PathBuf::from(path)
}

/// `artToDelete`: só apaga o que vive em `art/` e não contém `..`.
///
/// `separator` é o separador de caminho do chamador (`std::path::MAIN_SEPARATOR`
/// em produção; `/` nos testes multiplataforma).
pub fn art_to_delete(previous: &str, art_dir: &str, separator: &str) -> Option<PathBuf> {
    if previous.is_empty() {
        return None;
    }
    let path = previous.strip_prefix("file://").unwrap_or(previous);
    let root = if art_dir.ends_with(separator) {
        art_dir.to_string()
    } else {
        format!("{art_dir}{separator}")
    };
    if !path.starts_with(&root) {
        return None;
    }
    if path.contains("..") {
        return None;
    }
    Some(PathBuf::from(path))
}

/// [`art_to_delete`] usando o separador nativo do sistema.
pub fn art_to_delete_in(previous: &str, art_dir: impl AsRef<Path>) -> Option<PathBuf> {
    art_to_delete(
        previous,
        &art_dir.as_ref().to_string_lossy(),
        std::path::MAIN_SEPARATOR_STR,
    )
}

/// `urlCardDe`: reduz a arte para o card do trilho.
pub fn url_card_de(url: &str) -> Option<String> {
    static SGDB_GRID: OnceLock<Regex> = OnceLock::new();
    if url.is_empty() {
        return None;
    }
    if url.contains("image.api.playstation.com") {
        let separator = if url.contains('?') { '&' } else { '?' };
        return Some(format!("{url}{separator}w=384"));
    }
    let regex = SGDB_GRID.get_or_init(|| {
        Regex::new(r"(?i)^https://cdn2\.steamgriddb\.com/grid/([^./?]+)\.(png|jpg|jpeg|webp)")
            .expect("regex de grid da SGDB")
    });
    if let Some(capture) = regex.captures(url) {
        return Some(format!(
            "https://cdn2.steamgriddb.com/thumb/{}.jpg",
            &capture[1]
        ));
    }
    // Fontes que já aceitam tamanho na montagem (Xbox) ficam como estão.
    Some(url.to_string())
}

/// Caminho final do download (`destSemExt + extensão`).
pub fn download_path(base: &Path, url: &str, content_type: &str) -> PathBuf {
    let extension = extension_from_url(url, content_type);
    let mut path = OsString::from(base.as_os_str());
    path.push(extension);
    PathBuf::from(path)
}

/// `downloadTo`: baixa com o fetcher injetado e grava com [`FsWriter`].
pub async fn download_to(
    fetcher: &dyn Fetcher,
    url: &str,
    dest_without_extension: impl AsRef<Path>,
) -> Result<DownloadedArt> {
    download_to_with(fetcher, url, dest_without_extension, &FsWriter).await
}

/// Igual a [`download_to`], com writer injetado.
pub async fn download_to_with<W: ArtWriter + ?Sized>(
    fetcher: &dyn Fetcher,
    url: &str,
    dest_without_extension: impl AsRef<Path>,
    writer: &W,
) -> Result<DownloadedArt> {
    let response = fetcher.fetch(Request::get(url)).await?;
    if !response.is_success() {
        return Err(MediaError::Download {
            status: response.status,
        });
    }
    let content_type = response.find_header("content-type").unwrap_or("");
    let path = download_path(dest_without_extension.as_ref(), url, content_type);
    writer.write(&path, &response.body).map_err(|error| {
        MediaError::message(format!("falha ao gravar {}: {error}", path.display()))
    })?;
    Ok(DownloadedArt {
        bytes: response.body.len(),
        path,
    })
}

/// `arteQuadradaPSN`: capa 1:1 oficial (papel `MASTER`).
pub async fn square_art_psn(client: &PsnClient, title: &str) -> Option<SquareArt> {
    let game = client.game_from_search(title).await.ok().flatten()?;
    let url = psn_media_de(&game, "MASTER")?;
    Some(SquareArt {
        url_card: url_card_de(&url).unwrap_or_default(),
        url,
        fonte: "psn".to_string(),
        w: None,
        h: None,
    })
}

/// `logoPSN`: logo transparente oficial (papel `LOGO`, `?w=512`).
pub async fn logo_psn(client: &PsnClient, title: &str) -> Option<LogoArt> {
    let game = client.game_from_search(title).await.ok().flatten()?;
    let url = psn_media_de(&game, "LOGO")?;
    let separator = if url.contains('?') { '&' } else { '?' };
    Some(LogoArt {
        url: format!("{url}{separator}w=512"),
        fonte: "psn".to_string(),
        w: None,
        h: None,
    })
}

/// Melhor imagem quadrada (`>=512`) do produto Xbox, na ordem congelada.
pub fn xbox_square_image(
    product: &XboxLocalizedProduct,
) -> Option<&crate::providers::xbox::XboxImage> {
    let mut images: Vec<&crate::providers::xbox::XboxImage> = product
        .images
        .iter()
        .filter(|image| image.width == image.height && image.width >= SQUARE_ART_MIN)
        .collect();
    if images.is_empty() {
        return None;
    }
    images.sort_by(|left, right| {
        let left_index = SQUARE_ART_PURPOSES
            .iter()
            .position(|purpose| *purpose == left.purpose)
            .map(|index| index as i32)
            .unwrap_or(-1);
        let right_index = SQUARE_ART_PURPOSES
            .iter()
            .position(|purpose| *purpose == right.purpose)
            .map(|index| index as i32)
            .unwrap_or(-1);
        left_index
            .cmp(&right_index)
            .then_with(|| right.width.cmp(&left.width))
    });
    images.first().copied()
}

/// `arteQuadradaXbox`: arte 1:1 pelo catálogo público, sem chave.
pub async fn square_art_xbox(client: &XboxClient, title: &str) -> Option<SquareArt> {
    let games = client.search(title, "US", "en-us").await.ok()?;
    let product_id = games.first()?.id.clone();
    if product_id.is_empty() {
        return None;
    }
    let product = client
        .product(&product_id, "US", "en-us")
        .await
        .ok()
        .flatten()?;
    let image = xbox_square_image(&product)?;
    Some(SquareArt {
        url: xbox_img(
            &image.uri,
            XboxImageOptions {
                width: Some(image.width),
                height: Some(image.height),
                quality: 100,
            },
        ),
        url_card: xbox_img(
            &image.uri,
            XboxImageOptions {
                width: Some(384),
                height: Some(384),
                quality: 100,
            },
        ),
        fonte: "xbox".to_string(),
        w: Some(image.width),
        h: Some(image.height),
    })
}

/// `arteQuadradaWorker`: arte 1:1 do Worker do Arcadia (a chave da SGDB fica
/// lá, com cache de borda de 7 dias).
pub async fn square_art_worker(
    fetcher: &dyn Fetcher,
    title: &str,
    base_worker: &str,
) -> Option<SquareArt> {
    if base_worker.is_empty() {
        return None;
    }
    let base = base_worker.strip_suffix('/').unwrap_or(base_worker);
    let url = format!(
        "{base}/arte-quadrada?titulo={}",
        encode_uri_component(title)
    );
    let json = get_json(fetcher, url, &[]).await.ok()?;
    let url = json.get("url")?;
    if !value_truthy(Some(url)) {
        return None;
    }
    let url = crate::http::value_str(Some(url));
    let fonte = format!(
        "worker-{}",
        match json.get("fonte").map(|fonte| value_truthy(Some(fonte))) {
            Some(true) => crate::http::value_str(json.get("fonte")),
            _ => "sgdb".to_string(),
        }
    );
    Some(SquareArt {
        url_card: url_card_de(&url).unwrap_or_default(),
        url,
        fonte,
        w: number_field(json.get("w")),
        h: number_field(json.get("h")),
    })
}

fn number_field(value: Option<&Value>) -> Option<u32> {
    value
        .map(|value| value_number(Some(value)))
        .filter(|value| *value > 0.0)
        .map(|value| value as u32)
}

/// `arteQuadradaSgdb`: SGDB direto, só quando o config local tem chave.
///
/// O oráculo chama `sgdbArt(id, "grid", …)`, mas `SGDB_ENDPOINT` só conhece
/// `cover|hero|logo`; a chamada sempre lança e a cascata segue. O erro é
/// preservado aqui (bug-for-bug) em vez de inventar um endpoint `grid`.
pub async fn square_art_sgdb(client: &SteamGridDb, title: &str) -> Option<SquareArt> {
    if !client.has_key() {
        return None;
    }
    let games = client.search(title).await.ok()?;
    let id = games.first()?.id;
    let options = SgdbArtOptions::default().with_dimensions(["1024x1024", "512x512"]);
    let art = client.art_with_kind(id, "grid", &options).await.ok()?;
    let first = art.first()?;
    Some(SquareArt {
        url: first.url.clone(),
        url_card: if first.thumb.is_empty() {
            url_card_de(&first.url).unwrap_or_default()
        } else {
            first.thumb.clone()
        },
        fonte: "sgdb".to_string(),
        w: first.largura,
        h: first.altura,
    })
}

/// Cascata completa de arte 1:1, na ordem medida pelo cliente.
pub struct ArtResolver {
    fetcher: Arc<dyn Fetcher>,
    psn: PsnClient,
    xbox: XboxClient,
    sgdb: Option<SteamGridDb>,
    worker_base: Option<String>,
}

impl ArtResolver {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            psn: PsnClient::new(Arc::clone(&fetcher)),
            xbox: XboxClient::new(Arc::clone(&fetcher)),
            sgdb: None,
            worker_base: None,
            fetcher,
        }
    }

    pub fn with_worker_base(mut self, base: impl Into<String>) -> Self {
        self.worker_base = Some(base.into());
        self
    }

    pub fn with_sgdb(mut self, client: SteamGridDb) -> Self {
        self.sgdb = Some(client);
        self
    }

    pub fn with_psn(mut self, client: PsnClient) -> Self {
        self.psn = client;
        self
    }

    pub fn with_xbox(mut self, client: XboxClient) -> Self {
        self.xbox = client;
        self
    }

    pub fn psn(&self) -> &PsnClient {
        &self.psn
    }

    pub fn xbox(&self) -> &XboxClient {
        &self.xbox
    }

    pub fn sgdb(&self) -> Option<&SteamGridDb> {
        self.sgdb.as_ref()
    }

    /// Primeira fonte que devolver `url` vence; erros são engolidos.
    pub async fn square_art(&self, title: &str) -> Option<SquareArt> {
        let title = title.trim();
        if title.is_empty() {
            return None;
        }
        if let Some(art) = square_art_psn(&self.psn, title).await {
            if !art.url.is_empty() {
                return Some(art);
            }
        }
        if let Some(base) = self.worker_base.as_deref() {
            if let Some(art) = square_art_worker(self.fetcher.as_ref(), title, base).await {
                if !art.url.is_empty() {
                    return Some(art);
                }
            }
        }
        if let Some(art) = square_art_xbox(&self.xbox, title).await {
            if !art.url.is_empty() {
                return Some(art);
            }
        }
        if let Some(sgdb) = self.sgdb.as_ref() {
            if let Some(art) = square_art_sgdb(sgdb, title).await {
                if !art.url.is_empty() {
                    return Some(art);
                }
            }
        }
        None
    }

    /// `logoAuto`: PSN primeiro; SGDB como reserva, com o teto de 4096 px.
    pub async fn logo(&self, title: &str) -> Option<LogoArt> {
        let title = title.trim();
        if title.is_empty() {
            return None;
        }
        if let Some(logo) = logo_psn(&self.psn, title).await {
            if !logo.url.is_empty() {
                return Some(logo);
            }
        }
        let sgdb = self.sgdb.as_ref()?;
        if !sgdb.has_key() {
            return None;
        }
        let games = sgdb.search(title).await.ok()?;
        let id = games.first()?.id;
        let logos = sgdb
            .art(id, ArtKind::Logo, &SgdbArtOptions::default())
            .await
            .ok()?;
        let logos: Vec<_> = logos
            .into_iter()
            .filter(|logo| !logo.url.is_empty())
            .collect();
        if logos.is_empty() {
            return None;
        }
        let is_sane = |logo: &crate::providers::Artwork| match (logo.largura, logo.altura) {
            (Some(width), Some(height)) => {
                width == 0 || height == 0 || (width <= LOGO_MAX_PX && height <= LOGO_MAX_PX)
            }
            _ => true,
        };
        if let Some(large) = logos.iter().find(|logo| is_sane(logo)) {
            return Some(LogoArt {
                url: large.url.clone(),
                fonte: "sgdb".to_string(),
                w: large.largura,
                h: large.altura,
            });
        }
        logos
            .iter()
            .find(|logo| !logo.thumb.is_empty())
            .map(|thumb| LogoArt {
                url: thumb.thumb.clone(),
                fonte: "sgdb-thumb".to_string(),
                w: None,
                h: None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;
    use crate::providers::xbox::XboxImage;
    use crate::test_support::{json_response, MockFetcher, RecordingWriter};
    use serde_json::json;
    use std::sync::Arc;

    fn product(images: Vec<XboxImage>) -> XboxLocalizedProduct {
        XboxLocalizedProduct {
            product_title: "Portal 2".into(),
            product_description: String::new(),
            images,
        }
    }

    fn image(purpose: &str, uri: &str, width: u32, height: u32) -> XboxImage {
        XboxImage {
            purpose: purpose.into(),
            uri: uri.into(),
            width,
            height,
        }
    }

    #[test]
    fn safe_id_and_cache_paths_preserve_the_client_rules() {
        assert_eq!(safe_id("steam:10/path"), "steam_10_path");
        assert_eq!(safe_id("retro:sony-psp"), "retro_sony-psp");
        let base = art_cache_base("C:/data/art", "steam:10", ArtKind::Hero, 123);
        assert_eq!(base, PathBuf::from("C:/data/art").join("steam_10-hero-123"));
        let picked = art_pick_path("C:/data/art", "steam:10", ArtKind::Logo, 9, ".webp");
        assert_eq!(
            picked,
            PathBuf::from("C:/data/art").join("steam_10-logo-9.webp")
        );
        assert_eq!(
            art_pick_path("art", "x", ArtKind::Cover, 1, ""),
            PathBuf::from("art").join("x-cover-1.png")
        );
        assert_eq!(art_dir("data"), PathBuf::from("data").join("art"));
        assert!(art_cache_is_fresh(1_000, 1_000 + ART_CACHE_TTL_MS));
        assert!(!art_cache_is_fresh(1_000, 1_001 + ART_CACHE_TTL_MS));
    }

    #[test]
    fn art_to_delete_stays_inside_art_dir() {
        assert_eq!(
            art_to_delete("file:///data/art/steam_10-cover-1.png", "/data/art", "/"),
            Some(PathBuf::from("/data/art/steam_10-cover-1.png"))
        );
        assert_eq!(
            art_to_delete("https://cdn.example/cover.png", "/data/art", "/"),
            None
        );
        assert_eq!(
            art_to_delete("/data/art/../secret.png", "/data/art", "/"),
            None
        );
        assert_eq!(
            art_to_delete("/data/art2/steam.png", "/data/art", "/"),
            None
        );
        assert_eq!(art_to_delete("", "/data/art", "/"), None);
    }

    #[test]
    fn url_card_rewrites_only_psn_and_sgdb_grids() {
        assert_eq!(
            url_card_de("https://image.api.playstation.com/a.png").as_deref(),
            Some("https://image.api.playstation.com/a.png?w=384")
        );
        assert_eq!(
            url_card_de("https://image.api.playstation.com/a.png?x=1").as_deref(),
            Some("https://image.api.playstation.com/a.png?x=1&w=384")
        );
        assert_eq!(
            url_card_de("https://cdn2.steamgriddb.com/grid/abc.png").as_deref(),
            Some("https://cdn2.steamgriddb.com/thumb/abc.jpg")
        );
        assert_eq!(
            url_card_de("https://cdn2.steamgriddb.com/grid/abc.webp?x=1").as_deref(),
            Some("https://cdn2.steamgriddb.com/thumb/abc.jpg")
        );
        assert_eq!(
            url_card_de("https://img.example/x.png").as_deref(),
            Some("https://img.example/x.png")
        );
        assert_eq!(url_card_de(""), None);
    }

    #[tokio::test]
    async fn download_chooses_extension_and_writes_injected_sink() {
        let fetcher = MockFetcher::new(vec![Response::new(200, b"png-bytes".to_vec())
            .header("content-type", "image/png")
            .header("x", "y")]);
        let writer = RecordingWriter::default();
        let downloaded = download_to_with(
            &fetcher,
            "https://cdn.example/art/no-extension?id=1",
            "C:/data/art/steam_10-cover-1",
            &writer,
        )
        .await
        .unwrap();
        assert_eq!(downloaded.bytes, 9);
        assert_eq!(
            downloaded.path,
            PathBuf::from("C:/data/art/steam_10-cover-1.png")
        );
        assert_eq!(writer.files().len(), 1);
        assert_eq!(writer.files()[0].0, downloaded.path);
        assert_eq!(writer.files()[0].1, b"png-bytes".to_vec());
        assert_eq!(fetcher.requests()[0].method, crate::http::Method::Get);
    }

    #[tokio::test]
    async fn download_uses_the_url_extension_over_content_type() {
        let fetcher = MockFetcher::new(vec![
            Response::new(200, b"webm".to_vec()).header("content-type", "video/mp4")
        ]);
        let writer = RecordingWriter::default();
        let downloaded = download_to_with(
            &fetcher,
            "https://cdn.example/hero.webm",
            "/art/base",
            &writer,
        )
        .await
        .unwrap();
        assert_eq!(downloaded.path, PathBuf::from("/art/base.webm"));
    }

    #[tokio::test]
    async fn download_http_error_is_typed_and_does_not_write() {
        let fetcher = MockFetcher::new(vec![Response::new(404, "")]);
        let writer = RecordingWriter::default();
        let error = download_to_with(&fetcher, "https://x/a.png", "/art/base", &writer)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "download falhou: HTTP 404");
        assert!(writer.files().is_empty());
    }

    #[test]
    fn square_image_filter_and_priority_match_the_client() {
        let catalog_product = product(vec![
            image("Screenshot", "//i/screenshot.jpg", 3840, 3840),
            image("BoxArt", "//i/box.jpg", 1080, 1080),
            image("Tile", "//i/tile.jpg", 2160, 2160),
            image("Poster", "//i/poster.jpg", 600, 900),
            image("Logo", "//i/small.png", 256, 256),
        ]);
        let best = xbox_square_image(&catalog_product).unwrap();
        // Propósito desconhecido tem índice -1 no `indexOf` do JS e vence a
        // ordem declarada — comportamento congelado, preservado aqui.
        assert_eq!(best.uri, "//i/screenshot.jpg");
        assert!(xbox_square_image(&product(vec![image("BoxArt", "//i/x", 511, 511)])).is_none());
        assert!(xbox_square_image(&product(vec![image("BoxArt", "//i/x", 512, 512)])).is_some());
    }

    #[tokio::test]
    async fn psn_square_art_and_logo() {
        let body = json!({"data": {"universalSearch": {"results": [{
            "name": "Portal 2 Complete Edition",
            "media": [
                {"type": "IMAGE", "role": "MASTER", "url": "https://image.api.playstation.com/master.png"},
                {"type": "IMAGE", "role": "LOGO", "url": "https://image.api.playstation.com/logo.png"}
            ]
        }]}}});
        let fetcher = MockFetcher::new(vec![json_response(body.clone()), json_response(body)]);
        let psn = PsnClient::new(fetcher);
        let art = square_art_psn(&psn, "Portal 2 Complete").await.unwrap();
        assert_eq!(art.url, "https://image.api.playstation.com/master.png");
        assert_eq!(
            art.url_card,
            "https://image.api.playstation.com/master.png?w=384"
        );
        assert_eq!(art.fonte, "psn");
        let logo = logo_psn(&psn, "Portal 2 Complete").await.unwrap();
        assert_eq!(logo.url, "https://image.api.playstation.com/logo.png?w=512");
    }

    #[tokio::test]
    async fn xbox_square_art_uses_catalog_and_card_size() {
        let fetcher = MockFetcher::new(vec![
            json_response(
                json!({"Payload": {"SearchResults": [{"ProductId": "9A", "Title": "Portal 2"}]}}),
            ),
            json_response(json!({"Products": [{"LocalizedProperties": [{
                "ProductTitle": "Portal 2",
                "ProductDescription": "",
                "Images": [
                    {"ImagePurpose": "BoxArt", "Uri": "//i/box.jpg", "Width": 1080, "Height": 1080}
                ]
            }]}]})),
        ]);
        let xbox = XboxClient::new(fetcher);
        let art = square_art_xbox(&xbox, "Portal 2").await.unwrap();
        assert_eq!(art.url, "https://i/box.jpg?q=100&format=jpg&w=1080&h=1080");
        assert_eq!(
            art.url_card,
            "https://i/box.jpg?q=100&format=jpg&w=384&h=384"
        );
        assert_eq!(art.fonte, "xbox");
        assert_eq!((art.w, art.h), (Some(1080), Some(1080)));
    }

    #[tokio::test]
    async fn worker_square_art_builds_query_and_reads_fields() {
        let fetcher = MockFetcher::new(vec![json_response(json!({
            "url": "https://cdn2.steamgriddb.com/grid/abc.png",
            "fonte": "sgdb",
            "w": 1024,
            "h": 1024
        }))]);
        let art = square_art_worker(&fetcher, "Portal 2", "https://worker.example/")
            .await
            .unwrap();
        assert_eq!(art.fonte, "worker-sgdb");
        assert_eq!(art.url_card, "https://cdn2.steamgriddb.com/thumb/abc.jpg");
        assert_eq!((art.w, art.h), (Some(1024), Some(1024)));
        assert_eq!(
            fetcher.requests()[0].url,
            "https://worker.example/arte-quadrada?titulo=Portal%202"
        );
        assert_eq!(square_art_worker(&fetcher, "x", "").await, None);
    }

    #[tokio::test]
    async fn resolver_falls_through_psn_to_worker() {
        let fetcher = MockFetcher::new(vec![
            json_response(json!({"data": {"universalSearch": {"results": []}}})),
            json_response(json!({"url": "https://worker.example/square.png", "fonte": "psn"})),
        ]);
        let resolver = ArtResolver::new(fetcher.clone()).with_worker_base("https://worker.example");
        let art = resolver.square_art("Portal 2").await.unwrap();
        assert_eq!(art.fonte, "worker-psn");
        assert_eq!(fetcher.requests().len(), 2);
    }

    #[tokio::test]
    async fn resolver_returns_none_when_every_source_fails() {
        let fetcher = MockFetcher::new(vec![
            Response::new(500, ""),
            Response::new(500, ""),
            json_response(json!({"Payload": {"SearchResults": []}})),
        ]);
        let resolver = ArtResolver::new(fetcher).with_worker_base("https://w");
        assert_eq!(resolver.square_art("Portal 2").await, None);
        assert_eq!(resolver.square_art("   ").await, None);
    }

    #[tokio::test]
    async fn logo_falls_back_to_sgdb_thumb_when_the_png_is_too_large() {
        let fetcher = MockFetcher::new(vec![
            json_response(json!({"data": {"universalSearch": {"results": []}}})),
            json_response(json!({"success": true, "data": [{"id": 5, "name": "Portal 2"}]})),
            json_response(json!({"success": true, "data": [{
                "url": "https://cdn2.steamgriddb.com/logo/huge.png",
                "thumb": "https://cdn2.steamgriddb.com/thumb/huge.jpg",
                "width": 9657,
                "height": 12589
            }]})),
        ]);
        let sgdb = SteamGridDb::new(fetcher.clone(), "key");
        let resolver = ArtResolver::new(fetcher).with_sgdb(sgdb);
        let logo = resolver.logo("Portal 2").await.unwrap();
        assert_eq!(logo.url, "https://cdn2.steamgriddb.com/thumb/huge.jpg");
        assert_eq!(logo.fonte, "sgdb-thumb");
    }

    #[tokio::test]
    async fn logo_prefers_a_sane_sgdb_png_over_the_thumb() {
        let fetcher = MockFetcher::new(vec![
            json_response(json!({"data": {"universalSearch": {"results": []}}})),
            json_response(json!({"success": true, "data": [{"id": 5, "name": "Portal 2"}]})),
            json_response(json!({"success": true, "data": [
                {"url": "https://cdn2.steamgriddb.com/logo/big.png", "width": 8000, "height": 8000},
                {"url": "https://cdn2.steamgriddb.com/logo/ok.png", "width": 1024, "height": 512}
            ]})),
        ]);
        let sgdb = SteamGridDb::new(fetcher.clone(), "key");
        let resolver = ArtResolver::new(fetcher).with_sgdb(sgdb);
        let logo = resolver.logo("Portal 2").await.unwrap();
        assert_eq!(logo.url, "https://cdn2.steamgriddb.com/logo/ok.png");
        assert_eq!(logo.fonte, "sgdb");
        assert_eq!((logo.w, logo.h), (Some(1024), Some(512)));
    }

    #[tokio::test]
    async fn resolver_uses_the_same_fetcher_for_worker_and_clients() {
        let fetcher = MockFetcher::new(vec![]);
        let resolver = ArtResolver::new(Arc::clone(&fetcher) as Arc<dyn Fetcher>);
        assert!(resolver.sgdb().is_none());
        assert_eq!(resolver.logo("x").await, None);
    }
}
