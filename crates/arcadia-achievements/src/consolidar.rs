//! Fusão de itens que são A MESMA conquista, sem descartar informação.
//!
//! Porte de `app/electron/achievements/consolidar.js`. O ponto central é que
//! `apiname` não é estável entre as fontes (bin real `PFA_1` x scrape sintético
//! `ach_01`): o loader casa por título e grava o vínculo em
//! `aliases`/`remoteApiname`, mas os dois itens continuam na lista. Aqui eles
//! viram um só.
//!
//! Regras de merge:
//! - só funde com MESMO título normalizado E vínculo explícito de apiname;
//! - sobrevive o item do schema real (`reais`); empate fica com o primeiro;
//! - `achieved` é OR (monotônico) e `unlock` usa o menor timestamp positivo;
//! - `aliases`/`remoteApiname` dos dois são unidos;
//! - campos desconhecidos (`localUnlock`, `revoked`, `hidden`, `removed`…)
//!   sobrevivem no item final.

use std::collections::HashSet;

use serde_json::{Map, Value};
use unicode_normalization::UnicodeNormalization;

use crate::js::{js_number_value, js_string_value, js_trim, js_truthy_value};

/// Normalização de título da consolidação (NFKD, como o JS).
pub fn normalizar(title: &str) -> String {
    let decomposed: String = title.nfkd().collect();
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

/// Apinames que identificam o item em qualquer espaço de chave.
pub fn apinames_de(item: &Value) -> HashSet<String> {
    let mut names = HashSet::new();
    let Some(object) = item.as_object() else {
        return names;
    };
    if let Some(name) = object.get("apiname").filter(|value| js_truthy_value(value)) {
        names.insert(js_string_value(name));
    }
    if let Some(name) = object
        .get("remoteApiname")
        .filter(|value| js_truthy_value(value))
    {
        names.insert(js_string_value(name));
    }
    if let Some(Value::Array(aliases)) = object.get("aliases") {
        for alias in aliases {
            if js_truthy_value(alias) {
                names.insert(js_string_value(alias));
            }
        }
    }
    names
}

/// O item tem vínculo explícito com o outro (um apiname em comum)?
pub fn ligados(left: &Value, right: &Value) -> bool {
    let left_names = apinames_de(left);
    apinames_de(right)
        .iter()
        .any(|name| left_names.contains(name))
}

/// Funde dois itens; o primeiro é o sobrevivente. Nunca descarta campos
/// desconhecidos do sobrevivente.
pub fn fundir(survivor: &Value, other: &Value) -> Value {
    let mut output = survivor.as_object().cloned().unwrap_or_else(Map::new);
    // Campos que só o outro item conhece (hidden/removed/localUnlock/...)
    // também sobrevivem; em conflito, o sobrevivente vence.
    if let Some(other_object) = other.as_object() {
        for (key, value) in other_object {
            if !output.contains_key(key) {
                output.insert(key.clone(), value.clone());
            }
        }
    }

    let achieved = survivor
        .get("achieved")
        .map(js_truthy_value)
        .unwrap_or(false)
        || other.get("achieved").map(js_truthy_value).unwrap_or(false);
    output.insert("achieved".into(), Value::Bool(achieved));

    // Marcadores de apresentação/remoção são monotônicos: se qualquer lado
    // diz que a conquista é oculta/removida, o item consolidado também diz.
    for field in ["hidden", "removed"] {
        let flagged = |value: &Value| value.get(field) == Some(&Value::Bool(true));
        if flagged(survivor) || flagged(other) {
            output.insert(field.into(), Value::Bool(true));
        }
    }

    let unlock = earliest_positive_unlock(survivor, other).unwrap_or(0);
    output.insert("unlock".into(), Value::from(unlock));

    let percent = survivor
        .get("percent")
        .and_then(js_number_value)
        .filter(|value| value.is_finite() && *value != 0.0)
        .or_else(|| {
            other
                .get("percent")
                .and_then(js_number_value)
                .filter(|value| value.is_finite() && *value != 0.0)
        })
        .unwrap_or(0.0);
    output.insert("percent".into(), number_value(percent));

    for field in ["icon", "icongray", "desc"] {
        let value = survivor
            .get(field)
            .filter(|value| js_truthy_value(value))
            .or_else(|| other.get(field).filter(|value| js_truthy_value(value)))
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()));
        output.insert(field.into(), value);
    }

    if !output
        .get("remoteApiname")
        .map(js_truthy_value)
        .unwrap_or(false)
    {
        if let Some(remote) = other
            .get("remoteApiname")
            .filter(|value| js_truthy_value(value))
        {
            output.insert("remoteApiname".into(), remote.clone());
        }
    }

    let mut aliases: Vec<String> = Vec::new();
    let mut add_alias = |value: &str| {
        let name = js_trim(value).to_string();
        if !name.is_empty() && !aliases.contains(&name) {
            aliases.push(name);
        }
    };
    if let Some(Value::Array(values)) = survivor.get("aliases") {
        for value in values {
            if js_truthy_value(value) {
                add_alias(&js_string_value(value));
            }
        }
    }
    if let Some(Value::Array(values)) = other.get("aliases") {
        for value in values {
            if js_truthy_value(value) {
                add_alias(&js_string_value(value));
            }
        }
    }
    if let Some(other_apiname) = other.get("apiname").filter(|value| js_truthy_value(value)) {
        let other_apiname = js_string_value(other_apiname);
        let same = output
            .get("apiname")
            .map(|value| js_string_value(value) == other_apiname)
            .unwrap_or(false);
        if !same {
            add_alias(&other_apiname);
        }
    }
    if let Some(canonical) = output.get("apiname").filter(|value| js_truthy_value(value)) {
        let canonical = js_string_value(canonical);
        aliases.retain(|alias| alias != &canonical);
    }
    if let Some(remote) = output
        .get("remoteApiname")
        .filter(|value| js_truthy_value(value))
    {
        let remote = js_string_value(remote);
        aliases.retain(|alias| alias != &remote);
    }
    if aliases.is_empty() {
        output.remove("aliases");
    } else {
        output.insert(
            "aliases".into(),
            Value::Array(aliases.into_iter().map(Value::String).collect()),
        );
    }

    Value::Object(output)
}

/// `consolidarItens`: a mesma lista, sem duplicatas de conquista, na mesma
/// ordem. `reais` são os apinames vindos do schema de verdade.
pub fn consolidar_items(items: Vec<Value>, reais: &HashSet<String>) -> Vec<Value> {
    consolidar_items_with_report(items, reais).0
}

/// Igual a [`consolidar_items`], devolvendo também quantos itens foram fundidos.
pub fn consolidar_items_with_report(
    items: Vec<Value>,
    reais: &HashSet<String>,
) -> (Vec<Value>, usize) {
    let mut output: Vec<Value> = Vec::new();
    let mut merged = 0usize;

    for item in items {
        let title = item
            .get("title")
            .filter(|value| js_truthy_value(value))
            .map(|value| normalizar(&js_string_value(value)))
            .unwrap_or_default();
        let target = if title.is_empty() {
            None
        } else {
            output.iter().position(|candidate| {
                candidate
                    .get("title")
                    .filter(|value| js_truthy_value(value))
                    .map(|value| normalizar(&js_string_value(value)) == title)
                    .unwrap_or(false)
                    && ligados(candidate, &item)
            })
        };

        let Some(position) = target else {
            output.push(item);
            continue;
        };

        let target_is_real = output[position]
            .get("apiname")
            .map(|value| reais.contains(&js_string_value(value)))
            .unwrap_or(false);
        let item_is_real = item
            .get("apiname")
            .map(|value| reais.contains(&js_string_value(value)))
            .unwrap_or(false);
        output[position] = if item_is_real && !target_is_real {
            fundir(&item, &output[position])
        } else {
            fundir(&output[position], &item)
        };
        merged += 1;
    }

    (output, merged)
}

fn earliest_positive_unlock(survivor: &Value, other: &Value) -> Option<i64> {
    let values = [
        survivor.get("unlock").and_then(positive_number),
        other.get("unlock").and_then(positive_number),
    ];
    values
        .into_iter()
        .flatten()
        .min_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
}

fn positive_number(value: &Value) -> Option<i64> {
    let number = js_number_value(value)?;
    if !number.is_finite() || number <= 0.0 {
        return None;
    }
    Some(number.trunc() as i64)
}

fn number_value(number: f64) -> Value {
    if number.is_finite() && number.fract() == 0.0 {
        Value::from(number as i64)
    } else {
        serde_json::Number::from_f64(number)
            .map(Value::Number)
            .unwrap_or_else(|| Value::from(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reais(values: &[&str]) -> HashSet<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn normalizar_ignores_accent_case_and_extra_spaces() {
        assert_eq!(
            normalizar("  Um Seletivo   Começo de Ano "),
            "um seletivo comeco de ano"
        );
        assert_eq!(normalizar("Leite Derramado"), "leite derramado");
        assert_eq!(normalizar(""), "");
    }

    #[test]
    fn hogwarts_case_47_items_become_45_and_unlocks_do_not_double_count() {
        let items = vec![
            json!({ "apiname": "ach_01", "title": "Um Seletivo Começo de Ano", "achieved": true, "aliases": ["PFA_1"] }),
            json!({ "apiname": "PFA_1", "title": "Um Seletivo Começo de Ano", "block": 1, "bit": 0, "achieved": true }),
            json!({ "apiname": "PFA_33", "title": "Leite Derramado", "block": 2, "bit": 0, "achieved": true }),
            json!({ "apiname": "ach_37", "title": "Leite Derramado", "block": 36, "bit": 0, "achieved": true, "remoteApiname": "PFA_33" }),
            json!({ "apiname": "PFA_2", "title": "Outra conquista", "block": 1, "bit": 1, "achieved": true }),
            json!({ "apiname": "PFA_3", "title": "Mais uma", "block": 1, "bit": 2, "achieved": false }),
        ];
        let out = consolidar_items(items, &reais(&["PFA_1", "PFA_2", "PFA_3", "PFA_33"]));
        assert_eq!(out.len(), 4);
        assert_eq!(
            out.iter()
                .filter(|item| item["achieved"] == json!(true))
                .count(),
            3
        );
    }

    #[test]
    fn real_schema_item_survives_and_inherits_from_the_other() {
        let items = vec![
            json!({ "apiname": "ach_01", "title": "X", "achieved": true, "unlock": 111, "aliases": ["PFA_1"], "icon": "a" }),
            json!({ "apiname": "PFA_1", "title": "X", "block": 1, "bit": 0, "achieved": false, "unlock": 0, "percent": 7 }),
        ];
        let out = consolidar_items(items, &reais(&["PFA_1"]));
        assert_eq!(out.len(), 1);
        let item = &out[0];
        assert_eq!(item["apiname"], json!("PFA_1"));
        assert_eq!(item["block"], json!(1));
        assert_eq!(item["achieved"], json!(true));
        assert_eq!(item["unlock"], json!(111));
        assert_eq!(item["percent"], json!(7));
        assert_eq!(item["icon"], json!("a"));
        assert_eq!(item["aliases"], json!(["ach_01"]));
    }

    #[test]
    fn earliest_positive_unlock_wins_and_negative_or_zero_are_ignored() {
        let items = vec![
            json!({ "apiname": "A", "title": "X", "achieved": true, "unlock": 200 }),
            json!({ "apiname": "B", "title": "X", "achieved": true, "unlock": 100, "aliases": ["A"] }),
        ];
        let out = consolidar_items(items, &reais(&["B"]));
        assert_eq!(out[0]["unlock"], json!(100));

        let items = vec![
            json!({ "apiname": "A", "title": "Y", "achieved": true, "unlock": -5 }),
            json!({ "apiname": "B", "title": "Y", "achieved": true, "unlock": 0, "aliases": ["A"] }),
        ];
        let out = consolidar_items(items, &reais(&["B"]));
        assert_eq!(out[0]["unlock"], json!(0));
    }

    #[test]
    fn does_not_merge_equal_title_without_apiname_link() {
        let items = vec![
            json!({ "apiname": "A", "title": "Complete the game", "achieved": true }),
            json!({ "apiname": "B", "title": "Complete the game", "achieved": false }),
        ];
        assert_eq!(consolidar_items(items, &HashSet::new()).len(), 2);
    }

    #[test]
    fn does_not_merge_linked_apiname_with_different_titles() {
        let items = vec![
            json!({ "apiname": "A", "title": "Primeira", "aliases": ["B"] }),
            json!({ "apiname": "B", "title": "Segunda" }),
        ];
        assert_eq!(consolidar_items(items, &HashSet::new()).len(), 2);
    }

    #[test]
    fn list_without_duplicates_passes_through_in_order() {
        let items = vec![
            json!({ "apiname": "PFA_1", "title": "Um", "block": 0, "bit": 0, "achieved": true }),
            json!({ "apiname": "PFA_2", "title": "Dois", "block": 0, "bit": 1, "achieved": false }),
        ];
        let out = consolidar_items(items, &reais(&["PFA_1", "PFA_2"]));
        assert_eq!(
            out.iter()
                .map(|item| item["apiname"].clone())
                .collect::<Vec<_>>(),
            vec![json!("PFA_1"), json!("PFA_2")]
        );
    }

    #[test]
    fn ligados_recognizes_alias_remote_apiname_and_exact_name() {
        assert!(ligados(
            &json!({ "apiname": "a", "aliases": ["b"] }),
            &json!({ "apiname": "b" })
        ));
        assert!(ligados(
            &json!({ "apiname": "a" }),
            &json!({ "apiname": "x", "remoteApiname": "a" })
        ));
        assert!(ligados(
            &json!({ "apiname": "a" }),
            &json!({ "apiname": "a" })
        ));
        assert!(!ligados(
            &json!({ "apiname": "a" }),
            &json!({ "apiname": "b" })
        ));
    }

    #[test]
    fn unknown_flags_hidden_and_removed_survive_the_merge() {
        let items = vec![
            json!({ "apiname": "ach_1", "title": "X", "achieved": true, "hidden": true, "removed": false, "localUnlock": true, "aliases": ["PFA_1"] }),
            json!({ "apiname": "PFA_1", "title": "X", "block": 1, "bit": 0, "achieved": false, "removed": true }),
        ];
        let out = consolidar_items(items, &reais(&["PFA_1"]));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["hidden"], json!(true));
        assert_eq!(out[0]["removed"], json!(true), "hidden/removed são OR");
        assert_eq!(out[0]["localUnlock"], json!(true));
        assert_eq!(out[0]["apiname"], json!("PFA_1"));
    }

    #[test]
    fn report_counts_merged_items() {
        let items = vec![
            json!({ "apiname": "A", "title": "X", "aliases": ["B"] }),
            json!({ "apiname": "B", "title": "X" }),
        ];
        let (out, merged) = consolidar_items_with_report(items, &HashSet::new());
        assert_eq!(out.len(), 1);
        assert_eq!(merged, 1);
    }
}
