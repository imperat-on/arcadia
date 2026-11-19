//! Parser do KeyValues **binário** da Valve usado pelos arquivos
//! `UserGameStatsSchema_<appid>.bin` e `UserGameStats_<steamid>_<appid>.bin`.
//!
//! Porte fiel de `readKv`/`loadKvBin`/`progressMap` de
//! `app/electron/achievements/steam_bin.js`. Os únicos tipos que o cliente
//! Electron interpreta são reproduzidos aqui:
//!
//! | tag  | tipo          |
//! |------|---------------|
//! | 0x00 | objeto        |
//! | 0x01 | string (NUL)  |
//! | 0x02 | int32 LE      |
//! | 0x07 | uint64 LE     |
//! | 0x0a | int64 LE      |
//! | 0x0b | float32 LE    |
//! | 0x08 | fim de objeto |
//!
//! O parser é tolerante a arquivos truncados: sempre devolve [`KvError`], nunca
//! entra em pânico nem lê fora do buffer. O cliente JS não tem fallback de
//! schema em VDF de texto; essa variante não existe no oráculo congelado.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use crate::js::{js_number_str, js_number_to_string, js_to_int32};

pub const KV_OBJECT: u8 = 0x00;
pub const KV_STRING: u8 = 0x01;
pub const KV_INT32: u8 = 0x02;
pub const KV_UINT64: u8 = 0x07;
pub const KV_OBJECT_END: u8 = 0x08;
pub const KV_INT64: u8 = 0x0a;
pub const KV_FLOAT32: u8 = 0x0b;

/// Limite de aninhamento. Arquivos reais ficam muito abaixo; o limite evita
/// estouro de pilha em entrada corrompida.
pub const MAX_DEPTH: usize = 512;

/// Valor de uma chave KeyValues binária.
#[derive(Debug, Clone, PartialEq)]
pub enum KvValue {
    Object(KvObject),
    String(String),
    Int32(i32),
    UInt64(u64),
    Int64(i64),
    Float32(f32),
}

impl KvValue {
    pub fn as_object(&self) -> Option<&KvObject> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// `String(valor)` no espaço de valores do parser (números viram texto).
    pub fn js_string(&self) -> String {
        match self {
            Self::Object(_) => "[object Object]".to_string(),
            Self::String(value) => value.clone(),
            Self::Int32(value) => value.to_string(),
            Self::UInt64(value) => value.to_string(),
            Self::Int64(value) => value.to_string(),
            Self::Float32(value) => {
                if value.fract() == 0.0 {
                    format!("{value:.0}")
                } else {
                    value.to_string()
                }
            }
        }
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }
}

/// Objeto KeyValues com ordem de inserção preservada (como o objeto JS).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KvObject {
    entries: Vec<(String, KvValue)>,
}

impl KvObject {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&KvValue> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut KvValue> {
        self.entries
            .iter_mut()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    pub fn get_object(&self, key: &str) -> Option<&KvObject> {
        self.get(key)?.as_object()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `obj[chave] = valor`: substitui no lugar, ou acrescenta no fim.
    pub fn insert(&mut self, key: impl Into<String>, value: KvValue) -> Option<KvValue> {
        let key = key.into();
        match self
            .entries
            .iter_mut()
            .find(|(candidate, _)| *candidate == key)
        {
            Some((_, current)) => Some(std::mem::replace(current, value)),
            None => {
                self.entries.push((key, value));
                None
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &KvValue)> + '_ {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.iter().map(|(key, _)| key.as_str())
    }
}

/// Erro de leitura de um arquivo KeyValues binário.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvError {
    UnexpectedEof { position: usize },
    MissingTerminator { position: usize },
    UnknownType { position: usize, type_id: u8 },
    DepthLimit { position: usize },
    RootNotObject { type_id: u8 },
}

impl fmt::Display for KvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof { position } => {
                write!(formatter, "KV binário truncado na posição {position}")
            }
            Self::MissingTerminator { position } => {
                write!(
                    formatter,
                    "string KV sem terminador NUL na posição {position}"
                )
            }
            Self::UnknownType { position, type_id } => {
                write!(
                    formatter,
                    "tipo KV desconhecido {type_id:#04x} na posição {position}"
                )
            }
            Self::DepthLimit { position } => {
                write!(
                    formatter,
                    "aninhamento KV profundo demais na posição {position}"
                )
            }
            Self::RootNotObject { type_id } => {
                write!(formatter, "raiz KV não é objeto (tipo {type_id:#04x})")
            }
        }
    }
}

impl std::error::Error for KvError {}

/// Lê a raiz completa e devolve `(nome, valor, próxima posição)`.
///
/// O nome da raiz costuma ser vazio nos arquivos da Steam.
pub fn read_kv(
    bytes: &[u8],
    position: usize,
    depth: usize,
) -> Result<(String, KvValue, usize), KvError> {
    if depth > MAX_DEPTH {
        return Err(KvError::DepthLimit { position });
    }
    let type_position = position;
    let mut cursor = position;
    let type_id = *bytes
        .get(cursor)
        .ok_or(KvError::UnexpectedEof { position: cursor })?;
    cursor += 1;
    let end = find_nul(bytes, cursor)?;
    let name = String::from_utf8_lossy(&bytes[cursor..end]).into_owned();
    cursor = end + 1;

    match type_id {
        KV_OBJECT => {
            let mut object = KvObject::new();
            loop {
                let byte = *bytes
                    .get(cursor)
                    .ok_or(KvError::UnexpectedEof { position: cursor })?;
                if byte == KV_OBJECT_END {
                    cursor += 1;
                    break;
                }
                let (child_name, child_value, next) = read_kv(bytes, cursor, depth + 1)?;
                object.insert(child_name, child_value);
                cursor = next;
            }
            Ok((name, KvValue::Object(object), cursor))
        }
        KV_STRING => {
            let end = find_nul(bytes, cursor)?;
            let value = String::from_utf8_lossy(&bytes[cursor..end]).into_owned();
            Ok((name, KvValue::String(value), end + 1))
        }
        KV_INT32 => {
            let value = read_i32(bytes, cursor)?;
            Ok((name, KvValue::Int32(value), cursor + 4))
        }
        KV_UINT64 => {
            let value = read_array::<8>(bytes, cursor)?;
            Ok((name, KvValue::UInt64(u64::from_le_bytes(value)), cursor + 8))
        }
        KV_INT64 => {
            let value = read_array::<8>(bytes, cursor)?;
            Ok((name, KvValue::Int64(i64::from_le_bytes(value)), cursor + 8))
        }
        KV_FLOAT32 => {
            let value = read_array::<4>(bytes, cursor)?;
            Ok((
                name,
                KvValue::Float32(f32::from_le_bytes(value)),
                cursor + 4,
            ))
        }
        other => Err(KvError::UnknownType {
            position: type_position,
            type_id: other,
        }),
    }
}

/// Lê o primeiro valor do buffer. Devolve o objeto raiz.
pub fn parse_binary(bytes: &[u8]) -> Result<KvObject, KvError> {
    let (_, value, _) = read_kv(bytes, 0, 0)?;
    match value {
        KvValue::Object(object) => Ok(object),
        other => Err(KvError::RootNotObject {
            type_id: match other {
                KvValue::String(_) => KV_STRING,
                KvValue::Int32(_) => KV_INT32,
                KvValue::UInt64(_) => KV_UINT64,
                KvValue::Int64(_) => KV_INT64,
                KvValue::Float32(_) => KV_FLOAT32,
                KvValue::Object(_) => KV_OBJECT,
            },
        }),
    }
}

/// Equivalente a `loadKvBin`: lê o arquivo e devolve o primeiro valor.
/// Qualquer falha (arquivo ausente, truncado, tipo desconhecido) vira `None`.
pub fn load_kv_bin(path: impl AsRef<Path>) -> Option<KvValue> {
    let bytes = fs::read(path).ok()?;
    read_kv(&bytes, 0, 0).ok().map(|(_, value, _)| value)
}

/// Igual a [`load_kv_bin`], mas exige que a raiz seja um objeto.
pub fn load_binary_file(path: impl AsRef<Path>) -> Option<KvObject> {
    load_kv_bin(path)?.as_object().cloned()
}

pub fn parse_binary_object(bytes: &[u8]) -> Option<KvObject> {
    match read_kv(bytes, 0, 0) {
        Ok((_, KvValue::Object(object), _)) => Some(object),
        _ => None,
    }
}

fn kv_number(value: &KvValue) -> Option<f64> {
    match value {
        KvValue::Int32(number) => Some(f64::from(*number)),
        KvValue::UInt64(number) => Some(*number as f64),
        KvValue::Int64(number) => Some(*number as f64),
        KvValue::Float32(number) => Some(f64::from(*number)),
        KvValue::String(text) => js_number_str(text),
        KvValue::Object(_) => None,
    }
}

/// `(block|bit) -> epoch em SEGUNDOS` lido do bin de progresso.
///
/// Reproduz `progressMap` do JS:
/// - formato novo: bloco com `data` + `AchievementTimes` (`índice -> ts`);
/// - formato antigo: `bits` (`índice -> { bits, unlock_time }`) quando o bit 0
///   está setado;
/// - `AchievementTimes` tem prioridade sobre `bits` (o JS só insere o bit se a
///   chave ainda não existir).
pub fn progress_map(kv: &KvObject) -> BTreeMap<String, i64> {
    let mut output = BTreeMap::new();
    for (block, value) in kv.iter() {
        let Some(block_object) = value.as_object() else {
            continue;
        };
        // O JS exige `bval.data !== undefined`.
        if !block_object.contains_key("data") {
            continue;
        }
        if let Some(times) = block_object.get_object("AchievementTimes") {
            for (index, timestamp) in times.iter() {
                let seconds = kv_number(timestamp)
                    .filter(|number| number.is_finite())
                    .map(|number| number.trunc() as i64)
                    .unwrap_or(0);
                output.insert(format!("{block}|{index}"), seconds);
            }
        }
        if let Some(bits) = block_object.get_object("bits") {
            for (index, info) in bits.iter() {
                let Some(info) = info.as_object() else {
                    continue;
                };
                let bit_value = info.get("bits").and_then(kv_number).unwrap_or(0.0);
                if js_to_int32(bit_value) & 1 == 0 {
                    continue;
                }
                let key = format!("{block}|{index}");
                if output.contains_key(&key) {
                    continue;
                }
                let unlock = info
                    .get("unlock_time")
                    .and_then(kv_number)
                    .filter(|number| number.is_finite())
                    .map(|number| number.trunc() as i64)
                    .unwrap_or(0);
                output.insert(key, unlock);
            }
        }
    }
    output
}

/// Igual a [`progress_map`], a partir dos bytes do arquivo.
pub fn progress_map_bytes(bytes: &[u8]) -> BTreeMap<String, i64> {
    match read_kv(bytes, 0, 0) {
        Ok((_, KvValue::Object(object), _)) => progress_map(&object),
        _ => BTreeMap::new(),
    }
}

/// Lê o bin de progresso de um caminho; ausente/inválido devolve mapa vazio.
pub fn load_progress_map(path: impl AsRef<Path>) -> BTreeMap<String, i64> {
    match load_kv_bin(path) {
        Some(KvValue::Object(object)) => progress_map(&object),
        _ => BTreeMap::new(),
    }
}

/// Converte a chave `(block, bit)` para o formato usado em todo o app.
pub fn block_bit_key(block: &str, bit: &str) -> String {
    format!("{block}|{bit}")
}

/// `String(block) + "|" + String(bit)` a partir dos valores JSON.
pub fn json_block_bit_key(block: &serde_json::Value, bit: &serde_json::Value) -> Option<String> {
    let block = match block {
        serde_json::Value::Number(number) => js_number_to_string(number),
        serde_json::Value::String(text) => text.clone(),
        _ => return None,
    };
    let bit = match bit {
        serde_json::Value::Number(number) => js_number_to_string(number),
        serde_json::Value::String(text) => text.clone(),
        _ => return None,
    };
    Some(format!("{block}|{bit}"))
}

fn find_nul(bytes: &[u8], from: usize) -> Result<usize, KvError> {
    bytes
        .get(from..)
        .ok_or(KvError::UnexpectedEof { position: from })?
        .iter()
        .position(|byte| *byte == 0)
        .map(|offset| from + offset)
        .ok_or(KvError::MissingTerminator { position: from })
}

fn read_array<const N: usize>(bytes: &[u8], position: usize) -> Result<[u8; N], KvError> {
    bytes
        .get(position..position + N)
        .ok_or(KvError::UnexpectedEof { position })?
        .try_into()
        .map_err(|_| KvError::UnexpectedEof { position })
}

fn read_i32(bytes: &[u8], position: usize) -> Result<i32, KvError> {
    read_array::<4>(bytes, position).map(i32::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn int32(name_value: &str, value: i32) -> Vec<u8> {
        let mut bytes = vec![KV_INT32];
        bytes.extend(name(name_value));
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes
    }

    fn uint64(name_value: &str, value: u64) -> Vec<u8> {
        let mut bytes = vec![KV_UINT64];
        bytes.extend(name(name_value));
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes
    }

    fn int64(name_value: &str, value: i64) -> Vec<u8> {
        let mut bytes = vec![KV_INT64];
        bytes.extend(name(name_value));
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes
    }

    fn float32(name_value: &str, value: f32) -> Vec<u8> {
        let mut bytes = vec![KV_FLOAT32];
        bytes.extend(name(name_value));
        bytes.extend_from_slice(&value.to_le_bytes());
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

    fn root(children: &[Vec<u8>]) -> Vec<u8> {
        object("", children)
    }

    #[test]
    fn parses_every_tag_the_js_parser_handles() {
        let bytes = root(&[
            string("str", "olá"),
            int32("i32", -7),
            uint64("u64", 9_000_000_000),
            int64("i64", -9_000_000_000),
            float32("f32", 1.5),
            object("child", &[string("nested", "v")]),
        ]);
        let parsed = parse_binary(&bytes).unwrap();
        assert_eq!(parsed.get_str("str"), Some("olá"));
        assert_eq!(parsed.get("i32"), Some(&KvValue::Int32(-7)));
        assert_eq!(parsed.get("u64"), Some(&KvValue::UInt64(9_000_000_000)));
        assert_eq!(parsed.get("i64"), Some(&KvValue::Int64(-9_000_000_000)));
        assert_eq!(parsed.get("f32"), Some(&KvValue::Float32(1.5)));
        assert_eq!(
            parsed.get_object("child").unwrap().get_str("nested"),
            Some("v")
        );
    }

    #[test]
    fn duplicate_keys_keep_the_js_last_value_and_first_position() {
        let bytes = root(&[string("a", "1"), string("b", "2"), string("a", "3")]);
        let parsed = parse_binary(&bytes).unwrap();
        assert_eq!(parsed.get_str("a"), Some("3"));
        let keys: Vec<_> = parsed.keys().collect();
        assert_eq!(keys, ["a", "b"]);
    }

    #[test]
    fn truncated_inputs_return_errors_instead_of_panicking() {
        assert!(matches!(
            read_kv(&[], 0, 0),
            Err(KvError::UnexpectedEof { .. })
        ));
        // Tipo 0x01 sem NUL no nome.
        assert!(matches!(
            read_kv(&[KV_STRING, b'x'], 0, 0),
            Err(KvError::MissingTerminator { .. })
        ));
        // Objeto sem o 0x08 final.
        let mut bytes = vec![KV_OBJECT];
        bytes.extend(name("x"));
        assert!(matches!(
            read_kv(&bytes, 0, 0),
            Err(KvError::UnexpectedEof { .. })
        ));
        // int32 com apenas 2 bytes.
        assert!(matches!(
            read_kv(&[KV_INT32, 0, 1, 2], 0, 0),
            Err(KvError::UnexpectedEof { .. })
        ));
        // string de valor sem terminador.
        let mut bytes = vec![KV_STRING];
        bytes.extend(name("k"));
        bytes.extend_from_slice(b"sem-fim");
        assert!(matches!(
            read_kv(&bytes, 0, 0),
            Err(KvError::MissingTerminator { .. })
        ));
    }

    #[test]
    fn unknown_tag_is_reported_with_position() {
        let error = read_kv(&[0x55, 0], 0, 0).unwrap_err();
        assert_eq!(
            error,
            KvError::UnknownType {
                position: 0,
                type_id: 0x55
            }
        );
        assert!(error.to_string().contains("desconhecido"));
    }

    #[test]
    fn root_must_be_an_object_for_schema_and_progress() {
        let bytes = string("", "not-an-object");
        assert!(matches!(
            parse_binary(&bytes),
            Err(KvError::RootNotObject { .. })
        ));
        assert!(progress_map_bytes(&bytes).is_empty());
    }

    #[test]
    fn absurd_nesting_stops_at_the_depth_limit() {
        let mut bytes = Vec::new();
        for _ in 0..(MAX_DEPTH + 4) {
            bytes.push(KV_OBJECT);
            bytes.push(0);
        }
        assert!(matches!(
            read_kv(&bytes, 0, 0),
            Err(KvError::DepthLimit { .. })
        ));
    }

    #[test]
    fn progress_map_reads_the_new_achievement_times_format() {
        let block = object(
            "1",
            &[
                string("data", "x"),
                object(
                    "AchievementTimes",
                    &[string("0", "1700000000"), string("2", "0")],
                ),
            ],
        );
        let kv = parse_binary(&root(&[block])).unwrap();
        let map = progress_map(&kv);
        assert_eq!(map.get("1|0"), Some(&1_700_000_000));
        assert_eq!(map.get("1|2"), Some(&0));
    }

    #[test]
    fn progress_map_reads_the_old_bits_format_and_skips_unset_bits() {
        let block = object(
            "2",
            &[
                string("data", "x"),
                object(
                    "bits",
                    &[
                        object(
                            "0",
                            &[int32("bits", 1), int32("unlock_time", 1_600_000_000)],
                        ),
                        object(
                            "1",
                            &[int32("bits", 0), int32("unlock_time", 1_600_000_001)],
                        ),
                        object(
                            "2",
                            &[int32("bits", 3), int32("unlock_time", 1_600_000_002)],
                        ),
                    ],
                ),
            ],
        );
        let map = progress_map(&parse_binary(&root(&[block])).unwrap());
        assert_eq!(map.get("2|0"), Some(&1_600_000_000));
        assert!(!map.contains_key("2|1"));
        assert_eq!(map.get("2|2"), Some(&1_600_000_002));
    }

    #[test]
    fn achievement_times_wins_over_bits_for_the_same_key() {
        let block = object(
            "1",
            &[
                string("data", "x"),
                object("AchievementTimes", &[string("0", "111")]),
                object(
                    "bits",
                    &[object("0", &[int32("bits", 1), int32("unlock_time", 222)])],
                ),
            ],
        );
        let map = progress_map(&parse_binary(&root(&[block])).unwrap());
        assert_eq!(map.get("1|0"), Some(&111));
    }

    #[test]
    fn blocks_without_data_or_non_objects_are_ignored() {
        let kv = parse_binary(&root(&[
            object("1", &[object("AchievementTimes", &[string("0", "10")])]),
            string("2", "no-data"),
            object("3", &[object("bits", &[object("0", &[int32("bits", 1)])])]),
        ]))
        .unwrap();
        assert!(progress_map(&kv).is_empty());
    }

    #[test]
    fn negative_and_non_numeric_timestamps_become_zero_like_number_or_zero() {
        let block = object(
            "1",
            &[
                string("data", "x"),
                object(
                    "AchievementTimes",
                    &[string("0", "-5"), string("1", "abc"), string("2", "")],
                ),
            ],
        );
        let map = progress_map(&parse_binary(&root(&[block])).unwrap());
        assert_eq!(map.get("1|0"), Some(&-5));
        assert_eq!(map.get("1|1"), Some(&0));
        assert_eq!(map.get("1|2"), Some(&0));
    }

    #[test]
    fn load_helpers_tolerate_missing_and_corrupt_files() {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-steam-bin-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.bin");
        fs::write(&good, root(&[string("k", "v")])).unwrap();
        assert_eq!(load_binary_file(&good).unwrap().get_str("k"), Some("v"));
        let bad = dir.join("bad.bin");
        fs::write(&bad, [KV_OBJECT, 0, 1, 2]).unwrap();
        assert!(load_binary_file(&bad).is_none());
        assert!(load_kv_bin(dir.join("missing.bin")).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_map_bytes_handles_corruption() {
        assert!(progress_map_bytes(&[]).is_empty());
        assert!(progress_map_bytes(&[0x99]).is_empty());
    }

    #[test]
    fn every_truncation_of_a_valid_file_returns_without_panicking() {
        let full = root(&[
            object(
                "stats",
                &[object(
                    "1",
                    &[object("bits", &[object("0", &[string("name", "ACH01")])])],
                )],
            ),
            string("tail", "x"),
        ]);
        for length in 0..=full.len() {
            let _ = read_kv(&full[..length], 0, 0);
            let _ = parse_binary(&full[..length]);
            let _ = parse_binary_object(&full[..length]);
        }
        // Bytes aleatórios também não podem estourar.
        for seed in 0u32..256 {
            let bytes: Vec<u8> = (0..32)
                .map(|index| ((seed.wrapping_mul(31).wrapping_add(index)) & 0xff) as u8)
                .collect();
            let _ = read_kv(&bytes, 0, 0);
            let _ = parse_binary(&bytes);
        }
    }
}
