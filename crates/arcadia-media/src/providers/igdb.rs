//! IGDB pelo proxy público do Playnite.
//!
//! Duas chamadas por busca: `POST {proxy}/igdb/search` devolve candidatos
//! fuzzy (sem resumo/artes) e `GET {proxy}/igdb/game/{id}` devolve a ficha
//! completa. O resultado fica num LRU de 100 entradas/1 hora; falhas
//! consecutivas abrem um circuit breaker de 5 falhas/60 s.
//!
//! A IGDB não tem campo de idioma: o texto vem sempre em inglês, por isso os
//! provedores que traduzem vêm antes na ordem de qualidade.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cache::{lock, BreakerState, CircuitBreaker, Clock, LruCache, SystemClock};
use crate::error::{MediaError, Result};
use crate::http::{value_number, value_str, value_truthy, Fetcher, Request, IGDB_TIMEOUT};
use crate::providers::{ArtKind, Artwork, TextCandidate};
use crate::text::{normalize_title, strip_html};

pub const IGDB_PROXY: &str = "https://api2.playnite.link/api";
pub const IGDB_IMG: &str = "https://images.igdb.com/igdb/image/upload";
pub const IGDB_UA: &str = "Arcadia (github.com/imperat-on/arcadia)";

/// Cache da IGDB: 100 entradas, TTL de 1 hora (o mesmo do JS).
pub const IGDB_CACHE_SIZE: usize = 100;
pub const IGDB_CACHE_TTL_MS: u64 = 3_600_000;
/// Circuit breaker: 5 falhas consecutivas, 60 s aberto.
pub const IGDB_BREAKER_THRESHOLD: u32 = 5;
pub const IGDB_BREAKER_TIMEOUT_MS: u64 = 60_000;

/// Plataforma da ficha de busca (só o nome importa para o score).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgdbPlatform {
    pub name: String,
}

/// Candidato da busca fuzzy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgdbCandidate {
    pub id: i64,
    pub name: String,
    pub platforms: Vec<IgdbPlatform>,
    pub first_release_date: Option<i64>,
}

/// Parâmetros de `scoreIgdbMatch`/`igdbProxy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgdbSearch {
    pub title: String,
    pub platform: Option<String>,
    pub year: Option<i32>,
}

impl IgdbSearch {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            platform: None,
            year: None,
        }
    }

    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = Some(platform.into());
        self
    }

    pub fn with_year(mut self, year: i32) -> Self {
        self.year = Some(year);
        self
    }

    fn cache_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.title.to_lowercase(),
            self.platform.as_deref().unwrap_or(""),
            self.year
                .filter(|year| *year != 0)
                .map(|year| year.to_string())
                .unwrap_or_default()
        )
    }
}

/// Imagem da IGDB (só o `image_id` é usado para montar as URLs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgdbImage {
    pub image_id: String,
}

/// Ficha normalizada no formato que `igdbArtDe`/`igdbTextosDe` consomem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgdbGame {
    pub name: String,
    pub summary: String,
    pub storyline: String,
    pub cover: Option<IgdbImage>,
    pub artworks: Vec<IgdbImage>,
    pub screenshots: Vec<IgdbImage>,
}

/// URL de imagem: `t_{tamanho}/{image_id}.jpg`.
pub fn igdb_img(image_id: &str, size: &str) -> String {
    format!("{IGDB_IMG}/t_{size}/{image_id}.jpg")
}

fn utc_year_from_unix(seconds: i64) -> i32 {
    // Algoritmo civil_from_days (Howard Hinnant), em UTC. O JS usa
    // `getFullYear()` no fuso local; a diferença só aparece em datas de
    // lançamento na virada do ano, e o ano é apenas um bônus de score.
    let days = seconds.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    if month <= 2 {
        year += 1;
    }
    year as i32
}

fn platform_matches(platform_name: &str, wanted: &str) -> bool {
    const ALIASES: &[(&str, &[&str])] = &[
        ("windows", &["pc", "windows", "microsoft windows"]),
        ("pc", &["pc", "windows", "microsoft windows"]),
        ("playstation", &["playstation", "ps4", "ps5", "sony"]),
        ("ps4", &["ps4", "playstation 4"]),
        ("ps5", &["ps5", "playstation 5"]),
        ("xbox", &["xbox", "microsoft"]),
        ("switch", &["switch", "nintendo switch"]),
        ("steam", &["pc", "windows", "steam"]),
        ("sony-playstation", &["playstation", "playstation 1"]),
        ("sony-playstation-2", &["playstation 2"]),
        ("sony-playstation-3", &["playstation 3"]),
        ("sony-psp", &["playstation portable", "psp"]),
        ("nintendo-gamecube", &["gamecube"]),
        ("nintendo-wii", &["wii"]),
        ("nintendo-ds", &["nintendo ds"]),
        ("nintendo-dsi", &["nintendo dsi"]),
        ("nintendo-nes", &["nintendo entertainment system", "nes"]),
        (
            "nintendo-snes",
            &["super nintendo entertainment system", "snes"],
        ),
        ("nintendo-game-boy", &["game boy"]),
        ("nintendo-game-boy-color", &["game boy color"]),
        ("nintendo-game-boy-advance", &["game boy advance"]),
        ("nintendo-64", &["nintendo 64"]),
    ];
    match ALIASES.iter().find(|(name, _)| *name == wanted) {
        Some((_, aliases)) => aliases.iter().any(|alias| platform_name.contains(alias)),
        None => platform_name.contains(wanted),
    }
}

/// `scoreIgdbMatch`: título + plataforma + ano, com as mesmas faixas do JS.
pub fn score_igdb_match(candidate: &IgdbCandidate, search: &IgdbSearch) -> i32 {
    let name = normalize_title(&candidate.name);
    let wanted = normalize_title(&search.title);
    let mut score = 0;

    if name == wanted {
        score += 100;
    } else if name.starts_with(&wanted) || wanted.starts_with(&name) {
        score += 50;
    } else if name.contains(&wanted) || wanted.contains(&name) {
        score += 20;
    }

    if let Some(platform) = search.platform.as_deref() {
        if !platform.is_empty() {
            let platform = platform.to_lowercase();
            if candidate
                .platforms
                .iter()
                .any(|candidate| platform_matches(&candidate.name.to_lowercase(), &platform))
            {
                score += 30;
            }
        }
    }

    if let (Some(year), Some(release_date)) = (search.year, candidate.first_release_date) {
        if year != 0 && release_date != 0 {
            let game_year = utc_year_from_unix(release_date);
            if game_year == year {
                score += 20;
            } else {
                let difference = (game_year - year).abs();
                if difference <= 1 {
                    score += 10;
                } else if difference > 2 {
                    score -= 10;
                }
            }
        }
    }

    // Jogos sem release date provavelmente ainda não foram lançados. No JS
    // `!0` também é verdadeiro, então data zero leva a mesma penalidade.
    if candidate.first_release_date.unwrap_or(0) == 0 {
        score -= 5;
    }

    score
}

/// `igdbNormaliza`: traduz os campos `*_expanded` do proxy.
pub fn normalize_igdb_game(value: &Value) -> IgdbGame {
    IgdbGame {
        name: value_str(value.get("name")),
        summary: value_str(value.get("summary")),
        storyline: value_str(value.get("storyline")),
        cover: value.get("cover_expanded").and_then(image_from_json),
        artworks: image_list(value.get("artworks_expanded")),
        screenshots: image_list(value.get("screenshots_expanded")),
    }
}

fn image_from_json(value: &Value) -> Option<IgdbImage> {
    value
        .get("image_id")
        .and_then(Value::as_str)
        .filter(|image_id| !image_id.is_empty())
        .map(|image_id| IgdbImage {
            image_id: image_id.to_string(),
        })
}

fn image_list(value: Option<&Value>) -> Vec<IgdbImage> {
    // Diferença deliberada de robustez: o JS chamaria `igdbImg(undefined)` e
    // geraria `.../undefined.jpg` para um artwork sem `image_id`. Aqui a
    // entrada inválida é descartada.
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(image_from_json).collect())
        .unwrap_or_default()
}

fn candidate_from_json(value: &Value) -> IgdbCandidate {
    IgdbCandidate {
        id: value_number(value.get("id")) as i64,
        name: value_str(value.get("name")),
        platforms: value
            .get("platforms")
            .and_then(Value::as_array)
            .map(|platforms| {
                platforms
                    .iter()
                    .map(|platform| IgdbPlatform {
                        name: value_str(platform.get("name")),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        first_release_date: value
            .get("first_release_date")
            .map(|date| value_number(Some(date)) as i64),
    }
}

/// Cliente IGDB com cache e circuit breaker por instância.
pub struct IgdbClient {
    fetcher: Arc<dyn Fetcher>,
    clock: Arc<dyn Clock>,
    base: String,
    cache: Mutex<LruCache<Vec<IgdbGame>>>,
    breaker: Mutex<CircuitBreaker>,
}

impl IgdbClient {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            fetcher,
            clock: Arc::new(SystemClock),
            base: IGDB_PROXY.to_string(),
            cache: Mutex::new(LruCache::new(IGDB_CACHE_SIZE, IGDB_CACHE_TTL_MS)),
            breaker: Mutex::new(CircuitBreaker::new(
                IGDB_BREAKER_THRESHOLD,
                IGDB_BREAKER_TIMEOUT_MS,
            )),
        }
    }

    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn cache_len(&self) -> usize {
        lock(&self.cache).len()
    }

    pub fn breaker_state(&self) -> BreakerState {
        lock(&self.breaker).state()
    }

    pub fn breaker_failures(&self) -> u32 {
        lock(&self.breaker).failures()
    }

    pub fn reset_breaker(&self) {
        lock(&self.breaker).reset();
    }

    /// `igdbProxy`: busca por título e devolve fichas normalizadas.
    ///
    /// Título vazio devolve `[]` sem tocar a rede. Resultados vazios e o
    /// "melhor score <= 0" são cacheados, como no cliente congelado.
    pub async fn search(&self, search: &IgdbSearch) -> Result<Vec<IgdbGame>> {
        let title = search.title.trim();
        if title.is_empty() {
            return Ok(Vec::new());
        }
        let mut normalized = search.clone();
        normalized.title = title.to_string();
        let key = normalized.cache_key();
        let now = self.clock.now_ms();

        if let Some(cached) = lock(&self.cache).get(&key, now) {
            return Ok(cached);
        }
        if !lock(&self.breaker).allow(now) {
            return Err(MediaError::message(
                "IGDB circuit breaker aberto (muitas falhas recentes)",
            ));
        }

        match self.fetch_games(&normalized).await {
            Ok(games) => {
                lock(&self.breaker).record_success();
                lock(&self.cache).set(&key, games.clone(), now);
                Ok(games)
            }
            Err(error) => {
                lock(&self.breaker).record_failure(now);
                Err(error)
            }
        }
    }

    async fn fetch_games(&self, search: &IgdbSearch) -> Result<Vec<IgdbGame>> {
        let base = self.base.trim_end_matches('/');
        let body = serde_json::to_vec(&json!({ "SearchTerm": search.title }))
            .map_err(|error| MediaError::decode(error.to_string()))?;
        let request = Request::post(format!("{base}/igdb/search"))
            .header("Content-Type", "application/json")
            .header("User-Agent", IGDB_UA)
            .body(body)
            .timeout(IGDB_TIMEOUT);
        let response = self.fetcher.fetch(request).await?;
        if !response.is_success() {
            return Err(MediaError::message(format!(
                "IGDB respondeu HTTP {}",
                response.status
            )));
        }
        let json = response.json_value()?;
        let candidates: Vec<IgdbCandidate> = json
            .get("data")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(candidate_from_json).collect())
            .unwrap_or_default();
        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let mut scored: Vec<(IgdbCandidate, i32)> = candidates
            .into_iter()
            .map(|candidate| {
                let score = score_igdb_match(&candidate, search);
                (candidate, score)
            })
            .collect();
        scored.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));
        let Some((best, score)) = scored.first() else {
            return Ok(Vec::new());
        };
        if *score <= 0 {
            return Ok(Vec::new());
        }

        let response = self
            .fetcher
            .fetch(
                Request::get(format!("{base}/igdb/game/{}", best.id))
                    .header("User-Agent", IGDB_UA)
                    .timeout(IGDB_TIMEOUT),
            )
            .await?;
        if !response.is_success() {
            return Err(MediaError::message(format!(
                "IGDB respondeu HTTP {}",
                response.status
            )));
        }
        let json = response.json_value()?;
        let game = json.get("data").filter(|data| value_truthy(Some(*data)));
        Ok(game
            .map(|game| vec![normalize_igdb_game(game)])
            .unwrap_or_default())
    }
}

/// `igdbArtDe`: capa em `cover`; artworks e screenshots viram hero; a IGDB
/// não tem logo transparente.
pub fn igdb_art_de(games: &[IgdbGame], kind: ArtKind) -> Vec<Artwork> {
    let mut artworks = Vec::new();
    for game in games {
        match kind {
            ArtKind::Cover => {
                if let Some(cover) = &game.cover {
                    artworks.push(igdb_candidate(&cover.image_id, "cover_big_2x", 528, 748));
                }
            }
            ArtKind::Hero => {
                for artwork in &game.artworks {
                    artworks.push(igdb_candidate(&artwork.image_id, "1080p_2x", 3840, 2160));
                }
                for screenshot in &game.screenshots {
                    artworks.push(igdb_candidate(&screenshot.image_id, "1080p_2x", 3840, 2160));
                }
            }
            ArtKind::Logo => {}
        }
    }
    artworks
}

fn igdb_candidate(image_id: &str, size: &str, width: u32, height: u32) -> Artwork {
    Artwork {
        fonte: "IGDB".to_string(),
        url: igdb_img(image_id, size),
        thumb: igdb_img(image_id, "screenshot_med"),
        largura: Some(width),
        altura: Some(height),
        animado: false,
        autor: "IGDB".to_string(),
    }
}

/// `igdbTextosDe`: resumo e enredo, limpos de HTML.
pub fn igdb_textos_de(games: &[IgdbGame]) -> Vec<TextCandidate> {
    let mut texts = Vec::new();
    for game in games {
        if !game.summary.is_empty() {
            texts.push(TextCandidate::new(
                format!("IGDB — {}", game.name),
                strip_html(&game.summary),
            ));
        }
        if !game.storyline.is_empty() {
            texts.push(TextCandidate::new(
                format!("IGDB (enredo) — {}", game.name),
                strip_html(&game.storyline),
            ));
        }
    }
    texts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::ManualClock;
    use crate::http::Response;
    use crate::test_support::{json_response, MockFetcher};

    fn candidate(name: &str, platforms: &[&str], release: Option<i64>) -> IgdbCandidate {
        IgdbCandidate {
            id: 1,
            name: name.to_string(),
            platforms: platforms
                .iter()
                .map(|name| IgdbPlatform {
                    name: (*name).to_string(),
                })
                .collect(),
            first_release_date: release,
        }
    }

    #[test]
    fn image_url_matches_igdb_token_format() {
        assert_eq!(
            igdb_img("abc123", "cover_big_2x"),
            "https://images.igdb.com/igdb/image/upload/t_cover_big_2x/abc123.jpg"
        );
    }

    #[test]
    fn score_title_exact_prefix_and_contains() {
        let search = IgdbSearch::new("Portal 2");
        assert_eq!(
            score_igdb_match(&candidate("Portal 2", &[], Some(1_302_931_200)), &search),
            100
        );
        assert_eq!(
            score_igdb_match(
                &candidate("Portal 2 Deluxe", &[], Some(1_302_931_200)),
                &search
            ),
            50
        );
        assert_eq!(
            score_igdb_match(
                &candidate("The Portal 2 Collection", &[], Some(1_302_931_200)),
                &search
            ),
            20
        );
    }

    #[test]
    fn score_platform_aliases_add_thirty() {
        let search = IgdbSearch::new("Portal 2").with_platform("windows");
        assert_eq!(
            score_igdb_match(
                &candidate("Portal 2", &["PC (Microsoft Windows)"], Some(1_302_931_200)),
                &search
            ),
            130
        );
        let search = IgdbSearch::new("Portal 2").with_platform("sony-playstation-2");
        assert_eq!(
            score_igdb_match(
                &candidate("Portal 2", &["PlayStation 2"], Some(1_302_931_200)),
                &search
            ),
            130
        );
        assert_eq!(
            score_igdb_match(
                &candidate("Portal 2", &["Xbox"], Some(1_302_931_200)),
                &search
            ),
            100
        );
    }

    #[test]
    fn score_year_windows_and_missing_date() {
        // 2011-04-19T00:00:00Z
        let release = Some(1_302_931_200);
        assert_eq!(
            score_igdb_match(
                &candidate("X", &[], release),
                &IgdbSearch::new("X").with_year(2011)
            ),
            120
        );
        assert_eq!(
            score_igdb_match(
                &candidate("X", &[], release),
                &IgdbSearch::new("X").with_year(2012)
            ),
            110
        );
        assert_eq!(
            score_igdb_match(
                &candidate("X", &[], release),
                &IgdbSearch::new("X").with_year(2013)
            ),
            100
        );
        assert_eq!(
            score_igdb_match(
                &candidate("X", &[], release),
                &IgdbSearch::new("X").with_year(2015)
            ),
            90
        );
        assert_eq!(
            score_igdb_match(
                &candidate("X", &[], None),
                &IgdbSearch::new("X").with_year(2011)
            ),
            95
        );
        assert_eq!(
            score_igdb_match(&candidate("X", &[], Some(0)), &IgdbSearch::new("X")),
            95
        );
    }

    #[test]
    fn normalize_extracts_expanded_fields() {
        let game = normalize_igdb_game(&json!({
            "name": "Portal 2",
            "summary": "<p>Resumo</p>",
            "storyline": "Enredo",
            "cover_expanded": {"image_id": "cover1"},
            "artworks_expanded": [{"image_id": "art1"}, {"nope": 1}],
            "screenshots_expanded": [{"image_id": "shot1"}]
        }));
        assert_eq!(game.cover.unwrap().image_id, "cover1");
        assert_eq!(game.artworks.len(), 1);
        assert_eq!(game.screenshots[0].image_id, "shot1");
        assert_eq!(game.summary, "<p>Resumo</p>");
    }

    #[test]
    fn art_and_texts_follow_the_shared_format() {
        let games = vec![IgdbGame {
            name: "Portal 2".into(),
            summary: "<b>Resumo</b>".into(),
            storyline: "Enredo".into(),
            cover: Some(IgdbImage {
                image_id: "c".into(),
            }),
            artworks: vec![IgdbImage {
                image_id: "a".into(),
            }],
            screenshots: vec![IgdbImage {
                image_id: "s".into(),
            }],
        }];
        let cover = igdb_art_de(&games, ArtKind::Cover);
        assert_eq!(cover.len(), 1);
        assert_eq!(cover[0].url, igdb_img("c", "cover_big_2x"));
        assert_eq!(cover[0].thumb, igdb_img("c", "screenshot_med"));
        assert_eq!(cover[0].largura, Some(528));
        assert_eq!(igdb_art_de(&games, ArtKind::Hero).len(), 2);
        assert!(igdb_art_de(&games, ArtKind::Logo).is_empty());
        assert_eq!(
            igdb_textos_de(&games),
            vec![
                TextCandidate::new("IGDB — Portal 2", "Resumo"),
                TextCandidate::new("IGDB (enredo) — Portal 2", "Enredo"),
            ]
        );
    }

    fn search_body() -> Value {
        json!({
            "data": [
                {"id": 1, "name": "Portal 2", "platforms": [{"name": "PC"}], "first_release_date": 1_302_931_200i64},
                {"id": 2, "name": "Portal 2 Deluxe", "platforms": [{"name": "PC"}], "first_release_date": 1_302_931_200i64}
            ]
        })
    }

    fn game_body() -> Value {
        json!({
            "data": {
                "name": "Portal 2",
                "summary": "Resumo",
                "storyline": "",
                "cover_expanded": {"image_id": "abc"},
                "artworks_expanded": [],
                "screenshots_expanded": []
            }
        })
    }

    #[tokio::test]
    async fn search_posts_exact_body_then_fetches_the_best_candidate() {
        let fetcher = MockFetcher::new(vec![
            json_response(search_body()),
            json_response(game_body()),
        ]);
        let clock = Arc::new(ManualClock::new(1_000));
        let client = IgdbClient::new(fetcher.clone()).with_clock(clock);
        let games = client
            .search(
                &IgdbSearch::new("Portal 2")
                    .with_platform("pc")
                    .with_year(2011),
            )
            .await
            .unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Portal 2");
        let requests = fetcher.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, crate::http::Method::Post);
        assert_eq!(
            requests[0].url,
            "https://api2.playnite.link/api/igdb/search"
        );
        assert_eq!(
            String::from_utf8(requests[0].body.clone().unwrap()).unwrap(),
            "{\"SearchTerm\":\"Portal 2\"}"
        );
        assert_eq!(
            requests[0].headers,
            vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("User-Agent".to_string(), IGDB_UA.to_string()),
            ]
        );
        assert_eq!(
            requests[1].url,
            "https://api2.playnite.link/api/igdb/game/1"
        );
        assert_eq!(
            requests[1]
                .headers
                .iter()
                .find(|(name, _)| name == "User-Agent")
                .map(|(_, value)| value.as_str()),
            Some(IGDB_UA)
        );
    }

    #[tokio::test]
    async fn empty_or_low_score_searches_are_cached_without_detail_request() {
        let fetcher = MockFetcher::new(vec![
            json_response(json!({"data": []})),
            json_response(json!({"data": [{"id": 1, "name": "Completely Different"}]})),
        ]);
        let client = IgdbClient::new(fetcher.clone());
        assert!(client
            .search(&IgdbSearch::new("Portal 2"))
            .await
            .unwrap()
            .is_empty());
        // Título diferente para não bater no cache do caso vazio acima.
        assert!(client
            .search(&IgdbSearch::new("Portal 3"))
            .await
            .unwrap()
            .is_empty());
        assert_eq!(fetcher.requests().len(), 2);
        assert_eq!(client.cache_len(), 2);
    }

    #[tokio::test]
    async fn equal_scores_keep_the_first_candidate_from_the_fuzzy_search() {
        let fetcher = MockFetcher::new(vec![
            json_response(json!({
                "data": [
                    {"id": 7, "name": "Portal 2"},
                    {"id": 8, "name": "Portal 2"}
                ]
            })),
            json_response(game_body()),
        ]);
        let client = IgdbClient::new(fetcher.clone());
        client.search(&IgdbSearch::new("Portal 2")).await.unwrap();
        assert_eq!(
            fetcher.requests()[1].url,
            "https://api2.playnite.link/api/igdb/game/7"
        );
    }

    #[tokio::test]
    async fn empty_title_short_circuits() {
        let fetcher = MockFetcher::new(vec![]);
        let client = IgdbClient::new(fetcher.clone());
        assert!(client
            .search(&IgdbSearch::new("   "))
            .await
            .unwrap()
            .is_empty());
        assert!(fetcher.requests().is_empty());
    }

    #[tokio::test]
    async fn cache_ttl_uses_one_hour_and_strictly_greater_boundary() {
        let fetcher = MockFetcher::new(vec![
            json_response(search_body()),
            json_response(game_body()),
            json_response(search_body()),
            json_response(game_body()),
        ]);
        let clock = Arc::new(ManualClock::new(0));
        let client = IgdbClient::new(fetcher.clone()).with_clock(clock.clone());
        assert_eq!(
            client
                .search(&IgdbSearch::new("Portal 2"))
                .await
                .unwrap()
                .len(),
            1
        );
        clock.set(3_600_000);
        assert_eq!(
            client
                .search(&IgdbSearch::new("Portal 2"))
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(fetcher.requests().len(), 2);
        clock.set(3_600_001);
        assert_eq!(
            client
                .search(&IgdbSearch::new("Portal 2"))
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(fetcher.requests().len(), 4);
    }

    #[tokio::test]
    async fn breaker_opens_after_five_failures_and_recovers_half_open() {
        let mut responses = vec![Response::new(500, "erro"); 5];
        responses.push(json_response(json!({"data": []})));
        let fetcher = MockFetcher::new(responses);
        let clock = Arc::new(ManualClock::new(0));
        let client = IgdbClient::new(fetcher.clone()).with_clock(clock.clone());
        for index in 0..5 {
            let error = client
                .search(&IgdbSearch::new(format!("jogo {index}")))
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), "IGDB respondeu HTTP 500");
        }
        assert_eq!(client.breaker_state(), BreakerState::Open);
        let error = client
            .search(&IgdbSearch::new("bloqueado"))
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "IGDB circuit breaker aberto (muitas falhas recentes)"
        );
        assert_eq!(fetcher.requests().len(), 5);

        clock.set(60_000);
        assert!(client
            .search(&IgdbSearch::new("recuperando"))
            .await
            .unwrap()
            .is_empty());
        assert_eq!(client.breaker_state(), BreakerState::Closed);
        assert_eq!(client.breaker_failures(), 0);
        assert_eq!(fetcher.requests().len(), 6);
    }

    #[tokio::test]
    async fn detail_failure_is_propagated_not_cached() {
        let fetcher = MockFetcher::new(vec![
            json_response(search_body()),
            Response::new(503, ""),
            json_response(search_body()),
            json_response(game_body()),
        ]);
        let client = IgdbClient::new(fetcher.clone());
        let error = client
            .search(&IgdbSearch::new("Portal 2"))
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "IGDB respondeu HTTP 503");
        assert_eq!(client.cache_len(), 0);
        assert_eq!(
            client
                .search(&IgdbSearch::new("Portal 2"))
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn invalid_json_is_a_decode_error() {
        let fetcher = MockFetcher::new(vec![Response::text(200, "not json")]);
        let client = IgdbClient::new(fetcher);
        assert!(client
            .search(&IgdbSearch::new("x"))
            .await
            .unwrap_err()
            .to_string()
            .contains("resposta inválida"));
    }
}
