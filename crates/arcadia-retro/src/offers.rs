//! Ofertas de download — port 1:1 de `app/electron/retro-offers.js`.
//!
//! O fingerprint é estável entre reordenações do feed Hydra; a normalização
//! resolve sistema, seriais, região, idiomas e tipo de release. As funções
//! puras aceitam `serde_json::Value` (formato dos downloads) e devolvem
//! [`Offer`] com campos desconhecidos preservados em `extra`.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::matcher::extract_all_serials;
use crate::systems::resolve_system;
use crate::title::{js_trim, normalize_retro_title};
use crate::util::{field_or_string, js_number, js_or_string, js_string, js_truthy, sha256_hex};

/// Referência mínima a uma fonte Hydra (o que as ofertas realmente usam).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OfferSource {
    pub id: String,
    pub title: String,
}

impl OfferSource {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}

/// Bloco `match` de uma oferta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OfferMatch {
    pub method: String,
    /// Score numérico (o JS usa inteiros na prática).
    pub confidence: i64,
    pub catalog_version: u64,
    /// `null` quando a oferta ainda não foi associada.
    pub game_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for OfferMatch {
    fn default() -> Self {
        Self {
            method: "none".to_string(),
            confidence: 0,
            catalog_version: 2,
            game_id: None,
            evidence: None,
            quality: None,
            extra: Map::new(),
        }
    }
}

/// Oferta normalizada, no mesmo formato de `normalizeOffer`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Offer {
    pub id: String,
    pub source_id: String,
    pub source_title: String,
    pub original_title: String,
    pub normalized_title: String,
    /// `null` quando a plataforma não foi reconhecida.
    pub system_id: Option<String>,
    pub platform_raw: String,
    pub serials: Vec<String>,
    pub region: Option<String>,
    pub languages: Vec<String>,
    pub release_kind: String,
    pub file_size: Option<Value>,
    pub upload_date: Option<Value>,
    pub description: Option<Value>,
    pub uris: Vec<String>,
    pub cover: Option<Value>,
    #[serde(rename = "match")]
    pub r#match: OfferMatch,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Offer {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

static RE_BIOS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bbios\b").expect("regex bios"));
static RE_COLLECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(\d+[\s-]?in[\s-]?1|collection|pack|bundle|anthology|compilation)\b")
        .expect("regex coleção")
});
static RE_HACK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(hack|mod|patch|romhack|rom\s*hack)\b").expect("regex hack")
});
static RE_TRANSLATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(translation|translated|fan[\s-]?translation|[+]\s*(?:eng|rus|pt|es|fr))\b")
        .expect("regex tradução")
});
static RE_HOMEBREW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(homebrew|indie|fangame|fan[\s-]?game)\b").expect("regex homebrew")
});
static RE_DLC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(dlc|downloadable[\s-]?content|expansion|add[\s-]?on)\b").expect("regex dlc")
});
static RE_UPDATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(update|patch|hotfix|firmware)\b").expect("regex update"));
static RE_VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bv?\d+\.\d+").expect("regex versão"));

static RE_REGION_USA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(usa|ntsc[\s-]?u|us)\b").expect("regex região USA"));
static RE_REGION_EUR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(europe|pal|eu|eur)\b").expect("regex região EUR"));
static RE_REGION_JPN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(japan|ntsc[\s-]?j|jp|jpn)\b").expect("regex região JPN"));
static RE_REGION_ASIA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(asia|ntsc[\s-]?k|kor|korea)\b").expect("regex região ASIA")
});
static RE_REGION_BRA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(brazil|br|brasil)\b").expect("regex região BRA"));
static RE_REGION_WORLD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(world|ww|global)\b").expect("regex região WORLD"));

static RE_LANG_EN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(eng|english|en)\b").expect("regex idioma en"));
static RE_LANG_RU: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(rus|russian|ru)\b").expect("regex idioma ru"));
static RE_LANG_PT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(por|portuguese|pt|ptbr|pt-br)\b").expect("regex idioma pt")
});
static RE_LANG_ES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(spa|spanish|es)\b").expect("regex idioma es"));
static RE_LANG_FR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(fra|french|fr)\b").expect("regex idioma fr"));
static RE_LANG_DE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(ger|german|de)\b").expect("regex idioma de"));
static RE_LANG_IT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(ita|italian|it)\b").expect("regex idioma it"));
static RE_LANG_JA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(jpn|japanese|ja)\b").expect("regex idioma ja"));
static RE_LANG_MULTI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(multi\d*)\b").expect("regex idioma multi"));

/// Fingerprint estável de 24 caracteres (sha256 de source + título + URIs).
pub fn generate_offer_fingerprint(
    source_id: &str,
    original_title: &str,
    uris: &[String],
) -> String {
    let mut normalized: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for uri in uris {
        if uri.is_empty() || !seen.insert(uri.clone()) {
            continue;
        }
        normalized.push(js_trim(uri).to_string());
    }
    normalized.sort();
    let normalized_uris = normalized.join("\u{0}");
    let content = [
        js_trim(source_id),
        js_trim(original_title),
        normalized_uris.as_str(),
    ]
    .join("\u{0}");
    sha256_hex(&content).chars().take(24).collect()
}

/// Classifica o tipo de release (jogo, BIOS, coleção, hack...).
pub fn classify_release_kind(title: &str, original_title: &str, description: &str) -> String {
    let text = format!("{original_title} {title} {description}").to_lowercase();
    if RE_BIOS.is_match(&text) {
        return "bios".to_string();
    }
    if RE_COLLECTION.is_match(&text) {
        return "collection".to_string();
    }
    if RE_HACK.is_match(&text) {
        return "hack".to_string();
    }
    if RE_TRANSLATION.is_match(&text) {
        return "translation".to_string();
    }
    if RE_HOMEBREW.is_match(&text) {
        return "homebrew".to_string();
    }
    if RE_DLC.is_match(&text) {
        return "dlc".to_string();
    }
    if RE_UPDATE.is_match(&text) && RE_VERSION.is_match(&text) {
        return "update".to_string();
    }
    "game".to_string()
}

/// Extrai a região (primeiro padrão vencedor).
pub fn extract_region(text: &str) -> Option<String> {
    let patterns: [(&LazyLock<Regex>, &str); 6] = [
        (&RE_REGION_USA, "USA"),
        (&RE_REGION_EUR, "EUR"),
        (&RE_REGION_JPN, "JPN"),
        (&RE_REGION_ASIA, "ASIA"),
        (&RE_REGION_BRA, "BRA"),
        (&RE_REGION_WORLD, "WORLD"),
    ];
    for (pattern, region) in patterns {
        if pattern.is_match(text) {
            return Some(region.to_string());
        }
    }
    None
}

/// Extrai idiomas na ordem dos padrões, sem duplicatas.
pub fn extract_languages(text: &str) -> Vec<String> {
    let patterns: [(&LazyLock<Regex>, &str); 9] = [
        (&RE_LANG_EN, "en"),
        (&RE_LANG_RU, "ru"),
        (&RE_LANG_PT, "pt"),
        (&RE_LANG_ES, "es"),
        (&RE_LANG_FR, "fr"),
        (&RE_LANG_DE, "de"),
        (&RE_LANG_IT, "it"),
        (&RE_LANG_JA, "ja"),
        (&RE_LANG_MULTI, "multi"),
    ];
    let mut languages = Vec::new();
    for (pattern, code) in patterns {
        if pattern.is_match(text) && !languages.iter().any(|value| value == code) {
            languages.push(code.to_string());
        }
    }
    languages
}

fn download_uris(download: &Value) -> Vec<String> {
    match download.get("uris") {
        Some(Value::Array(items)) => items
            .iter()
            .filter(|item| js_truthy(Some(item)))
            .map(js_string)
            .collect(),
        _ => match download.get("uri") {
            Some(uri) if js_truthy(Some(uri)) => vec![js_string(uri)],
            _ => Vec::new(),
        },
    }
}

/// Normaliza um download Hydra para o formato interno de oferta.
pub fn normalize_offer(download: &Value, source: &OfferSource) -> Offer {
    let original_title = js_trim(&field_or_string(download, "title")).to_string();
    let normalized_title = normalize_retro_title(&original_title);
    let uris = download_uris(download);

    let id = generate_offer_fingerprint(&source.id, &original_title, &uris);
    let platform_raw = js_trim(&field_or_string(download, "platform")).to_lowercase();
    let system_id = resolve_system(&platform_raw).map(ToString::to_string);

    let description_raw = download.get("description");
    let description_text = js_or_string(description_raw);
    let text = format!("{original_title} {description_text}");
    let release_kind = classify_release_kind(&normalized_title, &original_title, &description_text);
    let region = extract_region(&text);
    let languages = extract_languages(&text);
    let serials = match system_id.as_deref() {
        Some(system) => extract_all_serials(&text, system),
        None => Vec::new(),
    };

    Offer {
        id,
        source_id: source.id.clone(),
        source_title: source.title.clone(),
        original_title,
        normalized_title,
        system_id,
        platform_raw,
        serials,
        region,
        languages,
        release_kind,
        file_size: download
            .get("fileSize")
            .filter(|value| js_truthy(Some(value)))
            .cloned(),
        upload_date: download
            .get("uploadDate")
            .filter(|value| js_truthy(Some(value)))
            .cloned(),
        description: description_raw
            .filter(|value| js_truthy(Some(value)))
            .cloned(),
        uris,
        cover: download
            .get("cover")
            .filter(|value| js_truthy(Some(value)))
            .or_else(|| download.get("capa").filter(|value| js_truthy(Some(value))))
            .cloned(),
        r#match: OfferMatch::default(),
        extra: Map::new(),
    }
}

/// Processa e deduplica (por fingerprint) os downloads de uma fonte.
pub fn process_source_downloads(downloads: &[Value], source: &OfferSource) -> Vec<Offer> {
    let mut offers = Vec::new();
    let mut seen = HashSet::new();
    for download in downloads {
        if !download.is_object() {
            continue;
        }
        let offer = normalize_offer(download, source);
        if !seen.insert(offer.id.clone()) {
            continue;
        }
        offers.push(offer);
    }
    offers
}

/// Atualiza o bloco `match` a partir do resultado do matcher.
pub fn update_offer_match(offer: &Offer, match_result: &Value) -> Offer {
    let method = {
        let method = field_or_string(match_result, "method");
        if method.is_empty() {
            "none".to_string()
        } else {
            method
        }
    };
    let score = js_number(match_result.get("score"));
    let confidence = if score.is_finite() { score as i64 } else { 0 };
    let game_id = field_string_result(match_result, "gameId");
    let evidence = field_string_result(match_result, "evidence");
    let quality =
        field_string_result(match_result, "quality").unwrap_or_else(|| "unmatched".to_string());

    let mut updated = offer.clone();
    updated.r#match = OfferMatch {
        method,
        confidence,
        catalog_version: 2,
        game_id,
        evidence,
        quality: Some(quality),
        extra: Map::new(),
    };
    updated
}

fn field_string_result(value: &Value, key: &str) -> Option<String> {
    let field = value.get(key);
    if js_truthy(field) {
        field.map(js_string)
    } else {
        None
    }
}

/// Filtra ofertas por `releaseKind`.
pub fn filter_by_release_kind(offers: &[Offer], kinds: &[&str]) -> Vec<Offer> {
    offers
        .iter()
        .filter(|offer| kinds.iter().any(|kind| *kind == offer.release_kind))
        .cloned()
        .collect()
}

/// Agrupa ofertas associadas por `gameId`, preservando a ordem de entrada.
pub fn group_offers_by_game(offers: &[Offer]) -> Vec<(String, Vec<Offer>)> {
    let mut order: Vec<(String, Vec<Offer>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for offer in offers {
        let Some(game_id) = offer.r#match.game_id.as_deref().filter(|id| !id.is_empty()) else {
            continue;
        };
        let position = *index.entry(game_id.to_string()).or_insert_with(|| {
            order.push((game_id.to_string(), Vec::new()));
            order.len() - 1
        });
        order[position].1.push(offer.clone());
    }
    order
}

/// Sumário sem URIs, com `hasUris`/`uriCount` (detalhe do jogo).
pub fn create_offer_summary(offer: &Offer) -> Value {
    let mut summary = offer.to_value();
    if let Value::Object(object) = &mut summary {
        object.remove("uris");
        object.insert("hasUris".to_string(), Value::Bool(!offer.uris.is_empty()));
        object.insert("uriCount".to_string(), json!(offer.uris.len()));
    }
    summary
}

/// Serializa ofertas para o cache (`version: 2`).
pub fn serialize_offers(offers: &[Offer], metadata: &Map<String, Value>, updated_at: i64) -> Value {
    let mut object = Map::new();
    object.insert("version".to_string(), json!(2));
    object.insert("updatedAt".to_string(), json!(updated_at));
    for (key, value) in metadata {
        object.insert(key.clone(), value.clone());
    }
    let serialized: Vec<Value> = offers
        .iter()
        .map(|offer| {
            let mut value = offer.to_value();
            if let Value::Object(entry) = &mut value {
                entry
                    .entry("uris".to_string())
                    .or_insert_with(|| Value::Array(Vec::new()));
                entry
                    .entry("serials".to_string())
                    .or_insert_with(|| Value::Array(Vec::new()));
                entry
                    .entry("languages".to_string())
                    .or_insert_with(|| Value::Array(Vec::new()));
            }
            value
        })
        .collect();
    object.insert("offers".to_string(), Value::Array(serialized));
    Value::Object(object)
}

/// Desserializa o cache de ofertas; `None` quando a versão/shape é inválida.
pub fn deserialize_offers(cached: &Value) -> Option<Vec<Offer>> {
    let object = cached.as_object()?;
    if object.get("version").and_then(Value::as_u64) != Some(2) {
        return None;
    }
    let offers = object.get("offers")?.as_array()?;
    let mut output = Vec::with_capacity(offers.len());
    for raw in offers {
        let Some(mut entry) = raw.as_object().cloned() else {
            continue;
        };
        for key in ["uris", "serials", "languages"] {
            if entry.get(key).is_some_and(Value::is_null) {
                entry.insert(key.to_string(), Value::Array(Vec::new()));
            }
        }
        if entry.get("match").is_none_or(Value::is_null) {
            entry.insert(
                "match".to_string(),
                serde_json::to_value(OfferMatch::default()).unwrap_or(Value::Null),
            );
        }
        if entry.get("systemId").is_none() {
            entry.insert("systemId".to_string(), Value::Null);
        }
        let offer: Offer = serde_json::from_value(Value::Object(entry)).ok()?;
        output.push(offer);
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> OfferSource {
        OfferSource::new("classic", "Classics")
    }

    #[test]
    fn fingerprint_ignora_ordem_e_duplicatas_de_uris() {
        let a = generate_offer_fingerprint(
            "source",
            "Game",
            &[
                " magnet:?xt=urn:btih:A ".to_string(),
                "magnet:?xt=urn:btih:B".to_string(),
            ],
        );
        let b = generate_offer_fingerprint(
            "source",
            "Game",
            &[
                "magnet:?xt=urn:btih:B".to_string(),
                "magnet:?xt=urn:btih:A".to_string(),
                "magnet:?xt=urn:btih:A".to_string(),
            ],
        );
        assert_eq!(a, b);
        assert_eq!(a.len(), 24);
        assert_ne!(a, generate_offer_fingerprint("source", "Other", &[]));
    }

    #[test]
    fn classifica_tipos_de_release_como_o_js() {
        assert_eq!(classify_release_kind("Game", "Game", ""), "game");
        assert_eq!(classify_release_kind("BIOS", "BIOS", ""), "bios");
        assert_eq!(
            classify_release_kind("100 in 1", "100 in 1", ""),
            "collection"
        );
        assert_eq!(classify_release_kind("Game", "Game Hack", ""), "hack");
        assert_eq!(
            classify_release_kind("Game", "Game", "fan translation"),
            "translation"
        );
        assert_eq!(
            classify_release_kind("Game", "Game", "homebrew"),
            "homebrew"
        );
        assert_eq!(classify_release_kind("Game", "Game", "DLC add-on"), "dlc");
        assert_eq!(
            classify_release_kind("Game", "Game update v1.2", ""),
            "update"
        );
    }

    #[test]
    fn extrai_regiao_e_idiomas() {
        assert_eq!(extract_region("Game (USA)"), Some("USA".to_string()));
        assert_eq!(extract_region("Game (Europe)"), Some("EUR".to_string()));
        assert_eq!(extract_region("Game (Japan)"), Some("JPN".to_string()));
        assert_eq!(extract_region("Game (Brazil)"), Some("BRA".to_string()));
        assert_eq!(extract_region("Game"), None);
        assert_eq!(
            extract_languages("Game ENG/RUS/Multi5"),
            vec!["en".to_string(), "ru".to_string(), "multi".to_string()]
        );
    }

    #[test]
    fn normaliza_oferta_resolvendo_sistema_e_seriais() {
        let download = json!({
            "title": "Final Fantasy VII [USA]",
            "platform": "PS1",
            "description": "ENG",
            "fileSize": "700 MB",
            "uploadDate": "2024-01-01",
            "uris": ["magnet:?xt=urn:btih:A"],
            "cover": "cover.jpg"
        });
        let offer = normalize_offer(&download, &source());
        assert_eq!(offer.original_title, "Final Fantasy VII [USA]");
        assert_eq!(offer.normalized_title, "Final Fantasy 7");
        assert_eq!(offer.system_id.as_deref(), Some("sony-playstation"));
        assert_eq!(offer.platform_raw, "ps1");
        assert_eq!(offer.region.as_deref(), Some("USA"));
        assert_eq!(offer.languages, vec!["en".to_string()]);
        assert_eq!(offer.release_kind, "game");
        assert_eq!(offer.r#match.method, "none");
        assert_eq!(offer.r#match.catalog_version, 2);
        assert!(offer.r#match.game_id.is_none());
    }

    #[test]
    fn processa_e_deduplica_por_fingerprint() {
        let downloads = vec![
            json!({"title": "Game", "uris": ["magnet:?xt=urn:btih:A"]}),
            json!({"title": "Game", "uris": ["magnet:?xt=urn:btih:A"]}),
            json!({"title": "Other", "uris": ["magnet:?xt=urn:btih:B"]}),
            json!(null),
        ];
        let offers = process_source_downloads(&downloads, &source());
        assert_eq!(offers.len(), 2);
    }

    #[test]
    fn atualiza_match_com_quality_e_evidence() {
        let offer = normalize_offer(
            &json!({"title": "Game", "platform": "nes", "uris": ["magnet:?xt=urn:btih:A"]}),
            &source(),
        );
        let updated = update_offer_match(
            &offer,
            &json!({"method": "serial", "score": 100, "gameId": "game1", "evidence": "SLUS", "quality": "exact"}),
        );
        assert_eq!(updated.r#match.method, "serial");
        assert_eq!(updated.r#match.confidence, 100);
        assert_eq!(updated.r#match.game_id.as_deref(), Some("game1"));
        assert_eq!(updated.r#match.quality.as_deref(), Some("exact"));
    }

    #[test]
    fn filtra_agrupa_e_resume() {
        let mut first = normalize_offer(
            &json!({"title": "A", "platform": "nes", "uris": ["magnet:?xt=urn:btih:A"]}),
            &source(),
        );
        first.r#match.game_id = Some("g1".to_string());
        let mut second = normalize_offer(
            &json!({"title": "B", "platform": "nes", "description": "BIOS", "uris": ["magnet:?xt=urn:btih:B"]}),
            &source(),
        );
        second.r#match.game_id = Some("g1".to_string());
        let offers = vec![first, second];
        assert_eq!(filter_by_release_kind(&offers, &["game"]).len(), 1);
        let grouped = group_offers_by_game(&offers);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].1.len(), 2);
        let summary = create_offer_summary(&offers[0]);
        assert!(summary.get("uris").is_none());
        assert_eq!(summary.get("hasUris"), Some(&json!(true)));
        assert_eq!(summary.get("uriCount"), Some(&json!(1)));
    }

    #[test]
    fn serializa_e_desserializa_cache_versao_2() {
        let offer = normalize_offer(
            &json!({"title": "Game", "platform": "nes", "uris": ["magnet:?xt=urn:btih:A"]}),
            &source(),
        );
        let cached = serialize_offers(std::slice::from_ref(&offer), &Map::new(), 123);
        assert_eq!(cached.get("version"), Some(&json!(2)));
        assert_eq!(cached.get("updatedAt"), Some(&json!(123)));
        let decoded = deserialize_offers(&cached).expect("cache válido");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].id, offer.id);
        assert!(deserialize_offers(&json!({"version": 1, "offers": []})).is_none());

        let mut nulled = cached.clone();
        nulled["offers"][0]["uris"] = Value::Null;
        nulled["offers"][0]["serials"] = Value::Null;
        nulled["offers"][0]["languages"] = Value::Null;
        let decoded = deserialize_offers(&nulled).expect("cache tolerante");
        assert!(decoded[0].uris.is_empty());
    }

    #[test]
    fn preserva_campos_desconhecidos_da_oferta() {
        let offer = normalize_offer(
            &json!({"title": "Game", "platform": "nes", "uris": ["magnet:?xt=urn:btih:A"]}),
            &source(),
        );
        let mut value = offer.to_value();
        value["customFlag"] = json!("kept");
        let decoded: Offer = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.extra.get("customFlag"), Some(&json!("kept")));
    }
}
