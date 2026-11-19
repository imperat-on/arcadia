//! Construtores de fixtures binárias usados apenas pelos testes da crate.

use crate::steam_bin::{KV_OBJECT, KV_OBJECT_END, KV_STRING};

fn name(value: &str) -> Vec<u8> {
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

fn string(name_value: &str, value: &str) -> Vec<u8> {
    let mut bytes = vec![KV_STRING];
    bytes.extend(name(name_value));
    bytes.extend_from_slice(value.as_bytes());
    bytes.push(0);
    bytes
}

fn object(name_value: &str, children: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![KV_OBJECT];
    bytes.extend(name(name_value));
    for child in children {
        bytes.extend_from_slice(child);
    }
    bytes.push(KV_OBJECT_END);
    bytes
}

/// `(block, bit, apiname, [(idioma, título)])`.
pub type SchemaItem<'a> = (&'a str, &'a str, &'a str, &'a [(&'a str, &'a str)]);

/// Monta um `UserGameStatsSchema_*.bin` mínimo.
pub fn schema_bin(items: &[SchemaItem<'_>]) -> Vec<u8> {
    schema_bin_hidden(items, &[])
}

/// Igual a [`schema_bin`], marcando `hidden=1` nas chaves `(block, bit)`
/// listadas (usado para o payload do evento carregar `hidden`).
pub fn schema_bin_hidden(items: &[SchemaItem<'_>], hidden: &[(&str, &str)]) -> Vec<u8> {
    let mut blocks: Vec<(&str, Vec<Vec<u8>>)> = Vec::new();
    for (block, bit, apiname, titles) in items {
        let mut bit_children = vec![
            string("name", apiname),
            object(
                "display",
                &[object(
                    "name",
                    &titles
                        .iter()
                        .map(|(language, title)| string(language, title))
                        .collect::<Vec<_>>(),
                )],
            ),
        ];
        if hidden
            .iter()
            .any(|(hidden_block, hidden_bit)| hidden_block == block && hidden_bit == bit)
        {
            bit_children.push(string("hidden", "1"));
        }
        let bit_object = object(bit, &bit_children);
        match blocks.iter_mut().find(|(candidate, _)| candidate == block) {
            Some((_, children)) => children.push(bit_object),
            None => blocks.push((block, vec![bit_object])),
        }
    }
    let block_objects: Vec<Vec<u8>> = blocks
        .into_iter()
        .map(|(block, bits)| object(block, &[object("bits", &bits)]))
        .collect();
    object("", &[object("stats", &block_objects)])
}

/// Monta um `UserGameStats_<steamid>_<appid>.bin` mínimo (`data` +
/// `AchievementTimes`).
pub fn progress_bin(entries: &[(&str, &str, i64)]) -> Vec<u8> {
    let mut blocks: Vec<(&str, Vec<Vec<u8>>)> = Vec::new();
    for (block, bit, timestamp) in entries {
        let time = string(bit, &timestamp.to_string());
        match blocks.iter_mut().find(|(candidate, _)| candidate == block) {
            Some((_, children)) => children.push(time),
            None => blocks.push((block, vec![time])),
        }
    }
    let block_objects: Vec<Vec<u8>> = blocks
        .into_iter()
        .map(|(block, times)| {
            object(
                block,
                &[string("data", "1"), object("AchievementTimes", &times)],
            )
        })
        .collect();
    object("", &block_objects)
}
