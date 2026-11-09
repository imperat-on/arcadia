//! Motor de matching entre ofertas (downloads) e jogos canônicos.
//!
//! Port 1:1 de `app/electron/retro-matcher.js`. As funções públicas operam
//! sobre structs (`RetroOffer`/`RetroGame`) e há wrappers `*_json` para o
//! formato que o cliente Electron consome (campos camelCase preservados).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use regex::Regex;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use unicode_normalization::UnicodeNormalization;

use crate::systems::{extract_serial, normalize_serial};
use crate::title::{js_slice_utf16, js_trim};

/// Score >= 80 associa automaticamente.
pub const CONFIDENCE_THRESHOLD_AUTO: i32 = 80;
/// 65..=79 fica "probable", sujeito a auditoria.
pub const CONFIDENCE_THRESHOLD_PROBABLE: i32 = 65;

pub const SCORE_SERIAL_EXACT: i32 = 100;
pub const SCORE_HASH_EXACT: i32 = 100;
pub const SCORE_TITLE_CANONICAL_EXACT: i32 = 85;
pub const SCORE_ALIAS_EXACT: i32 = 80;
pub const SCORE_TITLE_STRONG: i32 = 80;
pub const SCORE_TITLE_FUZZY_MAX: i32 = 94;
pub const SCORE_REGION_BONUS: i32 = 10;
pub const SCORE_YEAR_BONUS: i32 = 5;

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "of", "and", "or", "in", "on", "with", "for", "to", "vs",
];

const NUMBER_WORDS: &[(&str, &str)] = &[
    ("zero", "0"),
    ("one", "1"),
    ("two", "2"),
    ("three", "3"),
    ("four", "4"),
    ("five", "5"),
    ("six", "6"),
    ("seven", "7"),
    ("eight", "8"),
    ("nine", "9"),
    ("ten", "10"),
];

const ROMAN_NUMERALS: &[(&str, &str)] = &[
    ("i", "1"),
    ("ii", "2"),
    ("iii", "3"),
    ("iv", "4"),
    ("v", "5"),
    ("vi", "6"),
    ("vii", "7"),
    ("viii", "8"),
    ("ix", "9"),
    ("x", "10"),
    ("xi", "11"),
    ("xii", "12"),
    ("xiii", "13"),
];

static RE_MATCH_NON_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_\s]").expect("regex pontuação"));
static RE_MATCH_WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("regex espaços"));
static RE_ASCII_DIGITS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+$").expect("regex dígitos"));

/// Tabela de hashes.
///
/// O JS aceita string única ou array. `calculateMatchScore` trata string como
/// singleton, mas `matchBatch` itera o valor cru (`for...of`), ou seja, uma
/// string vira seus caracteres. Guardamos `scalar_types` para reproduzir as
/// duas semânticas e serializar de volta no mesmo formato de entrada.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hashes {
    pub values: BTreeMap<String, Vec<String>>,
    pub scalar_types: BTreeSet<String>,
}

impl Hashes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Constrói a partir de listas (todas tratadas como array).
    pub fn from_values(values: BTreeMap<String, Vec<String>>) -> Self {
        Self {
            values,
            scalar_types: BTreeSet::new(),
        }
    }

    pub fn insert(&mut self, hash_type: impl Into<String>, values: Vec<String>) {
        let hash_type = hash_type.into();
        self.scalar_types.remove(&hash_type);
        self.values.insert(hash_type, values);
    }

    /// Insere um valor escalar (string única no JSON).
    pub fn insert_scalar(&mut self, hash_type: impl Into<String>, value: impl Into<String>) {
        let hash_type = hash_type.into();
        self.values.insert(hash_type.clone(), vec![value.into()]);
        self.scalar_types.insert(hash_type);
    }

    pub fn get(&self, hash_type: &str) -> Option<&Vec<String>> {
        self.values.get(hash_type)
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Valores como o `for...of` do JS os enxerga.
    pub fn iterable_values(&self, hash_type: &str) -> Vec<String> {
        let Some(values) = self.values.get(hash_type) else {
            return Vec::new();
        };
        if self.scalar_types.contains(hash_type) {
            values
                .first()
                .map(|value| {
                    value
                        .chars()
                        .map(|character| character.to_string())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            values.clone()
        }
    }
}

impl From<BTreeMap<String, Vec<String>>> for Hashes {
    fn from(values: BTreeMap<String, Vec<String>>) -> Self {
        Self::from_values(values)
    }
}

impl Serialize for Hashes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.values.len()))?;
        for (hash_type, values) in &self.values {
            if self.scalar_types.contains(hash_type) && values.len() == 1 {
                map.serialize_entry(hash_type, &values[0])?;
            } else {
                map.serialize_entry(hash_type, values)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Hashes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let Value::Object(raw) = value else {
            return Ok(Hashes::default());
        };
        let mut values = BTreeMap::new();
        let mut scalar_types = BTreeSet::new();
        for (hash_type, value) in raw {
            let mut entries = Vec::new();
            match value {
                Value::Null => {}
                Value::String(value) => {
                    entries.push(value);
                    scalar_types.insert(hash_type.clone());
                }
                Value::Array(items) => {
                    for item in items {
                        if let Some(value) = stringish(&item) {
                            entries.push(value);
                        }
                    }
                }
                other => {
                    if let Some(value) = stringish(&other) {
                        entries.push(value);
                        scalar_types.insert(hash_type.clone());
                    }
                }
            }
            values.insert(hash_type, entries);
        }
        Ok(Hashes {
            values,
            scalar_types,
        })
    }
}

fn stringish(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Array(_) => Some("[object Array]".to_string()),
        Value::Object(_) => Some("[object Object]".to_string()),
    }
}

fn deserialize_stringish<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(stringish(&value).unwrap_or_default())
}

fn deserialize_opt_stringish<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(stringish(&value))
}

fn deserialize_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let mut values = Vec::new();
    match value {
        Value::Null => {}
        Value::Array(items) => {
            for item in items {
                if let Some(value) = stringish(&item) {
                    values.push(value);
                }
            }
        }
        other => {
            if let Some(value) = stringish(&other) {
                values.push(value);
            }
        }
    }
    Ok(values)
}

fn deserialize_opt_year<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let number = match value {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|value| value.fract() == 0.0)
                .map(|value| value as i64)
        }),
        _ => None,
    };
    Ok(number)
}

/// Oferta de download no formato consumido pelo matcher.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RetroOffer {
    #[serde(deserialize_with = "deserialize_stringish")]
    pub id: String,
    #[serde(deserialize_with = "deserialize_opt_stringish")]
    pub normalized_title: Option<String>,
    #[serde(deserialize_with = "deserialize_opt_stringish")]
    pub title: Option<String>,
    #[serde(deserialize_with = "deserialize_opt_stringish")]
    pub system_id: Option<String>,
    #[serde(deserialize_with = "deserialize_string_list")]
    pub serials: Vec<String>,
    pub hashes: Hashes,
    #[serde(deserialize_with = "deserialize_opt_stringish")]
    pub region: Option<String>,
    #[serde(deserialize_with = "deserialize_opt_year")]
    pub release_year: Option<i64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl RetroOffer {
    pub fn from_json(value: &Value) -> Self {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }

    pub fn title_for_match(&self) -> &str {
        self.normalized_title
            .as_deref()
            .filter(|value| !value.is_empty())
            .or(self.title.as_deref())
            .unwrap_or("")
    }
}

/// Jogo canônico no formato consumido pelo matcher.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RetroGame {
    #[serde(deserialize_with = "deserialize_stringish")]
    pub id: String,
    #[serde(deserialize_with = "deserialize_opt_stringish")]
    pub system_id: Option<String>,
    #[serde(deserialize_with = "deserialize_stringish")]
    pub title: String,
    #[serde(deserialize_with = "deserialize_string_list")]
    pub aliases: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_list")]
    pub serials: Vec<String>,
    pub hashes: Hashes,
    #[serde(deserialize_with = "deserialize_string_list")]
    pub regions: Vec<String>,
    #[serde(deserialize_with = "deserialize_opt_stringish")]
    pub release_date: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl RetroGame {
    pub fn from_json(value: &Value) -> Self {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }
}

/// Resultado de `calculateMatchScore`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchScore {
    pub score: i32,
    pub method: String,
    pub evidence: String,
}

/// Faixa de confiança usada pela UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchQuality {
    Exact,
    Strong,
    Probable,
    Unmatched,
}

impl MatchQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Strong => "strong",
            Self::Probable => "probable",
            Self::Unmatched => "unmatched",
        }
    }
}

/// Opções de `findBestMatch`/`matchBatch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MatchOptions {
    pub min_confidence: i32,
    pub allow_probable: bool,
}

impl Default for MatchOptions {
    fn default() -> Self {
        Self {
            min_confidence: CONFIDENCE_THRESHOLD_PROBABLE,
            allow_probable: false,
        }
    }
}

/// Melhor match devolvido por `findBestMatch`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BestMatch {
    pub game_id: String,
    pub game: RetroGame,
    pub score: i32,
    pub confidence: i32,
    pub method: String,
    pub evidence: String,
    pub quality: MatchQuality,
    pub tied: bool,
}

/// Match consolidado de um lote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMatch {
    pub offer_id: String,
    pub game_id: String,
    pub score: i32,
    pub confidence: i32,
    pub method: String,
    pub evidence: String,
    pub quality: MatchQuality,
    pub tied: bool,
}

/// Oferta sem match no lote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnmatchedOffer {
    pub offer_id: String,
    pub reason: String,
    pub title: Option<String>,
    pub system_id: Option<String>,
}

/// Contadores por faixa de score.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfidenceBuckets {
    pub exact: usize,
    pub strong: usize,
    pub probable: usize,
    pub weak: usize,
}

/// Estatísticas de `matchBatch`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchStats {
    pub total: usize,
    pub matched: usize,
    pub unmatched: usize,
    pub by_method: BTreeMap<String, usize>,
    pub by_quality: BTreeMap<String, usize>,
    pub by_confidence: ConfidenceBuckets,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tied: Option<usize>,
}

/// Resultado completo de `matchBatch`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub matches: Vec<BatchMatch>,
    pub unmatched: Vec<UnmatchedOffer>,
    pub stats: BatchStats,
}

/// Normaliza título para comparação (minúsculas, sem acento/pontuação).
///
/// `\w` do JS é ASCII; por isso a classe é `[A-Za-z0-9_]` e não `\w` Unicode.
pub fn normalize_title(title: &str) -> String {
    let lowered = title.to_lowercase();
    let normalized: String = lowered.nfkc().collect::<String>().nfd().collect();
    let without_diacritics: String = normalized
        .chars()
        .filter(|character| !('\u{0300}'..='\u{036f}').contains(character))
        .collect();
    let mut output = RE_MATCH_NON_WORD
        .replace_all(&without_diacritics, " ")
        .into_owned();
    output = RE_MATCH_WHITESPACE.replace_all(&output, " ").into_owned();
    js_trim(&output).to_string()
}

/// Tokens significativos de um título (sem stopwords).
pub fn title_tokens(title: &str) -> Vec<String> {
    normalize_title(title)
        .split(' ')
        .filter(|token| !token.is_empty() && !STOPWORDS.contains(token))
        .map(ToString::to_string)
        .collect()
}

fn is_ascii_digits(token: &str) -> bool {
    RE_ASCII_DIGITS.is_match(token)
}

fn js_number_string(token: &str) -> String {
    let trimmed = token.trim_start_matches('0');
    if trimmed.is_empty() {
        return "0".to_string();
    }
    match trimmed.parse::<u64>() {
        Ok(number) => number.to_string(),
        Err(_) => trimmed.to_string(),
    }
}

fn js_round(value: f64) -> i32 {
    value.round() as i32
}

fn locale_primary(character: char) -> char {
    character
        .to_lowercase()
        .next()
        .unwrap_or(character)
        .nfd()
        .next()
        .unwrap_or(character)
}

/// Aproxima `String#localeCompare` (locale padrão do Node) para IDs de
/// catálogo: comparação primária sem caixa/acento e, no empate, minúsculas
/// antes de maiúsculas. Pontuação exótica pode divergir da ICU.
fn js_locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let mut left = a.chars();
    let mut right = b.chars();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(character_a), Some(character_b)) => {
                let primary_a = locale_primary(character_a);
                let primary_b = locale_primary(character_b);
                if primary_a != primary_b {
                    return primary_a.cmp(&primary_b);
                }
                let lowercase_a = character_a.is_lowercase();
                let lowercase_b = character_b.is_lowercase();
                if lowercase_a != lowercase_b {
                    return if lowercase_a {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    };
                }
                if character_a != character_b {
                    return character_a.cmp(&character_b);
                }
            }
        }
    }
}

fn bigrams(value: &str) -> HashSet<(u16, u16)> {
    let units: Vec<u16> = value.encode_utf16().collect();
    let mut pairs = HashSet::new();
    if units.len() < 2 {
        return pairs;
    }
    for index in 0..units.len() - 1 {
        pairs.insert((units[index], units[index + 1]));
    }
    pairs
}

fn bigram_dice(norm_a: &str, norm_b: &str) -> i32 {
    let bigrams_a = bigrams(norm_a);
    let bigrams_b = bigrams(norm_b);
    if bigrams_a.is_empty() && bigrams_b.is_empty() {
        return 100;
    }
    if bigrams_a.is_empty() || bigrams_b.is_empty() {
        return 0;
    }
    let intersection = bigrams_a
        .iter()
        .filter(|pair| bigrams_b.contains(pair))
        .count();
    js_round((2.0 * intersection as f64 / (bigrams_a.len() + bigrams_b.len()) as f64) * 100.0)
}

fn numeral_value(token: &str) -> Option<String> {
    if is_ascii_digits(token) {
        return Some(js_number_string(token));
    }
    NUMBER_WORDS
        .iter()
        .chain(ROMAN_NUMERALS.iter())
        .find(|(word, _)| *word == token)
        .map(|(_, value)| (*value).to_string())
}

/// Similaridade entre dois títulos (0-100), com as regras do JS.
pub fn fuzzy_match(a: &str, b: &str) -> i32 {
    let norm_a = normalize_title(a);
    let norm_b = normalize_title(b);
    if norm_a == norm_b {
        return 100;
    }
    if norm_a.is_empty() || norm_b.is_empty() {
        return 0;
    }

    let tokens_a = title_tokens(&norm_a);
    let tokens_b = title_tokens(&norm_b);
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return bigram_dice(&norm_a, &norm_b);
    }

    let digits_a: HashSet<String> = tokens_a
        .iter()
        .filter(|token| is_ascii_digits(token))
        .map(|token| js_number_string(token))
        .collect();
    let digits_b: HashSet<String> = tokens_b
        .iter()
        .filter(|token| is_ascii_digits(token))
        .map(|token| js_number_string(token))
        .collect();
    if digits_a != digits_b {
        return 40;
    }

    let set_a: HashSet<&str> = tokens_a.iter().map(String::as_str).collect();
    let set_b: HashSet<&str> = tokens_b.iter().map(String::as_str).collect();
    let shared = set_a.iter().filter(|token| set_b.contains(*token)).count();
    let dice = js_round((2.0 * shared as f64 / (set_a.len() + set_b.len()) as f64) * 100.0);

    let a_inside_b = set_a.iter().all(|token| set_b.contains(token));
    let b_inside_a = set_b.iter().all(|token| set_a.contains(token));

    let mut score = if a_inside_b && b_inside_a {
        95
    } else if a_inside_b || b_inside_a {
        let (shorter_size, shorter_tokens) = if a_inside_b {
            (set_a.len(), &tokens_a)
        } else {
            (set_b.len(), &tokens_b)
        };
        if shorter_size >= 2 {
            90
        } else if shorter_tokens[0].encode_utf16().count() >= 4 {
            88
        } else {
            70
        }
    } else {
        dice.min(88)
    };

    let numerals_a: HashSet<String> = tokens_a
        .iter()
        .filter_map(|token| numeral_value(token))
        .collect();
    let numerals_b: HashSet<String> = tokens_b
        .iter()
        .filter_map(|token| numeral_value(token))
        .collect();
    if numerals_a != numerals_b {
        score = score.min(CONFIDENCE_THRESHOLD_PROBABLE - 1);
    }

    score
}

/// Extrai todos os seriais possíveis de um texto, normalizados e sem duplicatas.
pub fn extract_all_serials(text: &str, system_id: &str) -> Vec<String> {
    if text.is_empty() || system_id.is_empty() {
        return Vec::new();
    }

    let mut serials = Vec::new();
    if let Some(direct) = extract_serial(text, system_id) {
        serials.push(direct);
    }

    let patterns = [
        Regex::new(r"(?i)(?-u:\b)([A-Z]{4}[-_ ]?[0-9]{5})(?-u:\b)").expect("serial PS"),
        Regex::new(r"(?i)(?-u:\b)([A-Z]{4}[0-9]{5})(?-u:\b)").expect("serial PS3"),
        Regex::new(r"(?i)(?-u:\b)(G[A-Z0-9]{3}[0-9]{2})(?-u:\b)").expect("serial GC"),
        Regex::new(r"(?i)(?-u:\b)(R[A-Z0-9]{3}[0-9]{2})(?-u:\b)").expect("serial Wii"),
    ];

    for pattern in patterns {
        for captures in pattern.captures_iter(text) {
            let normalized = normalize_serial(&captures[1]);
            if !normalized.is_empty() && !serials.contains(&normalized) {
                serials.push(normalized);
            }
        }
    }
    serials
}

fn truthy_str(value: Option<&str>) -> bool {
    value.map(|value| !value.is_empty()).unwrap_or(false)
}

fn system_is_truthy(system_id: Option<&str>) -> bool {
    truthy_str(system_id)
}

fn region_matches(offer: &RetroOffer, game: &RetroGame) -> bool {
    match offer.region.as_deref() {
        Some(region) if !region.is_empty() => game.regions.iter().any(|value| value == region),
        _ => false,
    }
}

/// Ano conforme `new Date(text).getFullYear()` para as formas que o V8 aceita
/// no uso real do catálogo: ISO (data-only é UTC), data/hora com ou sem offset,
/// e formatos legados US/mês por extenso (locais).
fn js_date_year(text: &str) -> Option<i64> {
    let trimmed = js_trim(text);
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(value.with_timezone(&Local).year() as i64);
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f%z", "%Y-%m-%d %H:%M:%S%.f%z"] {
        if let Ok(value) = DateTime::parse_from_str(trimmed, format) {
            return Some(value.with_timezone(&Local).year() as i64);
        }
    }

    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(value) = NaiveDateTime::parse_from_str(trimmed, format) {
            return Local
                .from_local_datetime(&value)
                .earliest()
                .map(|value| value.year() as i64);
        }
    }

    // ISO data-only: o V8 interpreta como UTC.
    if let Ok(value) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        let naive = value.and_hms_opt(0, 0, 0)?;
        return Some(Utc.from_utc_datetime(&naive).with_timezone(&Local).year() as i64);
    }
    if let Some((year, month)) = trimmed.split_once('-') {
        if year.len() == 4
            && !month.contains('-')
            && year.chars().all(|character| character.is_ascii_digit())
            && month.chars().all(|character| character.is_ascii_digit())
        {
            if let (Ok(year), Ok(month)) = (year.parse::<i32>(), month.parse::<u32>()) {
                if let Some(value) = NaiveDate::from_ymd_opt(year, month, 1) {
                    let naive = value.and_hms_opt(0, 0, 0)?;
                    return Some(Utc.from_utc_datetime(&naive).with_timezone(&Local).year() as i64);
                }
            }
        }
    }
    if trimmed.len() == 4 && trimmed.chars().all(|character| character.is_ascii_digit()) {
        if let Ok(year) = trimmed.parse::<i32>() {
            if let Some(value) = NaiveDate::from_ymd_opt(year, 1, 1) {
                let naive = value.and_hms_opt(0, 0, 0)?;
                return Some(Utc.from_utc_datetime(&naive).with_timezone(&Local).year() as i64);
            }
        }
    }

    // Formatos legados: o V8 interpreta na timezone local.
    for format in [
        "%Y/%m/%d",
        "%m/%d/%Y",
        "%m-%d-%Y",
        "%B %d, %Y",
        "%b %d, %Y",
        "%B %d %Y",
        "%b %d %Y",
        "%d %B %Y",
        "%d %b %Y",
    ] {
        if let Ok(value) = NaiveDate::parse_from_str(trimmed, format) {
            let naive = value.and_hms_opt(0, 0, 0)?;
            return Local
                .from_local_datetime(&naive)
                .earliest()
                .map(|value| value.year() as i64);
        }
    }

    None
}

fn hash_score(offer: &RetroOffer, game: &RetroGame) -> Option<(i32, String, String)> {
    if offer.hashes.is_empty() || game.hashes.is_empty() {
        return None;
    }
    for hash_type in ["sha1", "md5", "crc32"] {
        let Some(offer_hashes) = offer.hashes.get(hash_type) else {
            continue;
        };
        let Some(game_hashes) = game.hashes.get(hash_type) else {
            continue;
        };
        if offer_hashes.is_empty() || game_hashes.is_empty() {
            continue;
        }
        for offer_hash in offer_hashes {
            let lowered = offer_hash.to_lowercase();
            if game_hashes.iter().any(|game_hash| game_hash == &lowered) {
                return Some((
                    SCORE_HASH_EXACT,
                    "hash".to_string(),
                    format!("{hash_type}:{}", js_slice_utf16(offer_hash, 8)),
                ));
            }
        }
    }
    None
}

/// Calcula o score de match entre uma oferta e um jogo canônico.
pub fn calculate_match_score(offer: &RetroOffer, game: &RetroGame) -> MatchScore {
    let offer_system = offer.system_id.as_deref().filter(|value| !value.is_empty());
    let game_system = game.system_id.as_deref().filter(|value| !value.is_empty());
    if let (Some(offer_system), Some(game_system)) = (offer_system, game_system) {
        if offer_system != game_system {
            return MatchScore {
                score: 0,
                method: "none".to_string(),
                evidence: "different systems".to_string(),
            };
        }
    }

    let mut evidence: Vec<String> = Vec::new();

    // 1. Serial exato.
    if !offer.serials.is_empty() && !game.serials.is_empty() {
        let game_serials: Vec<String> = game
            .serials
            .iter()
            .map(|serial| normalize_serial(serial))
            .collect();
        for offer_serial in &offer.serials {
            let normalized = normalize_serial(offer_serial);
            if game_serials.iter().any(|serial| serial == &normalized) {
                evidence.push(format!("serial:{normalized}"));
                return MatchScore {
                    score: SCORE_SERIAL_EXACT,
                    method: "serial".to_string(),
                    evidence: evidence.join(", "),
                };
            }
        }
    }

    // 2. Hash exato.
    if let Some((score, method, proof)) = hash_score(offer, game) {
        evidence.push(proof);
        return MatchScore {
            score,
            method,
            evidence: evidence.join(", "),
        };
    }

    let offer_title = offer.title_for_match();
    let game_title = game.title.as_str();
    let normalized_offer_title = normalize_title(offer_title);
    let normalized_game_title = normalize_title(game_title);

    // 3. Título canônico exato.
    if !normalized_offer_title.is_empty()
        && !normalized_game_title.is_empty()
        && normalized_offer_title == normalized_game_title
    {
        let mut score = SCORE_TITLE_CANONICAL_EXACT;
        evidence.push("title:exact".to_string());
        if region_matches(offer, game) {
            score += SCORE_REGION_BONUS;
            evidence.push(format!("region:{}", offer.region.as_deref().unwrap_or("")));
        }
        return MatchScore {
            score,
            method: "exact-title".to_string(),
            evidence: evidence.join(", "),
        };
    }

    // 4. Alias exato.
    if !game.aliases.is_empty() {
        let aliases: Vec<String> = game
            .aliases
            .iter()
            .map(|alias| normalize_title(alias))
            .collect();
        if aliases.iter().any(|alias| alias == &normalized_offer_title) {
            let mut score = SCORE_ALIAS_EXACT;
            evidence.push("alias:match".to_string());
            if region_matches(offer, game) {
                score += SCORE_REGION_BONUS;
                evidence.push(format!("region:{}", offer.region.as_deref().unwrap_or("")));
            }
            return MatchScore {
                score,
                method: "alias".to_string(),
                evidence: evidence.join(", "),
            };
        }
    }

    // 5. Similaridade (título canônico e aliases).
    let mut similarity = 0;
    let mut best_name = String::new();
    for name in std::iter::once(game_title).chain(game.aliases.iter().map(String::as_str)) {
        let value = fuzzy_match(offer_title, name);
        if value > similarity {
            similarity = value;
            best_name = name.to_string();
        }
    }

    let offer_tokens: HashSet<String> = title_tokens(offer_title).into_iter().collect();
    let name_tokens: HashSet<String> = title_tokens(&best_name).into_iter().collect();
    let offer_subset = !offer_tokens.is_empty()
        && offer_tokens.len() < name_tokens.len()
        && offer_tokens.iter().all(|token| name_tokens.contains(token));

    let no_platform = !system_is_truthy(offer.system_id.as_deref());

    if similarity >= 90 {
        let mut score = if no_platform { 45 } else { SCORE_TITLE_STRONG };
        evidence.push(format!(
            "{}similarity:{similarity}",
            if no_platform { "no-platform," } else { "" }
        ));

        if !no_platform {
            if region_matches(offer, game) {
                score += SCORE_REGION_BONUS;
                evidence.push(format!("region:{}", offer.region.as_deref().unwrap_or("")));
            }
            if offer.release_year.filter(|year| *year != 0).is_some() {
                if let Some(release_date) = game.release_date.as_deref() {
                    if !release_date.is_empty() {
                        if let Some(game_year) = js_date_year(release_date) {
                            if offer.release_year == Some(game_year) {
                                score += SCORE_YEAR_BONUS;
                                evidence.push(format!("year:{game_year}"));
                            }
                        }
                    }
                }
            }
        }

        if offer_subset {
            score = score.min(CONFIDENCE_THRESHOLD_AUTO - 1);
            evidence.push("subset".to_string());
        }

        let score = if no_platform {
            score
        } else {
            score.min(SCORE_TITLE_FUZZY_MAX)
        };
        return MatchScore {
            score,
            method: "fuzzy".to_string(),
            evidence: evidence.join(", "),
        };
    }

    // 6. Fuzzy provável (65-69), sujeito a auditoria.
    if similarity >= 80 && !no_platform {
        let mut score = CONFIDENCE_THRESHOLD_PROBABLE + js_round((similarity - 80) as f64 / 3.0);
        evidence.push(format!("similarity:{similarity}"));
        if region_matches(offer, game) {
            score += SCORE_REGION_BONUS;
            evidence.push(format!("region:{}", offer.region.as_deref().unwrap_or("")));
        }
        return MatchScore {
            score: score.min(CONFIDENCE_THRESHOLD_AUTO - 1),
            method: "fuzzy".to_string(),
            evidence: evidence.join(", "),
        };
    }

    MatchScore {
        score: 0,
        method: "none".to_string(),
        evidence: "no match".to_string(),
    }
}

/// Qualidade do match a partir do score.
pub fn get_match_quality(score: i32) -> MatchQuality {
    if score >= 95 {
        MatchQuality::Exact
    } else if score >= CONFIDENCE_THRESHOLD_AUTO {
        MatchQuality::Strong
    } else if score >= CONFIDENCE_THRESHOLD_PROBABLE {
        MatchQuality::Probable
    } else {
        MatchQuality::Unmatched
    }
}

struct BestMatchCore {
    candidate: usize,
    score: i32,
    method: String,
    evidence: String,
    quality: MatchQuality,
    tied: bool,
}

fn find_best_match_core(
    offer: &RetroOffer,
    candidates: &[&RetroGame],
    options: MatchOptions,
) -> Option<BestMatchCore> {
    let offer_title = offer.title_for_match();
    struct Scored {
        candidate: usize,
        score: i32,
        same_system: bool,
        title_similarity: i32,
        method: String,
        evidence: String,
    }

    let mut scored: Vec<Scored> = Vec::new();
    for (candidate, game) in candidates.iter().enumerate() {
        let result = calculate_match_score(offer, game);
        if result.score <= 0 {
            continue;
        }
        let same_system = system_is_truthy(offer.system_id.as_deref())
            && system_is_truthy(game.system_id.as_deref())
            && offer.system_id == game.system_id;
        let mut title_similarity = 0;
        for name in
            std::iter::once(game.title.as_str()).chain(game.aliases.iter().map(String::as_str))
        {
            let value = fuzzy_match(offer_title, name);
            if value > title_similarity {
                title_similarity = value;
            }
        }
        scored.push(Scored {
            candidate,
            score: result.score,
            same_system,
            title_similarity,
            method: result.method,
            evidence: result.evidence,
        });
    }

    if scored.is_empty() {
        return None;
    }

    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| (b.same_system as i32).cmp(&(a.same_system as i32)))
            .then_with(|| b.title_similarity.cmp(&a.title_similarity))
            .then_with(|| {
                js_locale_compare(&candidates[a.candidate].id, &candidates[b.candidate].id)
            })
    });

    let top = &scored[0];
    let tie_count = scored
        .iter()
        .filter(|entry| {
            entry.score == top.score
                && entry.same_system == top.same_system
                && entry.title_similarity == top.title_similarity
        })
        .count();
    let tied = tie_count > 1;

    if top.score < options.min_confidence {
        return None;
    }

    let mut quality = get_match_quality(top.score);
    // Empate rebaixa para "probable": nunca associa automaticamente em silêncio.
    if tied && quality != MatchQuality::Unmatched {
        quality = MatchQuality::Probable;
    }
    if !options.allow_probable && quality == MatchQuality::Probable {
        return None;
    }

    let evidence = if tied {
        format!("{}, tie:{tie_count}", top.evidence)
    } else {
        top.evidence.clone()
    };

    Some(BestMatchCore {
        candidate: top.candidate,
        score: top.score,
        method: top.method.clone(),
        evidence,
        quality,
        tied,
    })
}

/// Encontra o melhor match para uma oferta em um catálogo, com opções padrão.
pub fn find_best_match(offer: &RetroOffer, games: &[RetroGame]) -> Option<BestMatch> {
    find_best_match_with(offer, games, MatchOptions::default())
}

/// Encontra o melhor match com opções explícitas.
pub fn find_best_match_with(
    offer: &RetroOffer,
    games: &[RetroGame],
    options: MatchOptions,
) -> Option<BestMatch> {
    let candidates: Vec<&RetroGame> = games.iter().collect();
    let best = find_best_match_core(offer, &candidates, options)?;
    let game = candidates[best.candidate].clone();
    Some(BestMatch {
        game_id: game.id.clone(),
        game,
        score: best.score,
        confidence: best.score,
        method: best.method,
        evidence: best.evidence,
        quality: best.quality,
        tied: best.tied,
    })
}

fn scoped_key(system_id: Option<&str>, value: &str) -> String {
    format!("{}\u{001f}{}", system_id.unwrap_or(""), value)
}

fn add_index(index: &mut HashMap<String, Vec<usize>>, key: String, position: usize) {
    index.entry(key).or_default().push(position);
}

fn first_token_of(value: &str) -> String {
    title_tokens(value).into_iter().next().unwrap_or_default()
}

/// Processa um lote com as opções padrão.
pub fn match_batch(offers: &[RetroOffer], games: &[RetroGame]) -> BatchResult {
    match_batch_with(offers, games, MatchOptions::default())
}

/// Processa um lote de ofertas contra um catálogo.
pub fn match_batch_with(
    offers: &[RetroOffer],
    games: &[RetroGame],
    options: MatchOptions,
) -> BatchResult {
    let mut by_serial: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_hash: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_title: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_alias: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_prefix: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_first_token: HashMap<String, Vec<usize>> = HashMap::new();

    for (position, game) in games.iter().enumerate() {
        let title = normalize_title(&game.title);
        add_index(
            &mut by_title,
            scoped_key(game.system_id.as_deref(), &title),
            position,
        );
        add_index(
            &mut by_prefix,
            scoped_key(game.system_id.as_deref(), js_slice_utf16(&title, 3)),
            position,
        );
        add_index(
            &mut by_first_token,
            scoped_key(game.system_id.as_deref(), &first_token_of(&title)),
            position,
        );

        for alias in &game.aliases {
            add_index(
                &mut by_alias,
                scoped_key(game.system_id.as_deref(), &normalize_title(alias)),
                position,
            );
            add_index(
                &mut by_first_token,
                scoped_key(game.system_id.as_deref(), &first_token_of(alias)),
                position,
            );
        }

        for serial in &game.serials {
            add_index(
                &mut by_serial,
                scoped_key(game.system_id.as_deref(), &normalize_serial(serial)),
                position,
            );
        }

        for hash_type in ["sha1", "md5", "crc32"] {
            for hash in game.hashes.iterable_values(hash_type) {
                add_index(
                    &mut by_hash,
                    scoped_key(
                        game.system_id.as_deref(),
                        &format!("{hash_type}:{}", hash.to_lowercase()),
                    ),
                    position,
                );
            }
        }
    }

    let candidates_for = |offer: &RetroOffer| -> Vec<usize> {
        fn include(
            found: &mut Vec<usize>,
            seen: &mut HashSet<usize>,
            positions: Option<&Vec<usize>>,
        ) {
            if let Some(positions) = positions {
                for position in positions {
                    if seen.insert(*position) {
                        found.push(*position);
                    }
                }
            }
        }

        let mut found: Vec<usize> = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();

        let system_id = offer.system_id.as_deref();
        for serial in &offer.serials {
            include(
                &mut found,
                &mut seen,
                by_serial.get(&scoped_key(system_id, &normalize_serial(serial))),
            );
        }
        for hash_type in ["sha1", "md5", "crc32"] {
            for hash in offer.hashes.iterable_values(hash_type) {
                include(
                    &mut found,
                    &mut seen,
                    by_hash.get(&scoped_key(
                        system_id,
                        &format!("{hash_type}:{}", hash.to_lowercase()),
                    )),
                );
            }
        }

        let title = normalize_title(offer.title_for_match());
        include(
            &mut found,
            &mut seen,
            by_title.get(&scoped_key(system_id, &title)),
        );
        include(
            &mut found,
            &mut seen,
            by_alias.get(&scoped_key(system_id, &title)),
        );

        for token in title_tokens(&title) {
            include(
                &mut found,
                &mut seen,
                by_first_token.get(&scoped_key(system_id, &token)),
            );
        }
        if found.is_empty() {
            include(
                &mut found,
                &mut seen,
                by_prefix.get(&scoped_key(system_id, js_slice_utf16(&title, 3))),
            );
        }

        found.truncate(2000);
        found
    };

    let mut matches = Vec::new();
    let mut unmatched = Vec::new();
    let mut stats = BatchStats {
        total: offers.len(),
        ..BatchStats::default()
    };

    for offer in offers {
        let positions = candidates_for(offer);
        let candidates: Vec<&RetroGame> =
            positions.iter().map(|position| &games[*position]).collect();
        let best = find_best_match_core(offer, &candidates, options);

        match best {
            Some(best) => {
                let game = candidates[best.candidate];
                matches.push(BatchMatch {
                    offer_id: offer.id.clone(),
                    game_id: game.id.clone(),
                    score: best.score,
                    confidence: best.score,
                    method: best.method.clone(),
                    evidence: best.evidence,
                    quality: best.quality,
                    tied: best.tied,
                });

                stats.matched += 1;
                *stats.by_method.entry(best.method).or_insert(0) += 1;
                *stats
                    .by_quality
                    .entry(best.quality.as_str().to_string())
                    .or_insert(0) += 1;
                if best.tied {
                    *stats.tied.get_or_insert(0) += 1;
                }

                if best.score >= 95 {
                    stats.by_confidence.exact += 1;
                } else if best.score >= 80 {
                    stats.by_confidence.strong += 1;
                } else if best.score >= 65 {
                    stats.by_confidence.probable += 1;
                } else {
                    stats.by_confidence.weak += 1;
                }
            }
            None => {
                unmatched.push(UnmatchedOffer {
                    offer_id: offer.id.clone(),
                    reason: "no match found".to_string(),
                    title: offer.title.clone(),
                    system_id: offer.system_id.clone(),
                });
                stats.unmatched += 1;
            }
        }
    }

    BatchResult {
        matches,
        unmatched,
        stats,
    }
}

/// Wrapper JSON de `calculateMatchScore`.
pub fn calculate_match_score_json(offer: &Value, game: &Value) -> Value {
    let offer = RetroOffer::from_json(offer);
    let game = RetroGame::from_json(game);
    serde_json::to_value(calculate_match_score(&offer, &game)).expect("score serializável")
}

/// Wrapper JSON de `findBestMatch` (preserva o objeto `game` original).
pub fn find_best_match_json(offer: &Value, games: &[Value]) -> Option<Value> {
    find_best_match_json_with(offer, games, MatchOptions::default())
}

/// Wrapper JSON de `findBestMatch` com opções explícitas.
pub fn find_best_match_json_with(
    offer: &Value,
    games: &[Value],
    options: MatchOptions,
) -> Option<Value> {
    let offer = RetroOffer::from_json(offer);
    let typed_games: Vec<RetroGame> = games.iter().map(RetroGame::from_json).collect();
    let candidates: Vec<&RetroGame> = typed_games.iter().collect();
    let best = find_best_match_core(&offer, &candidates, options)?;
    let game = candidates[best.candidate];
    let mut object = Map::new();
    object.insert("gameId".to_string(), Value::String(game.id.clone()));
    object.insert("game".to_string(), games[best.candidate].clone());
    object.insert("score".to_string(), Value::from(best.score));
    object.insert("confidence".to_string(), Value::from(best.score));
    object.insert("method".to_string(), Value::String(best.method));
    object.insert("evidence".to_string(), Value::String(best.evidence));
    object.insert(
        "quality".to_string(),
        Value::String(best.quality.as_str().to_string()),
    );
    object.insert("tied".to_string(), Value::Bool(best.tied));
    Some(Value::Object(object))
}

/// Wrapper JSON de `matchBatch`.
pub fn match_batch_json(offers: &[Value], games: &[Value]) -> Value {
    match_batch_json_with(offers, games, MatchOptions::default())
}

/// Wrapper JSON de `matchBatch` com opções explícitas.
pub fn match_batch_json_with(offers: &[Value], games: &[Value], options: MatchOptions) -> Value {
    let offers: Vec<RetroOffer> = offers.iter().map(RetroOffer::from_json).collect();
    let games: Vec<RetroGame> = games.iter().map(RetroGame::from_json).collect();
    serde_json::to_value(match_batch_with(&offers, &games, options)).expect("lote serializável")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PS1: &str = "sony-playstation";
    const PS2: &str = "sony-playstation-2";

    fn make_game(overrides: RetroGame) -> RetroGame {
        let id = if overrides.id.is_empty() {
            format!("game:{}", overrides.title)
        } else {
            overrides.id.clone()
        };
        let system_id = overrides
            .system_id
            .clone()
            .or_else(|| Some(PS2.to_string()));
        RetroGame {
            id,
            system_id,
            title: if overrides.title.is_empty() {
                "Game".to_string()
            } else {
                overrides.title
            },
            ..overrides
        }
    }

    fn game(title: &str) -> RetroGame {
        make_game(RetroGame {
            title: title.to_string(),
            ..RetroGame::default()
        })
    }

    fn make_offer(overrides: RetroOffer) -> RetroOffer {
        let normalized_title = overrides
            .normalized_title
            .clone()
            .or_else(|| Some("Game".to_string()));
        let id = if overrides.id.is_empty() {
            format!("offer:{}", normalized_title.clone().unwrap_or_default())
        } else {
            overrides.id.clone()
        };
        let system_id = overrides
            .system_id
            .clone()
            .or_else(|| Some(PS2.to_string()));
        RetroOffer {
            id,
            normalized_title,
            system_id,
            ..overrides
        }
    }

    fn offer(title: &str) -> RetroOffer {
        make_offer(RetroOffer {
            normalized_title: Some(title.to_string()),
            ..RetroOffer::default()
        })
    }

    #[test]
    fn falsos_positivos_com_palavras_comuns_nao_casam() {
        let pairs = [
            (
                "Grand Theft Auto: San Andreas",
                "Grand Theft Auto: Vice City",
            ),
            ("Spider-Man 2", "Spider-Man 3"),
            ("Final Fantasy X", "Final Fantasy X-2"),
            (
                "Ace Combat 5: The Unsung War",
                "Ace Combat Zero: The Belkan War",
            ),
            (
                "Mortal Kombat: Armageddon",
                "Mortal Kombat: Deadly Alliance",
            ),
            ("God of War II", "God of War"),
            ("Croc 2", "Croc"),
        ];

        for (offer_title, game_title) in pairs {
            let result = calculate_match_score(&offer(offer_title), &game(game_title));
            assert!(
                result.score < CONFIDENCE_THRESHOLD_PROBABLE,
                "{offer_title:?} vs {game_title:?} deu score {}",
                result.score
            );
            assert!(
                find_best_match(&offer(offer_title), &[game(game_title)]).is_none(),
                "{offer_title:?} não deveria casar com {game_title:?}"
            );
        }
    }

    #[test]
    fn numeros_diferentes_nunca_sao_o_mesmo_jogo() {
        assert!(fuzzy_match("Spider-Man 2", "Spider-Man 3") < 65);
        assert!(fuzzy_match("Final Fantasy X", "Final Fantasy X-2") < 65);
        assert!(fuzzy_match("Tekken 4", "Tekken 5") < 65);
    }

    #[test]
    fn match_batch_nao_casa_oferta_sem_correspondencia() {
        let offers = [
            make_offer(RetroOffer {
                id: "o1".to_string(),
                normalized_title: Some("Mortal Kombat: Deadly Alliance".to_string()),
                ..RetroOffer::default()
            }),
            make_offer(RetroOffer {
                id: "o2".to_string(),
                normalized_title: Some("Spider-Man 3".to_string()),
                ..RetroOffer::default()
            }),
        ];
        let games = [
            make_game(RetroGame {
                id: "g1".to_string(),
                title: "Mortal Kombat: Armageddon".to_string(),
                ..RetroGame::default()
            }),
            make_game(RetroGame {
                id: "g2".to_string(),
                title: "Spider-Man 2".to_string(),
                ..RetroGame::default()
            }),
        ];
        let batch = match_batch(&offers, &games);
        assert_eq!(batch.matches.len(), 0);
        assert_eq!(batch.unmatched.len(), 2);
    }

    #[test]
    fn escolhe_o_jogo_certo_entre_parecidos() {
        let offer = offer("Mortal Kombat: Armageddon");
        let games = [
            make_game(RetroGame {
                id: "deadly".to_string(),
                title: "Mortal Kombat: Deadly Alliance".to_string(),
                ..RetroGame::default()
            }),
            make_game(RetroGame {
                id: "armageddon".to_string(),
                title: "Mortal Kombat: Armageddon".to_string(),
                ..RetroGame::default()
            }),
            make_game(RetroGame {
                id: "deception".to_string(),
                title: "Mortal Kombat: Deception".to_string(),
                ..RetroGame::default()
            }),
        ];
        let matched = find_best_match(&offer, &games).expect("match");
        assert_eq!(matched.game_id, "armageddon");
        assert_eq!(matched.method, "exact-title");
        assert_eq!(matched.score, 85);
    }

    #[test]
    fn variacoes_do_mesmo_jogo_casam() {
        let variations = [
            (
                "James Bond 007: From Russia With Love",
                "007: From Russia with Love",
                CONFIDENCE_THRESHOLD_AUTO,
            ),
            (
                "7 Blades / Seven Blades",
                "7 Blades",
                CONFIDENCE_THRESHOLD_AUTO,
            ),
            (
                "7 Blades (Seven Blades)",
                "7 Blades",
                CONFIDENCE_THRESHOLD_AUTO,
            ),
            (
                "100 Bullets (unreleased)",
                "100 Bullets",
                CONFIDENCE_THRESHOLD_AUTO,
            ),
            (
                "Legend of Zelda, The",
                "The Legend of Zelda",
                CONFIDENCE_THRESHOLD_AUTO,
            ),
            (
                "AC/DC Live: Rock Band",
                "AC-DC Live - Rock Band",
                CONFIDENCE_THRESHOLD_AUTO,
            ),
        ];

        for (offer_title, game_title, floor) in variations {
            let matched = find_best_match(&offer(offer_title), &[game(game_title)])
                .unwrap_or_else(|| panic!("deveria casar {offer_title:?} com {game_title:?}"));
            assert!(
                matched.score >= floor,
                "{offer_title:?}: score {} abaixo de {floor}",
                matched.score
            );
            assert!(matches!(
                matched.quality,
                MatchQuality::Exact | MatchQuality::Strong
            ));
        }
    }

    #[test]
    fn titulo_curto_contido_na_oferta_casa_como_probable() {
        let matched = find_best_match_with(
            &offer("Bully (Canis Canem Edit)"),
            &[game("Bully")],
            MatchOptions {
                allow_probable: true,
                ..MatchOptions::default()
            },
        )
        .expect("match probable");
        assert_eq!(matched.quality, MatchQuality::Probable);
        assert!(matched.score >= CONFIDENCE_THRESHOLD_PROBABLE);
        assert!(matched.score < CONFIDENCE_THRESHOLD_AUTO);
    }

    #[test]
    fn match_batch_consolida_variacoes_da_mesma_fonte() {
        let offers = [
            make_offer(RetroOffer {
                id: "a".to_string(),
                normalized_title: Some("James Bond 007: Agent Under Fire".to_string()),
                ..RetroOffer::default()
            }),
            make_offer(RetroOffer {
                id: "b".to_string(),
                normalized_title: Some("007: Agent Under Fire".to_string()),
                ..RetroOffer::default()
            }),
            make_offer(RetroOffer {
                id: "c".to_string(),
                normalized_title: Some("Sky Odyssey".to_string()),
                ..RetroOffer::default()
            }),
        ];
        let games = [
            make_game(RetroGame {
                id: "agent".to_string(),
                title: "007: Agent Under Fire".to_string(),
                ..RetroGame::default()
            }),
            make_game(RetroGame {
                id: "sky".to_string(),
                title: "Sky Odyssey".to_string(),
                ..RetroGame::default()
            }),
        ];
        let batch = match_batch(&offers, &games);
        assert_eq!(batch.matches.len(), 3);
        let by_offer: HashMap<&str, &BatchMatch> = batch
            .matches
            .iter()
            .map(|matched| (matched.offer_id.as_str(), matched))
            .collect();
        assert_eq!(by_offer["a"].game_id, "agent");
        assert_eq!(by_offer["b"].game_id, "agent");
        assert_eq!(by_offer["c"].game_id, "sky");
        assert_eq!(batch.stats.by_method.get("fuzzy"), Some(&1));
        assert_eq!(batch.stats.by_method.get("exact-title"), Some(&2));
    }

    #[test]
    fn serial_exato_vence_titulo_completamente_diferente() {
        let offer = make_offer(RetroOffer {
            normalized_title: Some("Completely Different Release Name".to_string()),
            serials: vec!["SLUS-20312".to_string()],
            ..RetroOffer::default()
        });
        let game = make_game(RetroGame {
            title: "Example Adventure".to_string(),
            serials: vec!["SLUS20312".to_string()],
            ..RetroGame::default()
        });
        let result = calculate_match_score(&offer, &game);
        assert_eq!(result.score, 100);
        assert_eq!(result.method, "serial");
    }

    #[test]
    fn serial_normaliza_caixa_traco_e_underscore() {
        let offer = make_offer(RetroOffer {
            serials: vec!["slus_20312".to_string()],
            ..RetroOffer::default()
        });
        let game = make_game(RetroGame {
            serials: vec!["SLUS-20312".to_string()],
            ..RetroGame::default()
        });
        assert_eq!(calculate_match_score(&offer, &game).score, 100);
    }

    #[test]
    fn serial_nao_cruza_sistema() {
        let offer = make_offer(RetroOffer {
            system_id: Some(PS2.to_string()),
            serials: vec!["SLUS-20312".to_string()],
            ..RetroOffer::default()
        });
        let game = make_game(RetroGame {
            system_id: Some(PS1.to_string()),
            serials: vec!["SLUS20312".to_string()],
            ..RetroGame::default()
        });
        let result = calculate_match_score(&offer, &game);
        assert_eq!(result.score, 0);
        assert_eq!(result.method, "none");
    }

    #[test]
    fn match_batch_acha_jogo_so_por_serial() {
        let offers = [make_offer(RetroOffer {
            id: "serial-only".to_string(),
            normalized_title: Some("ZZZ Unknown Dump".to_string()),
            serials: vec!["SCUS-94194".to_string()],
            ..RetroOffer::default()
        })];
        let games = [make_game(RetroGame {
            id: "gt".to_string(),
            title: "Gran Turismo".to_string(),
            serials: vec!["SCUS94194".to_string()],
            ..RetroGame::default()
        })];
        let batch = match_batch(&offers, &games);
        assert_eq!(batch.matches.len(), 1);
        assert_eq!(batch.matches[0].game_id, "gt");
        assert_eq!(batch.matches[0].score, 100);
        assert_eq!(batch.matches[0].quality, MatchQuality::Exact);
    }

    #[test]
    fn extract_all_serials_acha_caixa_baixa_e_deduplica() {
        let serials = extract_all_serials("Tekken 5 slus-20312 / SLUS_20312 (USA)", PS2);
        assert_eq!(serials, vec!["SLUS20312"]);
    }

    #[test]
    fn mesmo_titulo_em_dois_sistemas_escolhe_o_sistema_da_oferta() {
        let offer = make_offer(RetroOffer {
            normalized_title: Some("Example Adventure".to_string()),
            system_id: Some(PS2.to_string()),
            ..RetroOffer::default()
        });
        let games = [
            make_game(RetroGame {
                id: "ps1".to_string(),
                system_id: Some(PS1.to_string()),
                title: "Example Adventure".to_string(),
                ..RetroGame::default()
            }),
            make_game(RetroGame {
                id: "ps2".to_string(),
                system_id: Some(PS2.to_string()),
                title: "Example Adventure".to_string(),
                ..RetroGame::default()
            }),
        ];
        let matched = find_best_match(&offer, &games).expect("match");
        assert_eq!(matched.game_id, "ps2");
    }

    #[test]
    fn nunca_cruza_sistemas_mesmo_com_titulo_identico() {
        let offer = make_offer(RetroOffer {
            normalized_title: Some("Example Adventure".to_string()),
            system_id: Some(PS2.to_string()),
            ..RetroOffer::default()
        });
        let game = make_game(RetroGame {
            system_id: Some(PS1.to_string()),
            title: "Example Adventure".to_string(),
            ..RetroGame::default()
        });
        assert_eq!(calculate_match_score(&offer, &game).score, 0);
    }

    fn tied_games() -> Vec<RetroGame> {
        vec![
            make_game(RetroGame {
                id: "zzz".to_string(),
                title: "Twin Game".to_string(),
                ..RetroGame::default()
            }),
            make_game(RetroGame {
                id: "aaa".to_string(),
                title: "Twin Game".to_string(),
                ..RetroGame::default()
            }),
        ]
    }

    #[test]
    fn empate_rebaixa_para_probable_e_fica_marcado() {
        let offer = offer("Twin Game");
        let matched = find_best_match_with(
            &offer,
            &tied_games(),
            MatchOptions {
                allow_probable: true,
                ..MatchOptions::default()
            },
        )
        .expect("match");
        assert!(matched.tied);
        assert_eq!(matched.quality, MatchQuality::Probable);
        assert!(matched.evidence.contains("tie:2"));
    }

    #[test]
    fn empate_nao_associa_automaticamente_sem_allow_probable() {
        assert!(find_best_match(&offer("Twin Game"), &tied_games()).is_none());
    }

    #[test]
    fn resultado_independe_da_ordem_do_catalogo() {
        let offer = offer("Twin Game");
        let options = MatchOptions {
            allow_probable: true,
            ..MatchOptions::default()
        };
        let forward = find_best_match_with(&offer, &tied_games(), options).expect("forward");
        let mut reversed = tied_games();
        reversed.reverse();
        let backward = find_best_match_with(&offer, &reversed, options).expect("backward");
        assert_eq!(forward.game_id, backward.game_id);
        assert_eq!(forward.game_id, "aaa");
    }

    #[test]
    fn match_batch_estavel_com_catalogo_invertido() {
        let offers = [make_offer(RetroOffer {
            id: "o1".to_string(),
            normalized_title: Some("Twin Game".to_string()),
            ..RetroOffer::default()
        })];
        let options = MatchOptions {
            allow_probable: true,
            ..MatchOptions::default()
        };
        let mut reversed = tied_games();
        reversed.reverse();
        let forward = match_batch_with(&offers, &tied_games(), options);
        let backward = match_batch_with(&offers, &reversed, options);
        assert_eq!(forward.matches, backward.matches);
        assert_eq!(forward.stats.tied, Some(1));
    }

    #[test]
    fn match_batch_devolve_score_confidence_quality_e_tied() {
        let offers = [make_offer(RetroOffer {
            id: "o1".to_string(),
            normalized_title: Some("James Bond 007: Agent Under Fire".to_string()),
            ..RetroOffer::default()
        })];
        let games = [make_game(RetroGame {
            id: "agent".to_string(),
            title: "007: Agent Under Fire".to_string(),
            ..RetroGame::default()
        })];
        let batch = match_batch(&offers, &games);
        let matched = &batch.matches[0];
        assert_eq!(matched.confidence, matched.score);
        assert_eq!(matched.quality, MatchQuality::Strong);
        assert!(!matched.tied);
        assert_eq!(matched.method, "fuzzy");
    }

    #[test]
    fn get_match_quality_classifica_as_faixas() {
        assert_eq!(get_match_quality(100), MatchQuality::Exact);
        assert_eq!(get_match_quality(95), MatchQuality::Exact);
        assert_eq!(get_match_quality(94), MatchQuality::Strong);
        assert_eq!(get_match_quality(80), MatchQuality::Strong);
        assert_eq!(get_match_quality(79), MatchQuality::Probable);
        assert_eq!(get_match_quality(65), MatchQuality::Probable);
        assert_eq!(get_match_quality(64), MatchQuality::Unmatched);
    }

    #[test]
    fn piso_padrao_nao_aceita_probable_sem_allow_probable() {
        let offer = offer("Bully (Canis Canem Edit)");
        let games = [game("Bully")];
        assert!(find_best_match(&offer, &games).is_none());
        let allowed = find_best_match_with(
            &offer,
            &games,
            MatchOptions {
                allow_probable: true,
                ..MatchOptions::default()
            },
        )
        .expect("match");
        assert_eq!(allowed.quality, MatchQuality::Probable);
    }

    #[test]
    fn min_confidence_respeita_o_piso_pedido() {
        let offer = offer("007: Agent Under Fire");
        let games = [game("007: Agent Under Fire")];
        assert!(find_best_match_with(
            &offer,
            &games,
            MatchOptions {
                min_confidence: 90,
                ..MatchOptions::default()
            }
        )
        .is_none());
        assert!(find_best_match_with(
            &offer,
            &games,
            MatchOptions {
                min_confidence: 85,
                ..MatchOptions::default()
            }
        )
        .is_some());
    }

    #[test]
    fn normalize_title_remove_acento_pontuacao_e_caixa() {
        assert_eq!(
            normalize_title("Pokémon: Edição Vermelha!"),
            "pokemon edicao vermelha"
        );
    }

    #[test]
    fn fuzzy_match_e_simetrico_e_identidade_vale_100() {
        assert_eq!(fuzzy_match("Final Fantasy VII", "Final Fantasy VII"), 100);
        assert_eq!(
            fuzzy_match("James Bond 007: Agent Under Fire", "007: Agent Under Fire"),
            fuzzy_match("007: Agent Under Fire", "James Bond 007: Agent Under Fire")
        );
    }

    #[test]
    fn alias_exato_casa_e_recebe_bonus_de_regiao() {
        let offer = make_offer(RetroOffer {
            normalized_title: Some("Alternate Name".to_string()),
            region: Some("USA".to_string()),
            ..RetroOffer::default()
        });
        let game = make_game(RetroGame {
            title: "Canonical Name".to_string(),
            aliases: vec!["Alternate Name".to_string()],
            regions: vec!["USA".to_string()],
            ..RetroGame::default()
        });
        let result = calculate_match_score(&offer, &game);
        assert_eq!(result.score, 90);
        assert_eq!(result.method, "alias");
        assert!(result.evidence.contains("region:USA"));
    }

    #[test]
    fn hash_exato_casa_por_sha1_md5_e_crc32() {
        for hash_type in ["sha1", "md5", "crc32"] {
            let mut offer_hashes = BTreeMap::new();
            offer_hashes.insert(hash_type.to_string(), vec!["ABCDEF0123456789".to_string()]);
            let mut game_hashes = BTreeMap::new();
            game_hashes.insert(hash_type.to_string(), vec!["abcdef0123456789".to_string()]);
            let offer = make_offer(RetroOffer {
                hashes: Hashes::from_values(offer_hashes),
                ..RetroOffer::default()
            });
            let game = make_game(RetroGame {
                title: "Different Title".to_string(),
                hashes: Hashes::from_values(game_hashes),
                ..RetroGame::default()
            });
            let result = calculate_match_score(&offer, &game);
            assert_eq!(result.score, 100, "{hash_type}");
            assert_eq!(result.method, "hash");
            assert!(result
                .evidence
                .starts_with(&format!("{hash_type}:ABCDEF01")));
        }
    }

    #[test]
    fn extrai_ano_de_data_como_o_date_do_js() {
        assert_eq!(js_date_year("1997-09-30"), Some(1997));
        assert_eq!(js_date_year("1997-09-30T23:00:00-05:00"), Some(1997));
        assert_eq!(js_date_year("09/30/1997"), Some(1997));
        assert_eq!(js_date_year("September 30, 1997"), Some(1997));
        assert_eq!(js_date_year("1997/09/30"), Some(1997));
        assert_eq!(js_date_year("30/09/1997"), None);
        assert_eq!(js_date_year("not a date"), None);
        assert_eq!(js_date_year(""), None);
    }

    #[test]
    fn ano_bonifica_match_fuzzy_no_mesmo_sistema() {
        let offer = make_offer(RetroOffer {
            normalized_title: Some("Game Name (unreleased)".to_string()),
            release_year: Some(1997),
            ..RetroOffer::default()
        });
        let game = make_game(RetroGame {
            title: "Game Name".to_string(),
            release_date: Some("1997-09-30".to_string()),
            ..RetroGame::default()
        });
        let result = calculate_match_score(&offer, &game);
        assert_eq!(result.score, 85);
        assert!(result.evidence.contains("year:1997"));
    }

    #[test]
    fn oferta_sem_plataforma_nao_sustenta_associacao_automatica() {
        let mut offer = offer("James Bond 007: From Russia With Love");
        offer.system_id = None;
        let game = game("007: From Russia with Love");
        let result = calculate_match_score(&offer, &game);
        assert_eq!(result.score, 45);
        assert!(result.evidence.contains("no-platform"));
        assert!(find_best_match(&offer, &[game]).is_none());
    }

    #[test]
    fn oferta_subconjunto_estrito_fica_abaixo_do_piso_automatico() {
        let offer = offer("Bully Canis");
        let game = game("Bully Canis Canem Edit");
        let result = calculate_match_score(&offer, &game);
        assert!(result.score < CONFIDENCE_THRESHOLD_AUTO);
        assert!(result.evidence.contains("subset"));
    }

    #[test]
    fn match_batch_limita_candidatos_por_sistema() {
        let offers = [make_offer(RetroOffer {
            normalized_title: Some("Example Adventure".to_string()),
            system_id: Some(PS2.to_string()),
            ..RetroOffer::default()
        })];
        let games = [make_game(RetroGame {
            id: "ps1-game".to_string(),
            system_id: Some(PS1.to_string()),
            title: "Example Adventure".to_string(),
            ..RetroGame::default()
        })];
        let batch = match_batch(&offers, &games);
        assert!(batch.matches.is_empty());
        assert_eq!(batch.unmatched.len(), 1);
    }

    #[test]
    fn json_preserva_o_shape_consumido_pelo_cliente() {
        let offer = json!({
            "id": "o1",
            "normalizedTitle": "Example Adventure",
            "systemId": PS2,
            "serials": ["SLUS-20312"],
            "hashes": { "sha1": "ABC" },
            "region": "USA"
        });
        let game = json!({
            "id": "g1",
            "systemId": PS2,
            "title": "Example Adventure",
            "aliases": [],
            "serials": ["SLUS20312"],
            "hashes": {},
            "regions": ["USA"],
            "releaseDate": "1997-09-30",
            "developer": "Preserved"
        });
        let matched = find_best_match_json(&offer, &[game]).expect("match");
        assert_eq!(matched["gameId"], "g1");
        assert_eq!(matched["score"], 100);
        assert_eq!(matched["method"], "serial");
        assert_eq!(matched["quality"], "exact");
        assert_eq!(matched["game"]["developer"], "Preserved");

        let batch = match_batch_json(
            &[offer],
            &[json!({
                "id": "g1",
                "systemId": PS2,
                "title": "Example Adventure",
                "serials": ["SLUS20312"],
                "hashes": {},
                "regions": []
            })],
        );
        assert_eq!(batch["matches"][0]["offerId"], "o1");
        assert_eq!(batch["stats"]["byMethod"]["serial"], 1);
        assert_eq!(batch["stats"]["byConfidence"]["exact"], 1);
    }

    #[test]
    fn json_calculate_match_score_devolve_evidence_string() {
        let result = calculate_match_score_json(
            &json!({ "normalizedTitle": "Twin Game", "systemId": PS2 }),
            &json!({ "title": "Twin Game", "systemId": PS2 }),
        );
        assert_eq!(result["score"], 85);
        assert_eq!(result["method"], "exact-title");
        assert_eq!(result["evidence"], "title:exact");
    }

    #[test]
    fn duplicatas_no_catalogo_sao_marcadas_como_empate() {
        let offer = offer("Twin Game");
        let options = MatchOptions {
            allow_probable: true,
            ..MatchOptions::default()
        };
        let games = [game("Twin Game"), game("Twin Game")];
        let matched = find_best_match_with(&offer, &games, options).expect("match");
        assert!(matched.tied);
        assert_eq!(matched.quality, MatchQuality::Probable);
    }

    #[test]
    fn empate_desempata_por_id_como_locale_compare() {
        let offer = offer("Twin Game");
        let options = MatchOptions {
            allow_probable: true,
            ..MatchOptions::default()
        };
        let games = [
            make_game(RetroGame {
                id: "Game".to_string(),
                title: "Twin Game".to_string(),
                ..RetroGame::default()
            }),
            make_game(RetroGame {
                id: "game".to_string(),
                title: "Twin Game".to_string(),
                ..RetroGame::default()
            }),
        ];
        // Node: "game".localeCompare("Game") < 0.
        let matched = find_best_match_with(&offer, &games, options).expect("match");
        assert_eq!(matched.game_id, "game");
    }

    #[test]
    fn hash_escalar_e_array_tem_semantica_diferente_no_lote() {
        // calculateMatchScore trata string como singleton.
        let mut scalar = RetroOffer {
            normalized_title: Some("Different".to_string()),
            system_id: Some(PS2.to_string()),
            ..RetroOffer::default()
        };
        scalar.hashes.insert_scalar("sha1", "abcdef0123456789");

        let mut array_game = RetroGame {
            id: "array-game".to_string(),
            title: "Other".to_string(),
            system_id: Some(PS2.to_string()),
            ..RetroGame::default()
        };
        array_game
            .hashes
            .insert("sha1", vec!["abcdef0123456789".to_string()]);
        assert_eq!(calculate_match_score(&scalar, &array_game).score, 100);

        // matchBatch itera a string do JS por caractere, então o índice do jogo
        // (array, chave completa) não é encontrado.
        let batch = match_batch(
            std::slice::from_ref(&scalar),
            std::slice::from_ref(&array_game),
        );
        assert!(batch.matches.is_empty());
        assert_eq!(batch.unmatched.len(), 1);

        // String + string iguais compartilham os caracteres e chegam ao score.
        let mut scalar_game = array_game.clone();
        scalar_game.hashes = Hashes::default();
        scalar_game.hashes.insert_scalar("sha1", "abcdef0123456789");
        let batch = match_batch(
            std::slice::from_ref(&scalar),
            std::slice::from_ref(&scalar_game),
        );
        assert_eq!(batch.matches.len(), 1);
        assert_eq!(batch.matches[0].score, 100);

        // Array + array é o caminho normal.
        let mut array_offer = scalar.clone();
        array_offer.hashes = Hashes::default();
        array_offer
            .hashes
            .insert("sha1", vec!["abcdef0123456789".to_string()]);
        let batch = match_batch(
            std::slice::from_ref(&array_offer),
            std::slice::from_ref(&array_game),
        );
        assert_eq!(batch.matches.len(), 1);
    }

    #[test]
    fn unicode_nao_ascii_segue_o_js() {
        // `\w` é ASCII no JS; letras cirílicas sumem da chave do matcher.
        assert_eq!(normalize_title("Игра: Edição"), "edicao");
        assert_eq!(normalize_title("Pokémon"), "pokemon");
    }

    #[test]
    fn bigram_dice_cobre_titulos_de_um_token() {
        assert!(fuzzy_match("X", "Y") < 65);
        assert_eq!(fuzzy_match("X", "X"), 100);
    }
}
