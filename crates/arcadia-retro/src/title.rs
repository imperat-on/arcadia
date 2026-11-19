//! Parser de títulos retrô.
//!
//! Port 1:1 de `app/electron/retro-title-parser.js`. As regexes repetem a
//! semântica do JavaScript: classes ASCII onde o JS usa `\w`/`\d`, limites de
//! palavra ASCII (`(?-u:\b)`), `\p{L}`/`\p{N}` onde o JS usa `u`, e dobra de
//! caixa Unicode como `String#toLowerCase`.
//!
//! Normalização Unicode (NFKC/NFD) usa `unicode-normalization`, a mesma
//! decomposição canônica que `String#normalize`.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

pub const MAX_TITLE: usize = 300;
pub const MAX_RAW_TITLE: usize = 2048;

const RETRO_REGION_TAGS: &[&str] = &[
    "australia",
    "asia",
    "brazil",
    "canada",
    "europe",
    "eu",
    "eur",
    "france",
    "germany",
    "italy",
    "japan",
    "jp",
    "korea",
    "pal",
    "russia",
    "rus",
    "spain",
    "usa",
    "us",
    "u",
    "world",
    "ww",
];

const RETRO_LANGUAGE_TAGS: &[&str] = &[
    "ar",
    "br",
    "de",
    "en",
    "es",
    "fr",
    "it",
    "ja",
    "j",
    "ko",
    "pt",
    "ru",
    "sp",
    "zh",
    "eng",
    "jpn",
    "jap",
    "rus",
    "spa",
    "fra",
    "ger",
    "ita",
    "kor",
    "chn",
    "ntsc",
    "english",
    "portuguese",
    "dutch",
    "french",
    "german",
    "italian",
    "japanese",
    "russian",
    "spanish",
];

const RETRO_RELEASE_GROUPS: &[&str] = &[
    "codex",
    "cpy",
    "dauphong",
    "dodi",
    "elamigos",
    "empress",
    "fitgirl",
    "flt",
    "goldberg",
    "hoodlum",
    "kaoskrew",
    "onlinefix",
    "p2p",
    "plaza",
    "prophet",
    "proper",
    "razor",
    "razor1911",
    "reloaded",
    "repack",
    "rip",
    "rld",
    "rune",
    "skidrow",
    "steamgg",
    "steamrip",
    "tenoke",
    "tinyiso",
    "xatab",
];

// Format tags: extensões de arquivo e tipos de mídia.
static RE_FORMAT_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:7z|archive|bin(?:\s*/\s*cue)?|cd|cso|cue|chd|disc|disk|dvd|e?boot|gdi|iso|nsp|pkg|rom|rvz|tap|torrent|wad|wbfs|xci)$",
    )
    .expect("regex de formato")
});

// Identificadores de plataforma.
static RE_PLATFORM_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:3do|amiga|arcade|atari(?:\s*2600|\s*5200|\s*7800)?|colecovision|dreamcast|game(?:boy|gear)|gba|gbc|genesis|megadrive|n(?:es|64|ds)|pc(?:\s+engine)?|pce(?:\s+cd)?(?:\s*[-/]\s*ps[1-5])?|psx?(?:\s*[-/]\s*(?:ps[1-5]|dvr))?|ps[1-5](?:\s*[-/]\s*ps[1-5])?|psp(?:\s*[-/]\s*ps[1-5])?|saturn|sega(?:\s*cd|\s*32x)?|snes|switch|wiiu?|xbox)$",
    )
    .expect("regex de plataforma")
});

// Seriais/versões.
static RE_SERIAL_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:[a-z]{2,8}[-_ ]?[0-9]{2,}[a-z0-9\-/ %]*|[0-9]{2,}[a-z][a-z0-9-]*|(?:rev|revision|ver|version|v|disc|disk|cd)\s*[a-z0-9._/-]+)$",
    )
    .expect("regex de serial")
});

// Flags/status.
static RE_FLAG_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:[!abfhp]|aftermarket|all|beta|bios?|convert|demo|digital|dendy|download|dlc|folder|full|hack|homebrew|mod|multi[0-9]*|move|ntsc(?:-[a-z0-9]+)?|ode|pal|patch|pirate|proper|prototype|proto|region|redump|repack|soft|translation|trainer|unl|unlicensed|virtual console)$",
    )
    .expect("regex de flag")
});

// Grupo de release genérico (exportado pelo JS, não usado no fluxo).
static RE_GROUP_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}][\p{L}\p{N}._'-]{1,31}$").expect("regex de grupo"));

// Prefixo de serial PlayStation (PS1/PS2/PS3/PSP).
static RE_PLAYSTATION_SERIAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)([SBUN][CL][UUJE][SD])[-_ ]?([0-9]{5})(?-u:\b)")
        .expect("regex de serial PlayStation")
});

static RE_PS3_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[BN][CL][UE][SD]$").expect("prefixo PS3"));
static RE_PSP_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[UN][CL][UJ][SD]$").expect("prefixo PSP"));
static RE_PS1_PS2_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[SB][CL][UE][SD]$").expect("prefixo PS1/PS2"));

// Marcador de versão/update no fim: `v1.1`, `v.92935+1300`, `- v1.0.4 + DLC`.
static RE_VERSION_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(?:[,;\u{2013}\u{2014}-]\s*)?(?:update\s*)?v\.?[0-9][A-Za-z0-9_.-]*(?:\s*\+[^()]*)?$")
        .expect("regex de versão")
});

static RE_BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").expect("regex br"));
static RE_HTML_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[^>]*>").expect("regex de tag html"));
static RE_NUMERIC_ENTITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)&#(x[0-9a-f]+|[0-9]+);").expect("regex entidade numérica"));
static RE_NBSP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)&nbsp;").expect("regex nbsp"));
static RE_AMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)&amp;").expect("regex amp"));
static RE_QUOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)&quot;").expect("regex quot"));
static RE_APOS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)&#39;|&apos;").expect("regex apos"));
static RE_LT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)&lt;").expect("regex lt"));
static RE_GT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)&gt;").expect("regex gt"));
static RE_CONTROL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x00-\x1f\x7f]").expect("regex de controle"));
static RE_WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("regex de espaços"));
static RE_DASHES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\u{2010}-\u{2015}]").expect("regex de travessões"));
static RE_UNDERSCORES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"_+").expect("regex de underscores"));
static RE_DOT_EXTENSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(?:7z|7zip|bin|chd|cue|iso|nsp|pkg|rar|rom|rvz|wad|wbfs|xci)$")
        .expect("regex de extensão")
});
static RE_TRAILING_SQUARE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+\[[^\]\n]*$").expect("regex colchete aberto"));
static RE_TRAILING_PAREN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+\([^()\n]*$").expect("regex parêntese aberto"));
static RE_LEADING_GROUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(\[[^\]\n]*\]|\([^()\n]*\))\s*").expect("regex grupo inicial")
});
static RE_SQUARE_GROUPS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\n]*)\]").expect("regex grupos colchetes"));
static RE_STRIP_SQUARE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\[([^\]\n]*)\]").expect("regex remoção colchetes"));
static RE_TRAILING_GROUPS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\s*)(\[[^\]\n]*\]|\([^()\n]*\))\s*$").expect("regex grupos finais")
});
static RE_EXPLICIT_RELEASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:team|studio|company|translation|translated|voice|sound|patch|озвуч|перевод|текст|фанат|версии)")
        .expect("regex release explícito")
});
static RE_PATCH_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(?:[+]\s*)?(?:patch|translation)[-_\s]+(?:eng|rus|jap|jpn|eur|usa)(?:[+|/]?(?:eng|rus|jap|jpn|eur|usa))*[+]?\s*$")
        .expect("regex patch")
});
static RE_FORMAT_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[\s\u{2013}\u{2014}|,:-])(?:7z|bin(?:\s*/\s*cue)?|chd|cso|cue|iso|nsp|pkg|rom|rvz|wad|wbfs|xci)\s*$")
        .expect("regex formato")
});
static RE_EDGE_PUNCT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\s_-]+|[\s_-]+$").expect("regex bordas"));
static RE_FREE_DOWNLOAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:free\s+download|download\s+free)(?-u:\b)")
        .expect("regex free download")
});
static RE_SCENE_REPACK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*[-\u{2013}\u{2014}]\s*(?:repack|proper|rip|p2p)\s*$").expect("regex repack")
});
static RE_SCENE_GROUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\s*[-\u{2013}\u{2014}]\s*([\p{L}\p{N}!._]{2,24})\s*$")
        .expect("regex grupo de cena")
});
static RE_LEADING_ARTICLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:the|an?)\s+").expect("regex artigo"));
static RE_AMPERSAND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*&\s*").expect("regex ampersand"));
static RE_VERSUS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?-u:\b)vs\.?(?-u:\b)").expect("regex versus"));
static RE_NON_ALNUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}]+").expect("regex não alfanumérico"));
static RE_TAG_WORDS_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\s,;|/+&().\-]+").expect("regex palavras de tag"));
static RE_TAG_EDGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[-_.]+|[-_.]+$").expect("regex borda de tag"));
static RE_COMBINED_LANGUAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:full|multi)?(?:rus|eng|eur|jap|jpn|spa|fra|ger|ita|kor|chn)(?:sound)?$")
        .expect("regex idioma combinado")
});
static RE_NTSC_PAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:ntsc|pal)(?:[-_][a-z0-9]+)?$").expect("regex ntsc/pal"));
static RE_PERCENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]{1,3}%$").expect("regex porcentagem"));
static RE_IRD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^ird(?:[0-9]+%?)?$").expect("regex ird"));
static RE_DISC_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:dvd|cd|disc)[0-9]+$").expect("regex disco"));
static RE_DIGITS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+$").expect("regex dígitos"));
static RE_DIMENSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^~?[0-9]+(?:x[0-9]+)?$").expect("regex dimensão"));
static RE_RATIO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+:[0-9]+$").expect("regex razão"));
static RE_BOOLEAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:true|false)$").expect("regex booleano"));
static RE_AUDIO_BONUS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:eng|rus)(?:sound|sounds|soundtracks|audio|bonus)(?:s)?$")
        .expect("regex áudio")
});
static RE_DATE_YMD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:19|20)[0-9]{2}(?:[-/.](?:0?[0-9]|1[0-2]))?(?:[-/.](?:0?[1-9]|[12][0-9]|3[01]))?$",
    )
    .expect("regex data ymd")
});
static RE_DATE_DMY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9]{1,2}[./-][0-9]{1,2}[./-](?:19|20)[0-9]{2}$").expect("regex data dmy")
});
static RE_ITEMS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?iu)^~?[0-9][0-9\s.,~]*(?:шт|items?|games?)\.?$").expect("regex itens")
});
static RE_REGION_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:(?:all\s+)?region|non[-\s]?redump)$").expect("regex região")
});
static RE_DLC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?-u:\b)dlc(?-u:\b)").expect("regex dlc"));
static RE_UPDATE_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:update|hotfix|patch|build|repack|install)(?-u:\b)")
        .expect("regex update")
});
static RE_VERSION_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:rev|revision|ver|version|v)[0-9]*$").expect("regex versão palavra")
});
static RE_MIXED_SEPARATORS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[|/+,]").expect("regex separadores"));
static RE_N_IN_N: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^[0-9]+\s+in\s+[0-9]+$").expect("regex n in n"));
static RE_EDITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:anniversary|collection|complete|cut|deluxe|director|edition|greatest|limited|remaster(?:ed)?|special|ultimate)(?-u:\b)")
        .expect("regex edição")
});
static RE_RELEASE_INDICATOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:[/|+,]|[-–—]|(?-u:\b)(?:company|convert|fan(?:s)?|mvo|retrogaming|rgr|studio|team|text|translation|transgen|version|voice|vhs)(?-u:\b)|(?:текст|озвуч|фанат|перевод|версии))")
        .expect("regex indicador de release")
});
static RE_GROUP_WORD_SHAPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\p{L}\p{N}][\p{L}\p{N}._'']*(?:\s+[\p{L}\p{N}][\p{L}\p{N}._'']*)*$")
        .expect("regex forma de grupo")
});
static RE_ALIAS_SPECIAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:dot\s+hack|biohazard|resident\s+evil)(?-u:\b)")
        .expect("regex alias especial")
});
static RE_ALIAS_PASSWORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:password|senha)\s*:").expect("regex senha"));
static RE_ALIAS_TEAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:team|studio|company)\s*$").expect("regex equipe"));
static RE_ALIAS_TRANSLATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:translation|translated|voice|sound|patch|озвуч|перевод|текст|фанат|версии)")
        .expect("regex tradução")
});
static RE_NON_ASCII_ALNUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^a-z0-9]+").expect("regex ascii"));
static RE_HACK_FRAGMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\.hack//frägment(?-u:\b)").expect("regex fragment"));

// Classificação de release.
static RE_BIOS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:bios|firmware|bootrom|system\s*files?)(?-u:\b)")
        .expect("regex bios")
});
static RE_DLC_CONTENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:dlc|downloadable\s*content|expansion\s*pack)(?-u:\b)")
        .expect("regex dlc content")
});
static RE_UPDATE_CONTENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?-u:\b)(?:update|patch(?:es)?|hotfix|bugfix|ver(?:sion)?\s*[0-9]+\.[0-9]+)(?-u:\b)",
    )
    .expect("regex update content")
});
static RE_HOMEBREW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:homebrew|home\s*brew|unofficial|fan\s*made)(?-u:\b)")
        .expect("regex homebrew")
});
static RE_HACK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:hack|rom\s*hack|romhack|mod(?:ified)?|hack\s*by)(?-u:\b)")
        .expect("regex hack")
});
static RE_TRANSLATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:translation|translated|patch[-_\s]+(?:eng|rus|jap|jpn|eur|usa|spa|fra|ger|ita))(?-u:\b)")
        .expect("regex translation")
});
static RE_COLLECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:collection|anthology|compilation|complete\s*(?:edition|set)|greatest\s*hits|all\s*in\s*one)(?-u:\b)")
        .expect("regex collection")
});
static RE_N_IN_N_CONTENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)[0-9]+\s*in\s*[0-9]+(?-u:\b)").expect("regex n in n content")
});

/// Serial PlayStation extraído de um texto.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayStationSerial {
    pub serial: String,
    pub platform: String,
}

/// Resultado de `parseRetroTitle`, com os mesmos nomes JSON do JS.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedRetroTitle {
    pub title: String,
    pub original_title: String,
    pub serial: Option<String>,
    pub platform: Option<String>,
    pub release_kind: Option<String>,
}

/// `String#trim` do JS: inclui BOM e separadores de linha Unicode.
fn is_js_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'..='\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

pub(crate) fn js_trim(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

/// `String#slice(0, max)` conta unidades UTF-16. Um par substituto cortado ao
/// meio não é representável em `String`; nesse caso o corte para antes do par.
pub(crate) fn js_slice_utf16(value: &str, max: usize) -> &str {
    if max == 0 {
        return "";
    }
    let mut units = 0usize;
    let mut end = 0usize;
    for (index, character) in value.char_indices() {
        let width = character.len_utf16();
        if units + width > max {
            break;
        }
        units += width;
        end = index + character.len_utf8();
    }
    &value[..end]
}

fn decode_numeric_entity(code: &str) -> String {
    let parsed = if code.len() > 1 && (code.starts_with('x') || code.starts_with('X')) {
        u32::from_str_radix(&code[1..], 16).ok()
    } else {
        code.parse::<u32>().ok()
    };
    match parsed {
        // `String.fromCodePoint` aceita substitutos isolados, mas eles deixam
        // de ser letras/números na etapa final; usar espaço produz o mesmo
        // título canônico.
        Some(value) if value <= 0x10ffff && !(0xd800..=0xdfff).contains(&value) => {
            char::from_u32(value)
                .map(|character| character.to_string())
                .unwrap_or_else(|| " ".to_string())
        }
        _ => " ".to_string(),
    }
}

fn decode_entities(value: &str) -> String {
    let mut output = RE_BR.replace_all(value, "\n").into_owned();
    output = RE_HTML_TAG.replace_all(&output, " ").into_owned();
    output = RE_NBSP.replace_all(&output, " ").into_owned();
    output = RE_AMP.replace_all(&output, "&").into_owned();
    output = RE_QUOT.replace_all(&output, "\"").into_owned();
    output = RE_APOS.replace_all(&output, "'").into_owned();
    output = RE_LT.replace_all(&output, "<").into_owned();
    output = RE_GT.replace_all(&output, ">").into_owned();
    RE_NUMERIC_ENTITY
        .replace_all(&output, |captures: &Captures<'_>| {
            decode_numeric_entity(&captures[1])
        })
        .into_owned()
}

fn is_metadata_set_token(token: &str) -> bool {
    token.is_empty()
        || RETRO_REGION_TAGS.contains(&token)
        || RETRO_LANGUAGE_TAGS.contains(&token)
        || RE_FORMAT_TAG.is_match(token)
        || RE_PLATFORM_TAG.is_match(token)
        || RE_FLAG_TAG.is_match(token)
        || RE_COMBINED_LANGUAGE.is_match(token)
        || RE_NTSC_PAL.is_match(token)
        || RE_PERCENT.is_match(token)
        || RE_IRD.is_match(token)
        || RE_DISC_NUMBER.is_match(token)
        || RE_DIGITS.is_match(token)
        || RE_DIMENSION.is_match(token)
        || RE_RATIO.is_match(token)
        || RE_BOOLEAN.is_match(token)
        || RE_AUDIO_BONUS.is_match(token)
        || RE_SERIAL_TAG.is_match(token)
}

/// Tokeniza um texto em palavras normalizadas, como `retroTagWords`.
pub fn retro_tag_words(value: &str) -> Vec<String> {
    let normalized: String = value.nfkc().collect::<String>().nfd().collect();
    let stripped: String = normalized
        .chars()
        .filter(|character| !('\u{0300}'..='\u{036f}').contains(character))
        .collect();
    let lowered = stripped.to_lowercase();
    RE_TAG_WORDS_SPLIT
        .split(&lowered)
        .map(js_trim)
        .filter(|word| !word.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Informa se um token é metadado (região, idioma, formato, serial, flag...).
pub fn is_retro_metadata_token(value: &str) -> bool {
    let token = js_trim(value).to_lowercase();
    if token.is_empty() {
        return true;
    }
    is_metadata_set_token(&token)
}

/// Informa se o conteúdo de um grupo entre colchetes/parênteses é metadado.
pub fn is_retro_metadata_tag(value: &str) -> bool {
    let trimmed = RE_TAG_EDGE.replace_all(value, "").into_owned();
    let tag = js_trim(&trimmed);
    if tag.is_empty() {
        return true;
    }
    if RE_FORMAT_TAG.is_match(tag) || RE_PLATFORM_TAG.is_match(tag) {
        return true;
    }
    if RE_SERIAL_TAG.is_match(tag) || RE_FLAG_TAG.is_match(tag) {
        return true;
    }
    if RE_DATE_YMD.is_match(tag) || RE_DATE_DMY.is_match(tag) || RE_ITEMS.is_match(tag) {
        return true;
    }
    if RE_REGION_WORD.is_match(tag) {
        return true;
    }
    if RE_DLC.is_match(tag) {
        return true;
    }
    if RE_UPDATE_WORD.is_match(tag) {
        return true;
    }

    let words = retro_tag_words(tag);
    if words.is_empty() {
        return true;
    }
    if words.iter().all(|word| is_retro_metadata_token(word)) {
        return true;
    }

    if RE_MIXED_SEPARATORS.is_match(tag)
        && words
            .iter()
            .filter(|word| is_retro_metadata_token(word))
            .count()
            >= 2
    {
        return true;
    }
    if words.iter().any(|word| RE_VERSION_WORD.is_match(word)) {
        return true;
    }

    false
}

/// Informa se o conteúdo de um grupo parece um nome de release group.
pub fn is_retro_release_group_tag(value: &str) -> bool {
    let tag = js_trim(value);
    if tag.is_empty() || is_retro_metadata_tag(tag) {
        return false;
    }
    if RE_N_IN_N.is_match(tag) {
        return false;
    }
    if RE_EDITION.is_match(tag) {
        return false;
    }
    if RE_RELEASE_INDICATOR.is_match(tag) {
        return true;
    }

    let words = retro_tag_words(tag);
    !words.is_empty() && words.len() <= 4 && RE_GROUP_WORD_SHAPE.is_match(tag)
}

/// Informa se o conteúdo de um parêntese é um alias/nome alternativo.
pub fn is_retro_alias_group(group: &str, before: &str) -> bool {
    let text = js_trim(group);
    if RE_ALIAS_SPECIAL.is_match(text) || RE_ALIAS_PASSWORD.is_match(text) {
        return true;
    }
    if RE_ALIAS_TEAM.is_match(text) || RE_ALIAS_TRANSLATION.is_match(text) {
        return true;
    }

    let before_words = RE_NON_ASCII_ALNUM
        .replace_all(&before.to_lowercase(), " ")
        .into_owned();
    let words: Vec<String> = RE_NON_ASCII_ALNUM
        .replace_all(&text.to_lowercase(), " ")
        .split_whitespace()
        .filter(|word| word.chars().count() > 2)
        .map(ToString::to_string)
        .collect();
    words
        .iter()
        .filter(|word| before_words.contains(word.as_str()))
        .count()
        >= 2
}

/// Remove grupos de metadados (colchetes/parênteses) do título.
pub fn strip_retro_groups(title: &str) -> String {
    let mut cleaned = title.to_string();
    let square_groups: Vec<String> = RE_SQUARE_GROUPS
        .captures_iter(title)
        .map(|captures| captures[1].to_string())
        .collect();

    let has_explicit_release = square_groups.iter().any(|group| {
        is_retro_metadata_tag(group)
            || (is_retro_release_group_tag(group) && RE_EXPLICIT_RELEASE.is_match(group))
    });

    if has_explicit_release {
        cleaned = RE_STRIP_SQUARE
            .replace_all(&cleaned, |captures: &Captures<'_>| {
                let value = &captures[1];
                if is_retro_metadata_tag(value) || is_retro_release_group_tag(value) {
                    String::new()
                } else {
                    captures[0].to_string()
                }
            })
            .into_owned();
    }

    // Grupos finais são avaliados individualmente.
    let mut rest = cleaned;
    let mut groups: Vec<String> = Vec::new();
    while let Some(found) = RE_TRAILING_GROUPS.captures(&rest) {
        let whole = found.get(0).expect("match inteiro");
        let raw = found[1].to_string();
        groups.insert(0, raw);
        rest = js_trim(&rest[..whole.start()]).to_string();
    }
    let kept: Vec<String> = groups
        .into_iter()
        .filter(|raw| {
            let value = js_trim(&raw[1..raw.len() - 1]);
            !is_retro_metadata_tag(value) && !is_retro_release_group_tag(value)
        })
        .collect();
    let mut output = rest;
    for raw in kept {
        output.push(' ');
        output.push_str(&raw);
    }
    js_trim(&output).to_string()
}

/// Remove parênteses de alias, respeitando aninhamento.
pub fn strip_retro_alias_parentheticals(title: &str) -> String {
    let characters: Vec<char> = title.chars().collect();
    let mut output = String::new();
    let mut index = 0usize;
    while index < characters.len() {
        if characters[index] != '(' {
            output.push(characters[index]);
            index += 1;
            continue;
        }
        let mut depth = 1usize;
        let mut end = index + 1;
        while end < characters.len() && depth > 0 {
            match characters[end] {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            end += 1;
        }
        if depth != 0 {
            output.push(characters[index]);
            index += 1;
            continue;
        }
        let group: String = characters[index + 1..end - 1].iter().collect();
        if is_retro_alias_group(&group, &output) {
            let trimmed = output.trim_end_matches(is_js_whitespace).len();
            output.truncate(trimmed);
        } else {
            output.extend(&characters[index..end]);
        }
        index = end;
    }
    output
}

/// Dobra diacríticos latinos e letras não decomponíveis para ASCII.
///
/// O `/[ßæœøđðþł]/gi` do JS (sem flag `u`) não dobra `ẞ` (U+1E9E): apenas as
/// formas listadas e suas variantes simples de caixa. Reproduzimos isso com
/// pares explícitos em vez de baixar a caixa de qualquer caractere.
pub fn fold_retro_diacritics(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.nfd() {
        if ('\u{0300}'..='\u{036f}').contains(&character) {
            continue;
        }
        match character {
            'ß' => output.push_str("ss"),
            'æ' | 'Æ' => output.push_str("ae"),
            'œ' | 'Œ' => output.push_str("oe"),
            'ø' | 'Ø' => output.push('o'),
            'đ' | 'Đ' => output.push('d'),
            'ð' | 'Ð' => output.push('d'),
            'þ' | 'Þ' => output.push_str("th"),
            'ł' | 'Ł' => output.push('l'),
            _ => output.push(character),
        }
    }
    output
}

fn strip_retro_scene_groups(title: &str) -> String {
    let stripped = js_trim(&RE_SCENE_REPACK.replace_all(title, "")).to_string();
    if let Some(captures) = RE_SCENE_GROUP.captures(&stripped) {
        let group = captures[1].to_lowercase();
        if RETRO_RELEASE_GROUPS.contains(&group.as_str()) {
            let start = captures.get(0).map(|found| found.start()).unwrap_or(0);
            return js_trim(&stripped[..start]).to_string();
        }
    }
    stripped
}

fn strip_retro_version_suffix(title: &str) -> String {
    let mut current = title.to_string();
    loop {
        let next = RE_VERSION_SUFFIX.replace_all(&current, "").into_owned();
        if next == current {
            break;
        }
        current = next;
    }
    js_trim(&current).to_string()
}

fn strip_retro_leading_article(title: &str) -> String {
    let stripped = js_trim(&RE_LEADING_ARTICLE.replace(title, "")).to_string();
    if stripped.is_empty() {
        title.to_string()
    } else {
        stripped
    }
}

/// Formas romanas canônicas de 1..=50, exatamente como o `Map` do JS.
static ROMAN_NUMERALS: LazyLock<HashMap<String, u32>> = LazyLock::new(|| {
    const STEPS: &[(u32, &str)] = &[
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut map = HashMap::new();
    for number in 1..=50u32 {
        let mut rest = number;
        let mut roman = String::new();
        for (value, symbol) in STEPS {
            while rest >= *value {
                roman.push_str(symbol);
                rest -= *value;
            }
        }
        map.insert(roman, number);
    }
    map
});

fn roman_numeral_value(roman: &str) -> Option<u32> {
    ROMAN_NUMERALS.get(roman).copied()
}

fn romanize_retro_numerals(title: &str) -> String {
    title
        .split(' ')
        .map(|word| {
            let roman = word.to_uppercase();
            if roman.chars().count() < 2
                || !roman
                    .chars()
                    .all(|character| matches!(character, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'))
            {
                return word.to_string();
            }
            match roman_numeral_value(&roman) {
                Some(value) => value.to_string(),
                None => word.to_string(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// `cleanText` do cliente Electron: decodifica entidades, remove tags/controle,
/// colapsa espaços e corta em `max` unidades UTF-16.
pub fn clean_text(value: &str, max: usize) -> String {
    let mut output = decode_entities(value);
    output = RE_CONTROL.replace_all(&output, " ").into_owned();
    output = RE_WHITESPACE.replace_all(&output, " ").into_owned();
    let trimmed = js_trim(&output);
    js_slice_utf16(trimmed, max).to_string()
}

/// Normaliza um título retrô para exibição/busca (limite padrão de 300).
pub fn normalize_retro_title(value: &str) -> String {
    normalize_retro_title_with_max(value, MAX_TITLE)
}

/// Variante com limite configurável, como o parâmetro `max` do JS.
pub fn normalize_retro_title_with_max(value: &str, max: usize) -> String {
    let mut original = decode_entities(value);
    original = RE_CONTROL.replace_all(&original, " ").into_owned();
    original = RE_WHITESPACE.replace_all(&original, " ").into_owned();
    original = js_trim(&original).to_string();
    original = js_slice_utf16(&original, MAX_RAW_TITLE).to_string();

    if original.is_empty() {
        return String::new();
    }

    let normalized: String = original.nfkc().collect();
    let mut title = RE_DASHES.replace_all(&normalized, "-").into_owned();
    title = RE_UNDERSCORES.replace_all(&title, " ").into_owned();
    title = RE_WHITESPACE.replace_all(&title, " ").into_owned();
    title = RE_DOT_EXTENSION.replace(&title, "").into_owned();
    title = js_trim(&title).to_string();

    // Remove grupos malformados.
    title = RE_TRAILING_SQUARE.replace(&title, "").into_owned();
    title = RE_TRAILING_PAREN.replace(&title, "").into_owned();
    title = js_trim(&title).to_string();

    // Remove tags iniciais de plataforma/metadados.
    while let Some(leading) = RE_LEADING_GROUP.find(&title) {
        let raw = &title[leading.start()..leading.end()];
        let group = raw
            .trim_start_matches(|character: char| character.is_whitespace())
            .trim_end_matches(|character: char| character.is_whitespace());
        let inner = &group[1..group.len() - 1];
        if !is_retro_metadata_tag(inner) {
            break;
        }
        title = js_trim(&title[leading.end()..]).to_string();
    }

    title = strip_retro_groups(&title);
    title = strip_retro_alias_parentheticals(&title);

    // Normaliza a série .hack//.
    let has_hack_prefix = title
        .get(..6)
        .map(|prefix| prefix.eq_ignore_ascii_case(".hack/"))
        .unwrap_or(false);
    let hack_rest = title.get(6..).unwrap_or("");
    if has_hack_prefix && !hack_rest.starts_with('/') {
        title = format!(".hack//{hack_rest}");
    }
    title = RE_HACK_FRAGMENT
        .replace(&title, ".hack//Fragment")
        .into_owned();

    // Remove sufixos de patch de tradução.
    loop {
        let next = RE_PATCH_SUFFIX.replace_all(&title, "").into_owned();
        if next == title {
            break;
        }
        title = next;
    }

    // Remove sufixos de formato.
    title = RE_FORMAT_SUFFIX.replace_all(&title, "").into_owned();
    title = RE_TRAILING_SQUARE.replace(&title, "").into_owned();
    title = RE_TRAILING_PAREN.replace(&title, "").into_owned();
    title = RE_WHITESPACE.replace_all(&title, " ").into_owned();
    title = RE_EDGE_PUNCT.replace_all(&title, "").into_owned();
    title = js_trim(&title).to_string();

    // Ruído de source e sufixos de release/versão.
    title = RE_FREE_DOWNLOAD.replace_all(&title, " ").into_owned();
    title = js_trim(&title).to_string();
    title = strip_retro_scene_groups(&title);
    title = strip_retro_version_suffix(&title);
    title = strip_retro_leading_article(&title);

    // Chave canônica de comparação.
    title = fold_retro_diacritics(&title);
    title = RE_AMPERSAND.replace_all(&title, " and ").into_owned();
    title = RE_VERSUS.replace_all(&title, " versus ").into_owned();
    title = RE_NON_ALNUM.replace_all(&title, " ").into_owned();
    title = RE_WHITESPACE.replace_all(&title, " ").into_owned();
    title = js_trim(&title).to_string();
    title = romanize_retro_numerals(&title);

    js_slice_utf16(&title, max).to_string()
}

/// Extrai um serial PlayStation (PS1/PS2/PS3/PSP) de um texto.
pub fn extract_playstation_serial(text: &str) -> Option<PlayStationSerial> {
    let captures = RE_PLAYSTATION_SERIAL.captures(text)?;
    let prefix = captures[1].to_uppercase();
    let number = captures[2].to_string();
    let serial = format!("{prefix}-{number}");

    let platform = if RE_PS3_PREFIX.is_match(&prefix) {
        "PS3"
    } else if RE_PSP_PREFIX.is_match(&prefix) {
        "PSP"
    } else if RE_PS1_PS2_PREFIX.is_match(&prefix) {
        // PS2 costuma começar em 20000+.
        match number.parse::<u32>() {
            Ok(value) if value >= 20000 => "PS2",
            Ok(_) => "PS1",
            Err(_) => return None,
        }
    } else {
        return None;
    };

    Some(PlayStationSerial {
        serial,
        platform: platform.to_string(),
    })
}

/// Classifica o tipo de release a partir do título original.
pub fn classify_release_kind(title: &str) -> Option<&'static str> {
    if title.is_empty() {
        return None;
    }
    let lower = title.to_lowercase();
    if RE_BIOS.is_match(&lower) {
        return Some("bios");
    }
    if RE_DLC_CONTENT.is_match(&lower) {
        return Some("dlc");
    }
    if RE_UPDATE_CONTENT.is_match(&lower) {
        return Some("update");
    }
    if RE_HOMEBREW.is_match(&lower) {
        return Some("homebrew");
    }
    if RE_HACK.is_match(&lower) {
        return Some("hack");
    }
    if RE_TRANSLATION.is_match(&lower) {
        return Some("translation");
    }
    if RE_COLLECTION.is_match(&lower) || RE_N_IN_N_CONTENT.is_match(&lower) {
        return Some("collection");
    }
    None
}

/// Extrai título canônico e metadados de um texto bruto.
pub fn parse_retro_title(value: &str) -> ParsedRetroTitle {
    let original_title = js_trim(value).to_string();
    let title = normalize_retro_title(&original_title);
    let serial = extract_playstation_serial(&original_title);
    let release_kind = classify_release_kind(&original_title).map(ToString::to_string);

    ParsedRetroTitle {
        title,
        original_title,
        serial: serial.as_ref().map(|value| value.serial.clone()),
        platform: serial.map(|value| value.platform),
        release_kind,
    }
}

/// Testa o padrão genérico de release group exportado pelo JS.
pub fn is_retro_group_tag(value: &str) -> bool {
    RE_GROUP_TAG.is_match(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regiao_entre_parenteses_sai_do_titulo_canonico() {
        for region in ["USA", "Europe", "Japan", "World", "BR"] {
            assert_eq!(
                normalize_retro_title(&format!("Sonic the Hedgehog ({region})")),
                "Sonic the Hedgehog",
                "{region}"
            );
        }
    }

    #[test]
    fn tags_de_dump_e_metadados_saem_do_titulo_canonico() {
        let raws = [
            "Chrono Trigger [!]",
            "Chrono Trigger (Rev 1)",
            "Chrono Trigger (v1.1)",
            "Chrono Trigger [b]",
            "Chrono Trigger (Beta)",
            "Chrono Trigger (Proto)",
        ];
        for raw in raws {
            assert_eq!(normalize_retro_title(raw), "Chrono Trigger", "{raw}");
        }
    }

    #[test]
    fn grupo_de_release_no_fim_sai_do_titulo_canonico() {
        assert_eq!(
            normalize_retro_title("007 First Light-CODEX"),
            "007 First Light"
        );
        assert_eq!(
            normalize_retro_title("007 First Light-RUNE"),
            "007 First Light"
        );
        assert_eq!(
            normalize_retro_title("007 First Light-SKIDROW"),
            "007 First Light"
        );
        assert_eq!(
            normalize_retro_title("Final Vendetta (MULTi11) [FitGirl Repack]"),
            "Final Vendetta"
        );
        assert_eq!(
            normalize_retro_title("Super Metroid (T-En by X)"),
            "Super Metroid"
        );
    }

    #[test]
    fn subtitulo_legitimo_em_caixa_alta_nao_e_grupo_de_release() {
        assert_eq!(
            normalize_retro_title("Armored Core - NEXUS [RUS|NTSC]"),
            "Armored Core NEXUS"
        );
        assert_eq!(
            normalize_retro_title("Sesame Street - ABC (USA)"),
            "Sesame Street ABC"
        );
        assert_eq!(
            normalize_retro_title("Aokana - Four Rhythms Across the Blue - EXTRA2"),
            "Aokana Four Rhythms Across the Blue EXTRA2"
        );
    }

    #[test]
    fn artigo_inicial_nao_diferencia_o_titulo_canonico() {
        assert_eq!(
            normalize_retro_title("The Legend of Zelda (USA)"),
            "Legend of Zelda"
        );
        assert_eq!(
            normalize_retro_title("Legend of Zelda (USA)"),
            "Legend of Zelda"
        );
    }

    #[test]
    fn numeros_romanos_multi_letra_e_arabicos_convergem() {
        assert_eq!(
            normalize_retro_title("Final Fantasy VII (USA)"),
            "Final Fantasy 7"
        );
        assert_eq!(
            normalize_retro_title("Final Fantasy 7 (USA)"),
            "Final Fantasy 7"
        );
        assert_eq!(
            normalize_retro_title("Final Fantasy XVI (USA)"),
            "Final Fantasy 16"
        );
        assert_eq!(
            normalize_retro_title("Adventure Island II (USA) (Beta)"),
            "Adventure Island 2"
        );
        assert_eq!(normalize_retro_title("MIX (USA)"), "MIX");
    }

    #[test]
    fn romano_de_uma_letra_fica_intacto() {
        assert_eq!(normalize_retro_title("Mega Man X (USA)"), "Mega Man X");
        assert_eq!(normalize_retro_title("Mega Man 10 (USA)"), "Mega Man 10");
        assert_eq!(
            normalize_retro_title("Grand Theft Auto V (USA)"),
            "Grand Theft Auto V"
        );
    }

    #[test]
    fn acentos_e_cedilha_convergem_para_ascii() {
        assert_eq!(normalize_retro_title("Ação Total (BR)"), "Acao Total");
        assert_eq!(normalize_retro_title("Acao Total (BR)"), "Acao Total");
        assert_eq!(normalize_retro_title("Pokémon Red (USA)"), "Pokemon Red");
        assert_eq!(normalize_retro_title("Pokemon Red (USA)"), "Pokemon Red");
    }

    #[test]
    fn pontuacao_espacos_ampersand_e_vs_convergem() {
        assert_eq!(
            normalize_retro_title("Spider-Man   vs.   The Hulk"),
            "Spider Man versus The Hulk"
        );
        assert_eq!(
            normalize_retro_title("Spider Man versus The Hulk"),
            "Spider Man versus The Hulk"
        );
        assert_eq!(normalize_retro_title("Conan & Rygar"), "Conan and Rygar");
        assert_eq!(normalize_retro_title("Conan and Rygar"), "Conan and Rygar");
    }

    #[test]
    fn abreviacoes_com_pontos_nao_perdem_o_artigo() {
        assert_eq!(
            normalize_retro_title("A.C.E.: Another Century's Episode [JAP|NTSC]"),
            "A C E Another Century s Episode"
        );
    }

    #[test]
    fn titulos_crus_reais_das_sources_normalizam_para_o_canonico() {
        let cases = [
            ("Bully (Canis Canem Edit) [RUS|NTSC] [ViT Company]", "Bully"),
            (
                "Ace Combat 04: Shattered Skies (Distant Thunder) [ENG|NTSC]",
                "Ace Combat 04 Shattered Skies",
            ),
            ("7 Blades (Seven Blades) [Multi3|PAL]", "7 Blades"),
            (
                "Resident Evil 4 (Biohazard 4) (password: psxroms.pro)",
                "Resident Evil 4",
            ),
            (
                "1000 Stars (World) (Proto) (Byte-Off 2020) (Aftermarket) (Unl)",
                "1000 Stars",
            ),
            (
                "EMPYRE: Dukes of the Far Frontier [FitGirl Repack]",
                "EMPYRE Dukes of the Far Frontier",
            ),
            ("007 First Light-RUNE", "007 First Light"),
            (
                "Mortal Shell II v.92935+1300 [Папка игры] (2026)",
                "Mortal Shell 2",
            ),
            (
                "Lou’s Lagoon: Deluxe Edition – v1.0.4-43683 + 3 DLCs/Bonuses",
                "Lou s Lagoon Deluxe Edition",
            ),
            (
                "The Elder Scrolls V: Skyrim Anniversary Edition Free Download (v1.7.104.0.8)",
                "Elder Scrolls V Skyrim Anniversary Edition",
            ),
        ];
        for (raw, canonical) in cases {
            assert_eq!(normalize_retro_title(raw), canonical, "{raw}");
        }
    }

    #[test]
    fn parse_retro_title_preserva_original_e_entrega_canonico() {
        let parsed = parse_retro_title("The Legend of Zelda (USA) (Rev 1)");
        assert_eq!(parsed.original_title, "The Legend of Zelda (USA) (Rev 1)");
        assert_eq!(parsed.title, "Legend of Zelda");
        assert_eq!(parsed.serial, None);
        assert_eq!(parsed.platform, None);
        assert_eq!(parsed.release_kind, None);
    }

    #[test]
    fn parse_retro_title_extrai_serial_e_plataforma() {
        let parsed = parse_retro_title("[SLUS-20312] Example Game");
        assert_eq!(parsed.serial.as_deref(), Some("SLUS-20312"));
        assert_eq!(parsed.platform.as_deref(), Some("PS2"));

        let ps1 = parse_retro_title("Example [SCUS-12345]");
        assert_eq!(ps1.serial.as_deref(), Some("SCUS-12345"));
        assert_eq!(ps1.platform.as_deref(), Some("PS1"));

        // O JS decide PS1/PS2 pela faixa numérica (>= 20000 vira PS2), mesmo
        // para seriais SCUS que historicamente são de PS1.
        let ps2_range = parse_retro_title("Example [SCUS-94900]");
        assert_eq!(ps2_range.platform.as_deref(), Some("PS2"));

        let ps3 = parse_retro_title("Example BLUS-30100");
        assert_eq!(ps3.serial.as_deref(), Some("BLUS-30100"));
        assert_eq!(ps3.platform.as_deref(), Some("PS3"));

        let psp = parse_retro_title("Example ULUS-10001");
        assert_eq!(psp.serial.as_deref(), Some("ULUS-10001"));
        assert_eq!(psp.platform.as_deref(), Some("PSP"));
    }

    #[test]
    fn parse_retro_title_serializa_com_os_nomes_do_cliente() {
        let parsed = parse_retro_title("Example [SLUS-20312]");
        let value = serde_json::to_value(&parsed).expect("json");
        assert_eq!(value["title"], "Example");
        assert_eq!(value["originalTitle"], "Example [SLUS-20312]");
        assert_eq!(value["serial"], "SLUS-20312");
        assert_eq!(value["platform"], "PS2");
        assert!(value["releaseKind"].is_null());
    }

    #[test]
    fn classify_release_kind_segue_a_ordem_do_js() {
        assert_eq!(classify_release_kind("Game BIOS (USA)"), Some("bios"));
        assert_eq!(classify_release_kind("Game DLC Pack"), Some("dlc"));
        assert_eq!(classify_release_kind("Game Update v1.2"), Some("update"));
        assert_eq!(classify_release_kind("Homebrew Game"), Some("homebrew"));
        assert_eq!(classify_release_kind("Rom Hack"), Some("hack"));
        assert_eq!(
            classify_release_kind("Game Translation"),
            Some("translation")
        );
        assert_eq!(classify_release_kind("Game Collection"), Some("collection"));
        assert_eq!(classify_release_kind("3 in 1 Game"), Some("collection"));
        assert_eq!(classify_release_kind("Normal Game"), None);
        assert_eq!(classify_release_kind(""), None);
    }

    #[test]
    fn tags_de_metadados_e_release_groups() {
        assert!(is_retro_metadata_tag("USA"));
        assert!(is_retro_metadata_tag("RUS|NTSC"));
        assert!(is_retro_metadata_tag("Rev 1"));
        assert!(is_retro_metadata_tag("1997-09-30"));
        assert!(is_retro_metadata_tag("2 items"));
        assert!(!is_retro_metadata_tag("Canis Canem Edit"));

        assert!(is_retro_metadata_tag("FitGirl Repack"));
        assert!(!is_retro_release_group_tag("FitGirl Repack"));
        assert!(is_retro_release_group_tag("ViT Company"));
        assert!(!is_retro_release_group_tag("Anniversary Edition"));
        assert!(!is_retro_release_group_tag("2 in 1"));
    }

    #[test]
    fn alias_parentetico_e_removido_sem_tocar_no_resto() {
        assert_eq!(
            strip_retro_alias_parentheticals("Resident Evil 4 (Biohazard 4)"),
            "Resident Evil 4"
        );
        // O fluxo completo remove o grupo por ser release group; o helper de
        // alias isolado mantém o texto, como no JS.
        assert_eq!(
            strip_retro_alias_parentheticals("Bully (Canis Canem Edit)"),
            "Bully (Canis Canem Edit)"
        );
        assert_eq!(normalize_retro_title("Bully (Canis Canem Edit)"), "Bully");
        assert_eq!(strip_retro_alias_parentheticals("Game (USA)"), "Game (USA)");
        assert_eq!(
            strip_retro_alias_parentheticals("Outer (Inner (Nested)) Tail"),
            "Outer (Inner (Nested)) Tail"
        );
    }

    #[test]
    fn dobra_de_diacriticos_nao_decomponiveis() {
        assert_eq!(fold_retro_diacritics("Straße"), "Strasse");
        assert_eq!(fold_retro_diacritics("ÆØÅ"), "aeoA");
        assert_eq!(fold_retro_diacritics("ŒĐÐÞŁ"), "oeddthl");
        assert_eq!(fold_retro_diacritics("Łódź"), "lodz");
        assert_eq!(fold_retro_diacritics("Pokémon"), "Pokemon");
        // O JS sem flag `u` não dobra ẞ (U+1E9E).
        assert_eq!(fold_retro_diacritics("ẞ"), "ẞ");
        assert_eq!(normalize_retro_title("ẞ (USA)"), "ẞ");
    }

    #[test]
    fn unicode_nao_ascii_e_preservado_quando_o_js_preserva() {
        // `\p{L}` mantém letras cirílicas na chave canônica.
        assert_eq!(normalize_retro_title("Игра (USA)"), "Игра");
        // Números não ASCII passam por NFKC e viram dígitos ASCII.
        assert_eq!(normalize_retro_title("Game １２３"), "Game 123");
    }

    #[test]
    fn limite_de_titulo_e_aplicado_em_unidades_utf16() {
        assert_eq!(normalize_retro_title_with_max("abcdef", 3), "abc");
        assert_eq!(normalize_retro_title_with_max("abcdef", 0), "");
    }
}
