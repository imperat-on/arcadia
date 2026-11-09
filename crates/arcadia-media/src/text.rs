//! Normalização de título, limpeza de HTML e extensão de arquivo.
//!
//! Port literal dos helpers de `app/electron/metadata.js`. Nada aqui depende
//! de rede ou de relógio.

use std::sync::OnceLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use url::Url;

/// `normalizaTitulo`: NFD, remove marcas de acento, minúsculas e corta tudo
/// que não for `[a-z0-9]`.
pub fn normalize_title(value: &str) -> String {
    value
        .nfd()
        .filter(|character| !matches!(*character, '\u{0300}'..='\u{036f}'))
        .collect::<String>()
        .to_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

/// `tituloBate`: igualdade ou prefixo comum entre os títulos normalizados.
pub fn titles_match(left: &str, right: &str) -> bool {
    let left = normalize_title(left);
    let right = normalize_title(right);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    left == right || left.starts_with(&right) || right.starts_with(&left)
}

/// `semHTML`: tags viram quebra de linha/remoção e as entidades usadas pela
/// Steam/Xbox são decodificadas na mesma ordem do JS.
pub fn strip_html(value: &str) -> String {
    static BREAK: OnceLock<Regex> = OnceLock::new();
    static PARAGRAPH: OnceLock<Regex> = OnceLock::new();
    static TAGS: OnceLock<Regex> = OnceLock::new();
    static BLANK_LINES: OnceLock<Regex> = OnceLock::new();

    let text = BREAK
        .get_or_init(|| Regex::new(r"(?i)<br\s*/?>").expect("regex de <br>"))
        .replace_all(value, "\n");
    let text = PARAGRAPH
        .get_or_init(|| Regex::new(r"(?i)</p>").expect("regex de </p>"))
        .replace_all(&text, "\n");
    let text = TAGS
        .get_or_init(|| Regex::new(r"<[^>]+>").expect("regex de tags"))
        .replace_all(&text, "");
    let text = text
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    let text = BLANK_LINES
        .get_or_init(|| Regex::new(r"\n{3,}").expect("regex de linhas em branco"))
        .replace_all(&text, "\n\n");
    text.trim().to_string()
}

/// Extensões que o `downloadTo` reconhece pelo `content-type`.
pub fn mime_extension(content_type: &str) -> Option<&'static str> {
    match content_type {
        "image/png" => Some(".png"),
        "image/jpeg" => Some(".jpg"),
        "image/webp" => Some(".webp"),
        "image/gif" => Some(".gif"),
        "video/webm" => Some(".webm"),
        "video/mp4" => Some(".mp4"),
        _ => None,
    }
}

/// `extFromUrl`: o caminho da URL manda; o content-type é o desempate; o
/// fallback final é `.png`.
pub fn extension_from_url(url: &str, content_type: &str) -> String {
    static EXTENSION: OnceLock<Regex> = OnceLock::new();
    if let Ok(parsed) = Url::parse(url) {
        let path = parsed.path();
        if let Some(capture) = EXTENSION
            .get_or_init(|| Regex::new(r"(?i)\.([a-z0-9]{2,5})$").expect("regex de extensão"))
            .captures(path)
        {
            return format!(".{}", capture[1].to_lowercase());
        }
    }
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    mime_extension(&media_type).unwrap_or(".png").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_title_strips_accents_case_and_punctuation() {
        assert_eq!(normalize_title("Ação & Reação: Vol. 2"), "acaoreacaovol2");
        assert_eq!(normalize_title("Half-Life 2"), "halflife2");
        assert_eq!(normalize_title("  ---  "), "");
        assert_eq!(normalize_title("Pokémon"), "pokemon");
        assert_eq!(normalize_title(42.to_string().as_str()), "42");
    }

    #[test]
    fn titles_match_exact_and_prefix_only() {
        assert!(titles_match("Elden Ring", "elden ring"));
        assert!(titles_match(
            "Grand Theft Auto V",
            "Grand Theft Auto V Legacy"
        ));
        assert!(titles_match("Portal", "Portal 2"));
        assert!(!titles_match("Elden Ring", "Eldrynn"));
        assert!(!titles_match("", "Portal"));
        assert!(!titles_match("!!!", "???"));
    }

    #[test]
    fn strip_html_matches_the_client_pipeline() {
        assert_eq!(
            strip_html("<p>Olá<br>mundo</p><p>Segunda</p>"),
            "Olá\nmundo\nSegunda"
        );
        assert_eq!(
            strip_html("&quot;A&quot; &amp; B &lt;C&gt;&nbsp;D"),
            "\"A\" & B <C> D"
        );
        assert_eq!(strip_html("a\n\n\n\nb"), "a\n\nb");
        assert_eq!(strip_html("   <b>x</b>   "), "x");
        assert_eq!(strip_html("<BR/>a</P>\n\n\nb"), "a\n\nb");
    }

    #[test]
    fn extension_prefers_the_url_path() {
        assert_eq!(
            extension_from_url("https://x/art.webp?x=1", "image/png"),
            ".webp"
        );
        assert_eq!(extension_from_url("https://x/ART.PNG", ""), ".png");
        assert_eq!(
            extension_from_url("https://x/download?id=1", "image/jpeg; charset=x"),
            ".jpg"
        );
        assert_eq!(
            extension_from_url("https://x/download", "video/webm"),
            ".webm"
        );
        assert_eq!(
            extension_from_url("https://x/download", "application/zip"),
            ".png"
        );
        // Extensão de 1 caractere não passa pelo padrão do JS.
        assert_eq!(extension_from_url("https://x/a.b", "image/gif"), ".gif");
        assert_eq!(extension_from_url("não é url", "image/png"), ".png");
    }
}
