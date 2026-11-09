//! Pontes entre espaços de chave da MESMA conquista e resolução de appid.
//!
//! Porte 1:1 de `app/electron/achievements/match.js`:
//! apiname exato → apelido (`aliases[]`) → título normalizado, este último
//! somente quando o título é ÚNICO dentro do appid. Inclui também as regras de
//! appid/serial usadas pelo watcher (`steam:<appid>`, `ACObsidian_Ach_40`).
//!
//! Títulos repetidos viram `None` no índice: marcar a conquista errada é pior
//! do que não marcar.

use std::collections::HashMap;

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use crate::js::{js_string_value, js_trim};

/// Título comparável: sem acento, sem caixa e sem espaço duplicado.
///
/// `"Pé de  valsa"` e `"Pe de valsa"` são o mesmo título.
pub fn normalize_title(value: &str) -> String {
    let decomposed: String = value.nfd().collect();
    let without_marks: String = decomposed
        .chars()
        .filter(|character| !('\u{0300}'..='\u{036f}').contains(character))
        .collect();
    without_marks
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `normalizeTitle(String(value ?? ""))`.
pub fn normalize_title_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(value) => normalize_title(&js_string_value(value)),
    }
}

/// apiname canônico do item + todos os apelidos conhecidos, sem repetição.
pub fn aliases_of(item: &Value) -> Vec<String> {
    let Some(object) = item.as_object() else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let mut add = |value: Option<&Value>| {
        let name = match value {
            None | Some(Value::Null) => String::new(),
            Some(value) => js_trim(&js_string_value(value)).to_string(),
        };
        if !name.is_empty() && !output.contains(&name) {
            output.push(name);
        }
    };
    add(object.get("apiname"));
    if let Some(Value::Array(aliases)) = object.get("aliases") {
        for alias in aliases {
            add(Some(alias));
        }
    }
    output
}

/// Guarda um apiname extra no item. Devolve `true` quando o item mudou.
///
/// Nunca guarda o apiname do próprio item nem duplicata (comparação exata,
/// como o `includes` do JS).
pub fn remember_alias(item: &mut Value, apiname: &str) -> bool {
    let Some(object) = item.as_object_mut() else {
        return false;
    };
    let name = js_trim(apiname).to_string();
    if name.is_empty() {
        return false;
    }
    let canonical = object
        .get("apiname")
        .map(js_string_value)
        .unwrap_or_default();
    if name == js_trim(&canonical) {
        return false;
    }
    if !object.get("aliases").map(Value::is_array).unwrap_or(false) {
        object.insert("aliases".into(), Value::Array(Vec::new()));
    }
    let Some(Value::Array(aliases)) = object.get_mut("aliases") else {
        return false;
    };
    if aliases
        .iter()
        .any(|alias| alias.as_str().map(|value| value == name).unwrap_or(false))
    {
        return false;
    }
    aliases.push(Value::String(name));
    true
}

/// Índice de busca de um appid: apinames/apelidos (case-insensitive) e títulos.
///
/// Os valores são posições no slice original. Título repetido vira `None`.
#[derive(Debug, Clone, Default)]
pub struct AchievementIndex {
    by_apiname: HashMap<String, usize>,
    by_title: HashMap<String, Option<usize>>,
}

impl AchievementIndex {
    pub fn by_apiname(&self, name: &str) -> Option<usize> {
        self.by_apiname.get(&name.to_lowercase()).copied()
    }

    /// `Some(Some(pos))` quando o título é único, `Some(None)` quando ambíguo.
    pub fn by_title(&self, title: &str) -> Option<Option<usize>> {
        self.by_title.get(&normalize_title(title)).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.by_apiname.is_empty() && self.by_title.is_empty()
    }
}

/// Constrói o índice de um appid (itens do `achievements.json`).
pub fn build_index(items: &[Value]) -> AchievementIndex {
    let mut index = AchievementIndex::default();
    for (position, item) in items.iter().enumerate() {
        if item.is_null() {
            continue;
        }
        for name in aliases_of(item) {
            index
                .by_apiname
                .entry(name.to_lowercase())
                .or_insert(position);
        }
        let title = normalize_title_value(item.get("title"));
        if title.is_empty() {
            continue;
        }
        match index.by_title.get_mut(&title) {
            Some(current) => *current = None,
            None => {
                index.by_title.insert(title, Some(position));
            }
        }
    }
    index
}

/// Acha a posição do item do appid por apiname exato → apelido → título único.
pub fn find_item_index(
    index: &AchievementIndex,
    items: &[Value],
    apiname: Option<&str>,
    title: Option<&str>,
) -> Option<usize> {
    if let Some(apiname) = apiname {
        let key = js_trim(apiname).to_lowercase();
        if !key.is_empty() {
            if let Some(position) = index.by_apiname(&key) {
                if items.get(position).is_some() {
                    return Some(position);
                }
            }
        }
    }
    let title = title.map(normalize_title).unwrap_or_default();
    if !title.is_empty() {
        if let Some(Some(position)) = index.by_title.get(&title) {
            if items.get(*position).is_some() {
                return Some(*position);
            }
        }
    }
    None
}

/// Igual a [`find_item_index`], devolvendo o item.
pub fn find_item<'a>(
    index: &AchievementIndex,
    items: &'a [Value],
    apiname: Option<&str>,
    title: Option<&str>,
) -> Option<&'a Value> {
    find_item_index(index, items, apiname, title).and_then(|position| items.get(position))
}

/// `^steam:(\d+)$` — devolve o appid (sem o prefixo).
pub fn extract_appid(value: &str) -> Option<&str> {
    let digits = value.strip_prefix("steam:")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(digits)
}

/// `extrairAppid(entry)` do watcher (`entry.id`).
pub fn extract_appid_value(entry: &Value) -> Option<String> {
    let id = entry.get("id")?;
    let text = match id {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    extract_appid(&text).map(str::to_string)
}

/// Resolve o appid de uma entrada de biblioteca (`entry.id = "steam:<id>"`).
pub fn resolve_appid(entry: &Value) -> Option<String> {
    extract_appid_value(entry)
}

/// Normaliza um appid vindo de configuração: aceita `440` ou `steam:440`.
pub fn normalize_appid(value: &str) -> Option<String> {
    let trimmed = js_trim(value);
    if !trimmed.is_empty() && trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(trimmed.to_string());
    }
    extract_appid(trimmed).map(str::to_string)
}

/// ID UPC/steam numérico: aceita número seguro >= 0 ou string de até 18
/// dígitos, sem zeros à esquerda.
pub fn normalize_numeric_id(value: &str) -> Option<String> {
    let text = js_trim(value);
    if text.is_empty() || text.len() > 18 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let stripped = text.trim_start_matches('0');
    if stripped.is_empty() {
        Some("0".to_string())
    } else {
        Some(stripped.to_string())
    }
}

/// Converte nomes internos Steam (`ACObsidian_Ach_40`) para o ID decimal.
///
/// Reproduz `/(?:^|[_-])ach(?:ievement)?[_-]?(\d+)$/i` seguido de
/// `normalizeNumericId`.
pub fn numeric_achievement_id(value: &str) -> Option<String> {
    let text = js_trim(value);
    if text.is_empty() {
        return None;
    }
    if let Some(direct) = normalize_numeric_id(text) {
        return Some(direct);
    }
    let bytes = text.as_bytes();
    let mut digits_start = bytes.len();
    while digits_start > 0 && bytes[digits_start - 1].is_ascii_digit() {
        digits_start -= 1;
    }
    if digits_start == bytes.len() {
        return None;
    }
    let prefix = &text[..digits_start];
    let digits = &text[digits_start..];
    if serial_prefix_matches(prefix) {
        normalize_numeric_id(digits)
    } else {
        None
    }
}

/// `String(value ?? "").trim()` para as funções de serial.
pub fn numeric_achievement_id_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        other => numeric_achievement_id(&js_string_value(other)),
    }
}

fn serial_prefix_matches(prefix: &str) -> bool {
    let lower = prefix.to_lowercase();
    let base = lower
        .strip_suffix('_')
        .or_else(|| lower.strip_suffix('-'))
        .unwrap_or(&lower);
    word_boundary_ach(base)
}

fn word_boundary_ach(value: &str) -> bool {
    for suffix in ["achievement", "ach"] {
        if let Some(head) = value.strip_suffix(suffix) {
            return head.is_empty() || head.ends_with('_') || head.ends_with('-');
        }
    }
    false
}

/// ID serial de uma conquista, na mesma ordem do `itemUplayId`:
/// campos explícitos primeiro, depois `apiname`/`name`.
pub fn item_serial_id(item: &Value) -> Option<String> {
    let object = item.as_object()?;
    for key in ["uplayId", "upcId", "objectiveId", "achievementId"] {
        if let Some(id) = object.get(key).and_then(numeric_achievement_id_value) {
            return Some(id);
        }
    }
    for key in ["apiname", "name"] {
        if let Some(id) = object.get(key).and_then(numeric_achievement_id_value) {
            return Some(id);
        }
    }
    None
}

/// Procura o item cujo serial numérico coincide com `id`.
pub fn find_by_serial(items: &[Value], id: &str) -> Option<usize> {
    let wanted = numeric_achievement_id(id)?;
    items.iter().position(|item| {
        item_serial_id(item)
            .map(|serial| serial == wanted)
            .unwrap_or(false)
    })
}

/// `typeof value === "object" && !Array.isArray(value)`.
pub fn is_plain_object(value: &Value) -> bool {
    matches!(value, Value::Object(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_title_ignores_accent_case_and_extra_spaces() {
        assert_eq!(normalize_title("  Pé de  Valsa "), "pe de valsa");
        assert_eq!(normalize_title("PE DE VALSA"), "pe de valsa");
        assert_eq!(normalize_title(""), "");
        assert_eq!(
            normalize_title("  Um Seletivo   Começo de Ano "),
            "um seletivo comeco de ano"
        );
    }

    #[test]
    fn aliases_of_dedupes_and_ignores_nulls_and_empty_values() {
        let item = json!({
            "apiname": " ACH01 ",
            "aliases": ["ach_01", null, "  ", "ACH01", "ach_01"]
        });
        assert_eq!(aliases_of(&item), ["ACH01", "ach_01"]);
        assert!(aliases_of(&json!(null)).is_empty());
        assert!(aliases_of(&json!("x")).is_empty());
    }

    #[test]
    fn remember_alias_appends_once_and_rejects_own_apiname() {
        let mut item = json!({ "apiname": "ACH01" });
        assert!(!remember_alias(&mut item, " ACH01 "));
        assert!(remember_alias(&mut item, "ach_01"));
        assert!(!remember_alias(&mut item, "ach_01"));
        assert_eq!(item["aliases"], json!(["ach_01"]));
        assert!(!remember_alias(&mut json!(null), "x"));
        let mut reset = json!({ "apiname": "A", "aliases": "not-array" });
        assert!(remember_alias(&mut reset, "B"));
        assert_eq!(reset["aliases"], json!(["B"]));
    }

    #[test]
    fn index_matches_apiname_aliases_case_insensitively() {
        let items = vec![
            json!({ "apiname": "ACH01", "title": "Um", "aliases": ["ach_01", "PFA_1"] }),
            json!({ "apiname": "ACH02", "title": "Dois" }),
        ];
        let index = build_index(&items);
        assert_eq!(
            find_item_index(&index, &items, Some("ach01"), None),
            Some(0)
        );
        assert_eq!(
            find_item_index(&index, &items, Some("pfa_1"), None),
            Some(0)
        );
        assert_eq!(
            find_item_index(&index, &items, Some("ACH02"), None),
            Some(1)
        );
    }

    #[test]
    fn unique_title_matches_but_duplicate_title_is_ambiguous() {
        let items = vec![
            json!({ "apiname": "a1", "title": "Mesma coisa" }),
            json!({ "apiname": "a2", "title": "Mesma coisa" }),
            json!({ "apiname": "a3", "title": "Pé de Valsa" }),
        ];
        let index = build_index(&items);
        assert_eq!(
            find_item_index(&index, &items, Some("desconhecido"), Some("Mesma coisa")),
            None
        );
        assert_eq!(
            find_item_index(&index, &items, Some("desconhecido"), Some("Pe de valsa")),
            Some(2)
        );
        assert_eq!(find_item_index(&index, &items, Some("a2"), None), Some(1));
        assert_eq!(find_item(&index, &items, None, Some("Outra")), None);
    }

    #[test]
    fn title_fallback_keeps_first_position_for_missing_aliases() {
        let items = vec![json!({ "apiname": null, "title": "Um" })];
        let index = build_index(&items);
        assert_eq!(
            find_item_index(&index, &items, Some("x"), Some("UM")),
            Some(0)
        );
        assert!(normalize_title_value(Some(&json!(12))) == "12");
    }

    #[test]
    fn appid_extraction_matches_the_steam_regex() {
        assert_eq!(extract_appid("steam:1091500"), Some("1091500"));
        assert_eq!(extract_appid("steam:"), None);
        assert_eq!(extract_appid("gog:1"), None);
        assert_eq!(extract_appid("steam:12a"), None);
        assert_eq!(
            extract_appid_value(&json!({ "id": "steam:440" })),
            Some("440".to_string())
        );
        assert_eq!(extract_appid_value(&json!({ "id": "steam:x" })), None);
        assert_eq!(normalize_appid("440"), Some("440".to_string()));
        assert_eq!(normalize_appid("steam:440"), Some("440".to_string()));
        assert_eq!(normalize_appid("steam:4x0"), None);
    }

    #[test]
    fn numeric_achievement_id_accepts_decimal_and_internal_names() {
        assert_eq!(numeric_achievement_id("40"), Some("40".to_string()));
        assert_eq!(numeric_achievement_id("040"), Some("40".to_string()));
        assert_eq!(numeric_achievement_id("000"), Some("0".to_string()));
        assert_eq!(
            numeric_achievement_id("ACObsidian_Ach_40"),
            Some("40".to_string())
        );
        assert_eq!(
            numeric_achievement_id("ACHIEVEMENT-12"),
            Some("12".to_string())
        );
        assert_eq!(numeric_achievement_id("foo_Ach_7"), Some("7".to_string()));
        assert_eq!(numeric_achievement_id("fooach_7"), None);
        assert_eq!(numeric_achievement_id("Ach"), None);
        assert_eq!(numeric_achievement_id("x-1"), None);
        assert_eq!(numeric_achievement_id("1234567890123456789"), None);
        assert_eq!(
            numeric_achievement_id_value(&json!("ACObsidian_Ach_1")),
            Some("1".to_string())
        );
        assert_eq!(numeric_achievement_id_value(&json!(null)), None);
    }

    #[test]
    fn item_serial_prefers_explicit_fields_and_finds_items() {
        let items = vec![
            json!({ "apiname": "ACObsidian_Ach_40", "title": "Quarenta" }),
            json!({ "apiname": "X", "title": "Setenta", "uplayId": "70" }),
            json!({ "apiname": "Sem serial", "title": "Nada" }),
        ];
        assert_eq!(item_serial_id(&items[1]), Some("70".to_string()));
        assert_eq!(find_by_serial(&items, "40"), Some(0));
        assert_eq!(find_by_serial(&items, "70"), Some(1));
        assert_eq!(find_by_serial(&items, "999"), None);
        assert_eq!(find_by_serial(&items, "abc"), None);
    }

    #[test]
    fn numeric_title_and_plain_object_helpers() {
        assert_eq!(normalize_title_value(Some(&json!(1.5))), "1.5");
        assert!(is_plain_object(&json!({})));
        assert!(!is_plain_object(&json!([])));
    }
}
