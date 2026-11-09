//! Parser e serializador de KeyValues (VDF) da Valve.
//!
//! Porte fiel de `app/electron/vdf.js`:
//!
//! - strings sempre entre aspas duplas;
//! - `//` inicia comentário até o fim da linha (não há comentário de bloco);
//! - escapes: `\n` → nova linha, `\t` → tab, qualquer outro → o próprio
//!   caractere (`\"`, `\\`, `\/`); uma barra no fim do arquivo fica literal;
//! - chaves repetidas viram lista, na ordem em que aparecem;
//! - a ordem das chaves é preservada (o JS depende dela para escolher a
//!   primeira conta de `loginusers.vdf` quando não há `MostRecent`);
//! - um `}` extra depois da raiz é ignorado, e uma string sem aspas de
//!   fechamento consome até o fim do arquivo (o JS não lança nesse caso).
//!
//! Além do parser do cliente, `parse_bytes` aceita os BOMs reais que aparecem
//! nos arquivos da Steam (UTF-8, UTF-16LE e UTF-16BE).

use std::fmt;

/// Erro de sintaxe com a posição (offset em bytes) onde o parser parou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VdfError {
    pub position: usize,
    pub message: String,
}

impl fmt::Display for VdfError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "VDF inválido na posição {}: {}",
            self.position, self.message
        )
    }
}

impl std::error::Error for VdfError {}

/// Valor de uma chave VDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdfValue {
    String(String),
    Object(VdfObject),
    /// Lista criada por chaves repetidas (o JS devolve um array nesse caso).
    List(Vec<VdfValue>),
}

impl VdfValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&VdfObject> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[VdfValue]> {
        match self {
            Self::List(values) => Some(values),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&VdfValue> {
        self.as_object()?.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    pub fn get_object(&self, key: &str) -> Option<&VdfObject> {
        self.get(key)?.as_object()
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::String(_) => "string",
            Self::Object(_) => "object",
            Self::List(_) => "array",
        }
    }

    /// Equivalente a `String(valor)` no JS.
    pub fn js_string(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Object(_) => "[object Object]".to_string(),
            Self::List(values) => values
                .iter()
                .map(VdfValue::js_string)
                .collect::<Vec<_>>()
                .join(","),
        }
    }

    /// Equivalente a `Number(valor)` para valores de VDF.
    pub fn js_number(&self) -> Option<f64> {
        match self {
            Self::String(value) => crate::js::js_number_str(value),
            Self::List(_) => crate::js::js_number_str(&self.js_string()),
            Self::Object(_) => None,
        }
    }
}

/// Objeto VDF com a ordem de inserção preservada.
///
/// Chaves repetidas são consolidadas em [`VdfValue::List`] por
/// [`VdfObject::push_value`], como o `porValor` do JS. [`VdfObject::insert`]
/// substitui um valor existente (semântica de mapa), útil para montar dados.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VdfObject {
    entries: Vec<(String, VdfValue)>,
}

impl VdfObject {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.position(key).is_some()
    }

    pub fn get(&self, key: &str) -> Option<&VdfValue> {
        self.position(key).map(|index| &self.entries[index].1)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut VdfValue> {
        self.position(key).map(|index| &mut self.entries[index].1)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    pub fn get_object(&self, key: &str) -> Option<&VdfObject> {
        self.get(key)?.as_object()
    }

    pub fn get_list(&self, key: &str) -> Option<&[VdfValue]> {
        self.get(key)?.as_list()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &VdfValue)> + '_ {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.iter().map(|(key, _)| key.as_str())
    }

    pub fn values(&self) -> impl Iterator<Item = &VdfValue> + '_ {
        self.entries.iter().map(|(_, value)| value)
    }

    /// Insere/substitui um valor; devolve o valor antigo quando existia.
    pub fn insert(&mut self, key: impl Into<String>, value: VdfValue) -> Option<VdfValue> {
        let key = key.into();
        match self.position(&key) {
            Some(index) => Some(std::mem::replace(&mut self.entries[index].1, value)),
            None => {
                self.entries.push((key, value));
                None
            }
        }
    }

    /// Mesma regra de `porValor`: a segunda ocorrência da chave vira lista.
    pub fn push_value(&mut self, key: impl Into<String>, value: VdfValue) {
        let key = key.into();
        if let Some(index) = self.position(&key) {
            let previous =
                std::mem::replace(&mut self.entries[index].1, VdfValue::String(String::new()));
            let merged = match previous {
                VdfValue::List(mut values) => {
                    values.push(value);
                    VdfValue::List(values)
                }
                other => VdfValue::List(vec![other, value]),
            };
            self.entries[index].1 = merged;
        } else {
            self.entries.push((key, value));
        }
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|(candidate, _)| candidate == key)
    }
}

impl fmt::Display for VdfObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&serialize(self))
    }
}

/// Lê VDF de um texto já decodificado. Um BOM UTF-8 no início é removido.
pub fn parse(text: &str) -> Result<VdfObject, VdfError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    Parser {
        source: text,
        index: 0,
    }
    .parse_root()
}

/// Lê VDF de bytes, detectando BOM UTF-8/UTF-16LE/UTF-16BE.
///
/// Sem BOM, o conteúdo é decodificado como UTF-8 com substituição de bytes
/// inválidos (o Node faz a mesma coisa ao ler `utf-8`).
pub fn parse_bytes(bytes: &[u8]) -> Result<VdfObject, VdfError> {
    let text = decode_bytes(bytes);
    parse(&text)
}

fn decode_bytes(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16(rest, false);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let (pairs, _) = bytes.as_chunks::<2>();
    let units = pairs.iter().map(|pair| {
        if little_endian {
            u16::from_le_bytes(*pair)
        } else {
            u16::from_be_bytes(*pair)
        }
    });
    char::decode_utf16(units)
        .map(|value| value.unwrap_or('\u{fffd}'))
        .collect()
}

struct Parser<'a> {
    source: &'a str,
    index: usize,
}

impl<'a> Parser<'a> {
    fn parse_root(mut self) -> Result<VdfObject, VdfError> {
        self.parse_block()
    }

    fn parse_block(&mut self) -> Result<VdfObject, VdfError> {
        let mut object = VdfObject::new();
        loop {
            self.skip_space();
            let Some(character) = self.peek() else {
                return Ok(object);
            };
            if character == '}' {
                self.bump();
                return Ok(object);
            }
            if character != '"' {
                return Err(self.error("esperava uma chave"));
            }
            let key = self.read_string()?;
            self.skip_space();
            match self.peek() {
                Some('{') => {
                    self.bump();
                    let child = self.parse_block()?;
                    object.push_value(key, VdfValue::Object(child));
                }
                Some('"') => {
                    let value = self.read_string()?;
                    object.push_value(key, VdfValue::String(value));
                }
                _ => {
                    return Err(self.error(format!("chave \"{key}\" sem valor nem bloco")));
                }
            }
        }
    }

    fn skip_space(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.bump();
                }
                Some('/') if self.peek_next() == Some('/') => {
                    while let Some(character) = self.peek() {
                        self.bump();
                        if character == '\n' {
                            break;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    fn read_string(&mut self) -> Result<String, VdfError> {
        if self.peek() != Some('"') {
            return Err(self.error("esperava uma string entre aspas"));
        }
        self.bump();
        let mut output = String::new();
        while let Some(character) = self.peek() {
            if character == '"' {
                self.bump();
                return Ok(output);
            }
            if character == '\\' {
                match self.peek_next() {
                    Some(escaped) => {
                        self.bump();
                        self.bump();
                        output.push(match escaped {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    }
                    // Barra no fim do arquivo: o JS mantém a barra literal.
                    None => {
                        self.bump();
                        output.push('\\');
                    }
                }
                continue;
            }
            self.bump();
            output.push(character);
        }
        // Sem aspas de fechamento o JS devolve o que leu e avança além do fim.
        Ok(output)
    }

    fn peek(&self) -> Option<char> {
        self.source[self.index..].chars().next()
    }

    fn peek_next(&self) -> Option<char> {
        let mut characters = self.source[self.index..].chars();
        characters.next()?;
        characters.next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.index += character.len_utf8();
        Some(character)
    }

    fn error(&self, message: impl Into<String>) -> VdfError {
        VdfError {
            position: self.index,
            message: message.into(),
        }
    }
}

/// Serializa de volta para KeyValues, com tabs e uma chave por linha.
///
/// Chaves repetidas (representadas por [`VdfValue::List`]) voltam a ser
/// emitidas como ocorrências repetidas da mesma chave, de modo que
/// `parse(&serialize(&value)) == value`.
pub fn serialize(root: &VdfObject) -> String {
    let mut output = String::new();
    write_entries(root, 0, &mut output);
    output
}

fn write_entries(object: &VdfObject, depth: usize, output: &mut String) {
    for (key, value) in object.iter() {
        write_named(key, value, depth, output);
    }
}

fn write_named(key: &str, value: &VdfValue, depth: usize, output: &mut String) {
    match value {
        VdfValue::List(values) => {
            for item in values {
                write_named(key, item, depth, output);
            }
        }
        VdfValue::String(value) => {
            write_indent(depth, output);
            write_quoted(key, output);
            output.push_str("\t\t");
            write_quoted(value, output);
            output.push('\n');
        }
        VdfValue::Object(child) => {
            write_indent(depth, output);
            write_quoted(key, output);
            output.push('\n');
            write_indent(depth, output);
            output.push_str("{\n");
            write_entries(child, depth + 1, output);
            write_indent(depth, output);
            output.push_str("}\n");
        }
    }
}

fn write_indent(depth: usize, output: &mut String) {
    for _ in 0..depth {
        output.push('\t');
    }
}

fn write_quoted(value: &str, output: &mut String) {
    output.push('"');
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            other => output.push(other),
        }
    }
    output.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGIN_USERS: &str = "\"users\"\n{\n\t\"76561198000010132\"\n\t{\n\t\t\"AccountName\"\t\t\"conta1\"\n\t\t\"PersonaName\"\t\t\"NEGO EJACULADO\"\n\t\t\"RememberPassword\"\t\t\"1\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t\t\"Timestamp\"\t\t\"1700000000\"\n\t}\n\t\"76561198000004541\"\n\t{\n\t\t\"AccountName\"\t\t\"conta2\"\n\t\t\"PersonaName\"\t\t\"Kk\"\n\t\t\"RememberPassword\"\t\t\"1\"\n\t\t\"MostRecent\"\t\t\"1\"\n\t\t\"Timestamp\"\t\t\"1800000000\"\n\t}\n}\n";

    #[test]
    fn parses_nesting_comments_and_escapes_like_the_js_oracle() {
        let parsed = parse(
            "// comentario\n\"raiz\"\n{\n\t\"a\" \"1\"\n\t\"b\" { \"c\" \"dois\" }\n\t\"quoted\" \"diz \\\"oi\\\"\"\n}\n",
        )
        .unwrap();
        let root = parsed.get_object("raiz").unwrap();
        assert_eq!(root.get_str("a"), Some("1"));
        assert_eq!(root.get_object("b").unwrap().get_str("c"), Some("dois"));
        assert_eq!(root.get_str("quoted"), Some("diz \"oi\""));
    }

    #[test]
    fn repeated_keys_become_a_list_in_file_order() {
        let parsed = parse("\"r\" { \"k\" \"1\" \"k\" \"2\" \"k\" \"3\" }").unwrap();
        let values = parsed.get_object("r").unwrap().get_list("k").unwrap();
        assert_eq!(values.len(), 3);
        assert_eq!(values[0].as_str(), Some("1"));
        assert_eq!(values[2].as_str(), Some("3"));
    }

    #[test]
    fn repeated_objects_also_become_a_list() {
        let parsed = parse("\"r\" { \"k\" { \"a\" \"1\" } \"k\" { \"a\" \"2\" } }").unwrap();
        let values = parsed.get_object("r").unwrap().get_list("k").unwrap();
        assert_eq!(values[0].get_str("a"), Some("1"));
        assert_eq!(values[1].get_str("a"), Some("2"));
    }

    #[test]
    fn key_order_is_preserved() {
        let parsed = parse("\"r\" { \"z\" \"1\" \"a\" \"2\" \"m\" \"3\" }").unwrap();
        let keys: Vec<_> = parsed.get_object("r").unwrap().keys().collect();
        assert_eq!(keys, ["z", "a", "m"]);
    }

    #[test]
    fn login_users_fixture_keeps_both_accounts_in_order() {
        let parsed = parse(LOGIN_USERS).unwrap();
        let users = parsed.get_object("users").unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users.keys().next(), Some("76561198000010132"));
        let first = users.get_object("76561198000010132").unwrap();
        assert_eq!(first.get_str("PersonaName"), Some("NEGO EJACULADO"));
        assert_eq!(first.get_str("MostRecent"), Some("0"));
    }

    #[test]
    fn escapes_follow_the_exact_js_rules() {
        let parsed = parse(
            "\"r\" {\n\
             \"newline\" \"a\\nb\"\n\
             \"tab\" \"a\\tb\"\n\
             \"quote\" \"a\\\"b\"\n\
             \"backslash\" \"a\\\\b\"\n\
             \"unknown\" \"a\\rb\"\n\
             \"slash\" \"a\\/b\"\n\
             }",
        )
        .unwrap();
        let root = parsed.get_object("r").unwrap();
        assert_eq!(root.get_str("newline"), Some("a\nb"));
        assert_eq!(root.get_str("tab"), Some("a\tb"));
        assert_eq!(root.get_str("quote"), Some("a\"b"));
        assert_eq!(root.get_str("backslash"), Some("a\\b"));
        // O JS só trata n/t; "\r" vira a letra r.
        assert_eq!(root.get_str("unknown"), Some("arb"));
        assert_eq!(root.get_str("slash"), Some("a/b"));
    }

    #[test]
    fn comments_inside_strings_are_not_comments() {
        let parsed = parse("\"r\" { \"url\" \"https://example.test/a//b\" }").unwrap();
        assert_eq!(
            parsed.get_object("r").unwrap().get_str("url"),
            Some("https://example.test/a//b")
        );
    }

    #[test]
    fn crlf_and_comments_are_accepted() {
        let parsed = parse("// c\r\n\"r\"\r\n{\r\n\t\"k\" \"v\" // fim\r\n}\r\n").unwrap();
        assert_eq!(parsed.get_object("r").unwrap().get_str("k"), Some("v"));
    }

    #[test]
    fn utf8_bom_is_stripped_and_utf16_is_decoded() {
        let utf8 = b"\xef\xbb\xbf\"r\" { \"k\" \"v\" }";
        assert_eq!(
            parse_bytes(utf8)
                .unwrap()
                .get_object("r")
                .unwrap()
                .get_str("k"),
            Some("v")
        );

        let text = "\"r\" { \"k\" \"olá\" }";
        let mut utf16le = vec![0xff, 0xfe];
        for unit in text.encode_utf16() {
            utf16le.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(
            parse_bytes(&utf16le)
                .unwrap()
                .get_object("r")
                .unwrap()
                .get_str("k"),
            Some("olá")
        );

        let mut utf16be = vec![0xfe, 0xff];
        for unit in text.encode_utf16() {
            utf16be.extend_from_slice(&unit.to_be_bytes());
        }
        assert_eq!(
            parse_bytes(&utf16be)
                .unwrap()
                .get_object("r")
                .unwrap()
                .get_str("k"),
            Some("olá")
        );
    }

    #[test]
    fn malformed_input_returns_a_clear_error() {
        let error = parse("\"a\" sem aspas").unwrap_err();
        assert!(error.to_string().starts_with("VDF inválido na posição"));
        assert!(error.message.contains("sem valor nem bloco"));

        let error = parse("sem chave").unwrap_err();
        assert!(error.message.contains("esperava uma chave"));
    }

    #[test]
    fn unterminated_string_and_extra_brace_match_the_js_parser() {
        let parsed = parse("\"r\" { \"k\" \"v }").unwrap();
        assert_eq!(parsed.get_object("r").unwrap().get_str("k"), Some("v }"));
        let parsed = parse("\"r\" { \"k\" \"v\" } }").unwrap();
        assert_eq!(parsed.get_object("r").unwrap().get_str("k"), Some("v"));
    }

    #[test]
    fn empty_and_whitespace_input_is_an_empty_object() {
        assert!(parse("").unwrap().is_empty());
        assert!(parse(" \r\n\t// so comentario\n").unwrap().is_empty());
    }

    #[test]
    fn serializer_round_trips_nesting_duplicates_and_escapes() {
        let source = "\"r\"\n{\n\t\"a\"\t\t\"1\"\n\t\"a\"\t\t\"2\"\n\t\"b\"\n\t{\n\t\t\"x\"\t\t\"diz \\\"oi\\\"\\n\"\n\t}\n}\n";
        let parsed = parse(source).unwrap();
        let serialized = serialize(&parsed);
        let reparsed = parse(&serialized).unwrap();
        assert_eq!(parsed, reparsed);
        assert!(serialized.contains("\"a\""));
        assert!(serialized.contains('\t'));
    }

    #[test]
    fn value_helpers_mirror_js_string_and_number() {
        let parsed =
            parse("\"r\" { \"s\" \"4907\" \"l\" \"1\" \"l\" \"2\" \"o\" { \"x\" \"1\" } }")
                .unwrap();
        let root = parsed.get_object("r").unwrap();
        assert_eq!(root.get("s").unwrap().js_number(), Some(4907.0));
        assert_eq!(root.get("l").unwrap().js_string(), "1,2");
        assert_eq!(root.get("l").unwrap().js_number(), None);
        assert_eq!(root.get("o").unwrap().js_string(), "[object Object]");
        assert_eq!(root.get("o").unwrap().js_number(), None);
    }

    #[test]
    fn insert_replaces_but_push_value_merges() {
        let mut object = VdfObject::new();
        object.insert("k", VdfValue::String("1".into()));
        object.insert("k", VdfValue::String("2".into()));
        assert_eq!(object.get_str("k"), Some("2"));
        object.push_value("k", VdfValue::String("3".into()));
        assert_eq!(object.get_list("k").unwrap().len(), 2);
    }
}
