//! Cliente do worker torrent Python (JSON por linha no stdio/socket) e regras
//! de validação de IDs/URIs do `app/electron/torrent.js`.
//!
//! O worker (`app/electron/torrent_rpc/main.py`) recebe
//! `{"id": N, "method": "...", "params": {...}}` e responde
//! `{"id": N, "result": ...}` ou `{"id": N, "error": {"code", "message"}}`;
//! o primeiro evento é `{"event": "ready", "protocolVersion": 1}`.
//!
//! O transporte é injetável (stdio, socket TCP ou fake). Este crate não
//! spawna o Python nem abre sockets sozinho: `worker_command` devolve o argv.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::js::{js_string, js_trim, js_truthy};

pub const MAX_TORRENT_ID_LENGTH: usize = 256;
pub const MAX_DOWNLOAD_URI_LENGTH: usize = 8192;
pub const DEFAULT_PYTHON: &str = "python3";
pub const PROTOCOL_VERSION: u64 = 1;

// ---------------------------------------------------------------------------
// Validação de IDs e URIs (torrent.js)
// ---------------------------------------------------------------------------

/// `normalizeTorrentId`: devolve `tor:<id>` ou string vazia.
pub fn normalize_torrent_id(value: &Value) -> String {
    let raw = match value {
        Value::String(text) => js_trim(text),
        other => {
            if js_truthy(other) {
                js_trim(&js_string(other))
            } else {
                String::new()
            }
        }
    };
    if raw.is_empty() {
        return String::new();
    }
    let id = raw.strip_prefix("tor:").unwrap_or(&raw);
    if id.is_empty()
        || id.len() > MAX_TORRENT_ID_LENGTH - 4
        || id.starts_with("tor:")
        || !is_valid_torrent_id(id)
    {
        return String::new();
    }
    format!("tor:{id}")
}

fn is_valid_torrent_id(id: &str) -> bool {
    let mut characters = id.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    characters.all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
    })
}

/// `isPrivateHost`: localhost, loopback, RFC1918 e qualquer literal IPv6.
pub fn is_private_host(hostname: &str) -> bool {
    let mut host = hostname;
    if let Some(stripped) = host.strip_prefix('[') {
        host = stripped;
    }
    if let Some(stripped) = host.strip_suffix(']') {
        host = stripped;
    }
    let host = host.to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host.ends_with(".localhost") || host == "::1" {
        return true;
    }
    if host.contains(':') {
        return true;
    }
    if host.starts_with("10.") || host.starts_with("127.") || host.starts_with("192.168.") {
        return true;
    }
    if let Some(rest) = host.strip_prefix("172.") {
        if let Some(octet) = rest
            .split('.')
            .next()
            .and_then(|part| part.parse::<u8>().ok())
        {
            if (16..=31).contains(&octet) {
                return true;
            }
        }
    }
    false
}

struct ParsedUrl {
    scheme: String,
    authority: String,
    hostname: String,
    has_credentials: bool,
    has_hash: bool,
    rest: String,
}

fn parse_url(raw: &str) -> Option<ParsedUrl> {
    let colon = raw.find(':')?;
    if colon == 0 {
        return None;
    }
    let scheme = raw[..colon].to_ascii_lowercase();
    let rest = &raw[colon + 1..];
    let (before_hash, _hash) = match rest.find('#') {
        Some(index) => (&rest[..index], &rest[index + 1..]),
        None => (rest, ""),
    };
    let has_hash = !_hash.is_empty();
    let authority_start = if before_hash.starts_with("//") { 2 } else { 0 };
    let after_scheme = &before_hash[authority_start..];
    let authority_end = after_scheme.find(['/', '?']).unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let (credentials, host_port) = match authority.rsplit_once('@') {
        Some((credentials, host_port)) => (Some(credentials), host_port),
        None => (None, authority),
    };
    let has_credentials = credentials.is_some_and(|credentials| {
        let (username, password) = credentials.split_once(':').unwrap_or((credentials, ""));
        !username.is_empty() || !password.is_empty()
    });
    let hostname = if let Some(after_bracket) = host_port.strip_prefix('[') {
        let end = after_bracket.find(']')?;
        let hostname = &after_bracket[..end];
        let remainder = &after_bracket[end + 1..];
        if !remainder.is_empty() {
            let port = remainder.strip_prefix(':')?;
            validate_port(port)?;
        }
        hostname.to_string()
    } else {
        let (host, port) = match host_port.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, Some(port)),
            Some(_) => return None,
            None => (host_port, None),
        };
        if let Some(port) = port {
            validate_port(port)?;
        }
        host.to_string()
    };
    if hostname.is_empty() && (scheme == "http" || scheme == "https") {
        return None;
    }
    if hostname
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    if hostname
        .chars()
        .any(|character| matches!(character, '/' | '?' | '#' | '@'))
    {
        return None;
    }
    Some(ParsedUrl {
        scheme,
        authority: authority.to_string(),
        hostname,
        has_credentials,
        has_hash,
        rest: rest.to_string(),
    })
}

fn validate_port(port: &str) -> Option<u16> {
    if port.is_empty() || !port.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    port.parse::<u16>().ok()
}

/// `normalizeDownloadUri`: aceita apenas magnet com `xt=urn:btih:` e HTTP(S)
/// público, sem credenciais/hash/controle.
pub fn normalize_download_uri(value: &Value) -> String {
    let Value::String(uri) = value else {
        return String::new();
    };
    let uri = js_trim(uri);
    if uri.is_empty()
        || uri.encode_utf16().count() > MAX_DOWNLOAD_URI_LENGTH
        || uri
            .chars()
            .any(|character| matches!(character, '\u{0}'..='\u{1f}' | '\u{7f}'))
    {
        return String::new();
    }
    if uri
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("magnet:"))
    {
        let Some(parsed) = parse_url(&uri) else {
            return String::new();
        };
        if parsed.scheme != "magnet" || parsed.has_credentials {
            return String::new();
        }
        if !magnet_has_btih(&parsed.rest) {
            return String::new();
        }
        // O worker aceita apenas o esquema canônico minúsculo.
        return format!("magnet:{}", &uri[7..]);
    }

    let parsed = match parse_url(&uri) {
        Some(parsed) => parsed,
        None => return String::new(),
    };
    if parsed.scheme != "https" && parsed.scheme != "http" {
        return String::new();
    }
    if parsed.has_credentials || parsed.has_hash || is_private_host(&parsed.hostname) {
        return String::new();
    }
    // Autoridade vazia não é um host válido (magnet não passa por aqui).
    if parsed.authority.is_empty() {
        return String::new();
    }
    uri
}

fn magnet_has_btih(rest: &str) -> bool {
    let Some(query_start) = rest.find('?') else {
        return false;
    };
    let query = &rest[query_start + 1..];
    let query = query.split('#').next().unwrap_or(query);
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(key) != "xt" {
            continue;
        }
        let value = percent_decode(value);
        let lower = value.to_ascii_lowercase();
        if let Some(hash) = lower.strip_prefix("urn:btih:") {
            if !hash.is_empty()
                && hash
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
            {
                return true;
            }
        }
    }
    false
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = hex_digit(bytes[index + 1]);
                let low = hex_digit(bytes[index + 2]);
                match (high, low) {
                    (Some(high), Some(low)) => {
                        output.push(high * 16 + low);
                        index += 3;
                    }
                    _ => {
                        output.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

// ---------------------------------------------------------------------------
// Transporte e protocolo
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentTransportError(pub String);

impl std::fmt::Display for TorrentTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TorrentTransportError {}

/// Canal de linhas do worker. `send_line` recebe a linha sem `\n`; o
/// transporte stdio/socket adiciona o delimitador.
pub trait TorrentTransport {
    fn send_line(&mut self, line: &str) -> Result<(), TorrentTransportError>;
    fn recv_line(&mut self) -> Result<Option<String>, TorrentTransportError>;
}

/// Transporte pronto para stdin/stdout de um filho ou para um `TcpStream`.
pub struct LineTransport<R: BufRead, W: Write> {
    reader: R,
    writer: W,
}

impl<R: BufRead, W: Write> LineTransport<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }

    pub fn into_inner(self) -> (R, W) {
        (self.reader, self.writer)
    }
}

impl LineTransport<BufReader<TcpStream>, TcpStream> {
    pub fn tcp(stream: TcpStream) -> std::io::Result<Self> {
        let writer = stream.try_clone()?;
        Ok(Self::new(BufReader::new(stream), writer))
    }
}

impl<R: BufRead, W: Write> TorrentTransport for LineTransport<R, W> {
    fn send_line(&mut self, line: &str) -> Result<(), TorrentTransportError> {
        self.writer
            .write_all(line.as_bytes())
            .and_then(|_| self.writer.write_all(b"\n"))
            .and_then(|_| self.writer.flush())
            .map_err(|error| TorrentTransportError(error.to_string()))
    }

    fn recv_line(&mut self) -> Result<Option<String>, TorrentTransportError> {
        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .map_err(|error| TorrentTransportError(error.to_string()))?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentRpcError {
    pub code: String,
    pub message: String,
}

impl TorrentRpcError {
    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            code: "transport_error".to_string(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for TorrentRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // O oráculo usa `new Error(msg.error.code)`.
        f.write_str(&self.code)
    }
}

impl std::error::Error for TorrentRpcError {}

/// Requisição JSON-lines exata do protocolo (`id`, `method`, `params`).
pub fn wire_request(id: u64, method: &str, params: &Value) -> String {
    json!({
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
}

pub struct TorrentRpcClient<T: TorrentTransport> {
    transport: T,
    next_id: u64,
    protocol_version: Option<u64>,
}

impl<T: TorrentTransport> TorrentRpcClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            next_id: 1,
            protocol_version: None,
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn protocol_version(&self) -> Option<u64> {
        self.protocol_version
    }

    /// Lê mensagens até o evento `ready` do worker.
    pub fn wait_ready(&mut self) -> Result<u64, TorrentRpcError> {
        for _ in 0..1024 {
            let Some(line) = self
                .transport
                .recv_line()
                .map_err(|error| TorrentRpcError::transport(error.0))?
            else {
                return Err(TorrentRpcError::transport("worker torrent morreu"));
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if message.get("event").and_then(Value::as_str) == Some("ready") {
                let version = message
                    .get("protocolVersion")
                    .and_then(Value::as_u64)
                    .unwrap_or(PROTOCOL_VERSION);
                self.protocol_version = Some(version);
                return Ok(version);
            }
        }
        Err(TorrentRpcError::transport("worker torrent sem ready"))
    }

    pub fn rpc(&mut self, method: &str, params: &Value) -> Result<Value, TorrentRpcError> {
        let id = self.next_id;
        self.next_id += 1;
        let line = wire_request(id, method, params);
        self.transport
            .send_line(&line)
            .map_err(|error| TorrentRpcError::transport(error.0))?;
        for _ in 0..1024 {
            let Some(raw) = self
                .transport
                .recv_line()
                .map_err(|error| TorrentRpcError::transport(error.0))?
            else {
                return Err(TorrentRpcError::transport("worker torrent morreu"));
            };
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(raw) else {
                continue;
            };
            if message.get("event").and_then(Value::as_str) == Some("ready") {
                self.protocol_version = message
                    .get("protocolVersion")
                    .and_then(Value::as_u64)
                    .or(self.protocol_version);
                continue;
            }
            if message.get("id") != Some(&Value::from(id)) {
                // Resposta de outra chamada: o oráculo só resolve o id
                // pendente correspondente.
                continue;
            }
            if let Some(error) = message.get("error").filter(|value| !value.is_null()) {
                let code = error
                    .get("code")
                    .map(js_string)
                    .unwrap_or_else(|| "internal_error".to_string());
                let detail = error
                    .get("message")
                    .map(js_string)
                    .unwrap_or_else(|| code.clone());
                return Err(TorrentRpcError {
                    code,
                    message: detail,
                });
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
        Err(TorrentRpcError::transport("worker torrent sem resposta"))
    }

    pub fn action(&mut self, params: Value) -> Result<Value, TorrentRpcError> {
        self.rpc("action", &params)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        game_id: &str,
        url: &str,
        save_path: &str,
        file_indices: Option<&[i64]>,
        trackers: Option<&[String]>,
    ) -> Result<(), TorrentRpcError> {
        let mut params = Map::new();
        params.insert("action".to_string(), json!("start"));
        params.insert("game_id".to_string(), json!(game_id));
        params.insert("url".to_string(), json!(url));
        params.insert("save_path".to_string(), json!(save_path));
        params.insert(
            "file_indices".to_string(),
            match file_indices {
                Some(indices) => json!(indices),
                None => Value::Null,
            },
        );
        if let Some(trackers) = trackers {
            params.insert("trackers".to_string(), json!(trackers));
        }
        self.action(Value::Object(params)).map(|_| ())
    }

    /// O worker não tem ação `resume`: retomar é reenviar `start` com os
    /// mesmos dados (o libtorrent valida o disco e continua).
    pub fn resume(
        &mut self,
        game_id: &str,
        url: &str,
        save_path: &str,
        file_indices: Option<&[i64]>,
        trackers: Option<&[String]>,
    ) -> Result<(), TorrentRpcError> {
        self.start(game_id, url, save_path, file_indices, trackers)
    }

    pub fn pause(&mut self, game_id: &str) -> Result<(), TorrentRpcError> {
        self.simple_action("pause", game_id)
    }

    pub fn cancel(&mut self, game_id: &str) -> Result<(), TorrentRpcError> {
        self.simple_action("cancel", game_id)
    }

    fn simple_action(&mut self, action: &str, game_id: &str) -> Result<(), TorrentRpcError> {
        self.action(json!({
            "action": action,
            "game_id": game_id,
        }))
        .map(|_| ())
    }

    pub fn set_download_limit(
        &mut self,
        bytes_per_second: Option<u64>,
    ) -> Result<(), TorrentRpcError> {
        let mut params = Map::new();
        params.insert("action".to_string(), json!("set_download_limit"));
        if let Some(limit) = bytes_per_second {
            params.insert(
                "max_download_speed_bytes_per_second".to_string(),
                json!(limit),
            );
        }
        self.action(Value::Object(params)).map(|_| ())
    }

    pub fn status(&mut self) -> Result<HashMap<String, TorrentStatus>, TorrentRpcError> {
        let result = self.rpc("status", &json!({}))?;
        if result.is_null() {
            return Ok(HashMap::new());
        }
        serde_json::from_value(result)
            .map_err(|error| TorrentRpcError::transport(error.to_string()))
    }

    pub fn torrent_files(
        &mut self,
        magnet: &str,
        timeout_ms: Option<i64>,
        trackers: Option<&[String]>,
    ) -> Result<TorrentFiles, TorrentRpcError> {
        let mut params = Map::new();
        params.insert("magnet".to_string(), json!(magnet));
        if let Some(timeout_ms) = timeout_ms {
            params.insert("timeout_ms".to_string(), json!(timeout_ms));
        }
        if let Some(trackers) = trackers {
            params.insert("trackers".to_string(), json!(trackers));
        }
        let result = self.rpc("torrent_files", &Value::Object(params))?;
        serde_json::from_value(result)
            .map_err(|error| TorrentRpcError::transport(error.to_string()))
    }

    /// Lista persistida em `torrent_state.json` (não vem do worker).
    pub fn list(&self, state_json: &str) -> Vec<Value> {
        parse_state(state_json)
    }
}

// ---------------------------------------------------------------------------
// Estado/progresso do worker
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TorrentStatus {
    pub folder_name: String,
    pub file_size: f64,
    pub progress: f64,
    pub download_speed: f64,
    pub upload_speed: f64,
    pub num_peers: f64,
    pub num_seeds: f64,
    pub state: String,
    pub bytes_downloaded: f64,
}

/// Campos que o tick do `torrent.js` espalha sobre o item persistido.
pub fn progress_patch(status: &TorrentStatus) -> Value {
    json!({
        "folderName": status.folder_name,
        "fileSize": status.file_size,
        "progress": status.progress,
        "downloadSpeed": status.download_speed,
        "uploadSpeed": status.upload_speed,
        "numPeers": status.num_peers,
        "numSeeds": status.num_seeds,
        "state": status.state,
        "bytesDownloaded": status.bytes_downloaded,
        "completo": status.progress >= 1.0,
    })
}

/// `{ ...item, ...status, completo }` preservando campos desconhecidos.
pub fn merge_progress(item: &Value, status: &TorrentStatus) -> Value {
    let mut merged = item.clone();
    if let (Some(target), Some(patch)) =
        (merged.as_object_mut(), progress_patch(status).as_object())
    {
        for (key, value) in patch {
            target.insert(key.clone(), value.clone());
        }
    }
    merged
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TorrentFile {
    pub index: i64,
    pub path: String,
    pub length: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TorrentFiles {
    pub info_hash: String,
    pub name: String,
    pub total_size: f64,
    pub files: Vec<TorrentFile>,
}

/// `readState`: array JSON de objetos; qualquer outra coisa vira lista vazia.
pub fn parse_state(text: &str) -> Vec<Value> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Value::Array(items) = value else {
        return Vec::new();
    };
    items.into_iter().filter(|item| item.is_object()).collect()
}

/// `writeState`: JSON indentado (2 espaços) como o `JSON.stringify(list, null, 2)`.
pub fn serialize_state(items: &[Value]) -> String {
    serde_json::to_string_pretty(items).unwrap_or_else(|_| "[]".to_string())
}

// ---------------------------------------------------------------------------
// Comando do worker
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentWorkerCommand {
    pub cmd: String,
    pub args: Vec<String>,
}

/// `spawn(python3, [main.py, <porta>?])`. A porta é opcional no oráculo; o
/// `main.py` usa `sys.argv[1]` quando presente (default 6881).
pub fn worker_command(
    python: &str,
    script: &Path,
    listen_port: Option<u16>,
) -> TorrentWorkerCommand {
    let mut args = vec![script.to_string_lossy().into_owned()];
    if let Some(port) = listen_port {
        args.push(port.to_string());
    }
    TorrentWorkerCommand {
        cmd: python.to_string(),
        args,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::Cursor;

    #[derive(Default)]
    struct FakeTransport {
        incoming: VecDeque<String>,
        written: Vec<String>,
    }

    impl FakeTransport {
        fn new(lines: Vec<&str>) -> Self {
            Self {
                incoming: lines.into_iter().map(str::to_string).collect(),
                written: Vec::new(),
            }
        }
    }

    impl TorrentTransport for FakeTransport {
        fn send_line(&mut self, line: &str) -> Result<(), TorrentTransportError> {
            self.written.push(line.to_string());
            Ok(())
        }

        fn recv_line(&mut self) -> Result<Option<String>, TorrentTransportError> {
            Ok(self.incoming.pop_front())
        }
    }

    fn value_to_string(value: &Value) -> String {
        value.to_string()
    }

    #[test]
    fn normalize_torrent_id_aceita_prefixo_e_rejeita_lixo() {
        assert_eq!(normalize_torrent_id(&json!("abc")), "tor:abc");
        assert_eq!(normalize_torrent_id(&json!("tor:abc")), "tor:abc");
        assert_eq!(
            normalize_torrent_id(&json!("  tor:a.b-c_d:e  ")),
            "tor:a.b-c_d:e"
        );
        assert_eq!(normalize_torrent_id(&json!(7)), "tor:7");
        assert_eq!(normalize_torrent_id(&json!(true)), "tor:true");
        assert_eq!(normalize_torrent_id(&json!(null)), "");
        assert_eq!(normalize_torrent_id(&json!("")), "");
        assert_eq!(normalize_torrent_id(&json!("-abc")), "");
        assert_eq!(normalize_torrent_id(&json!("a b")), "");
        assert_eq!(normalize_torrent_id(&json!("tor:tor:x")), "");
        assert_eq!(normalize_torrent_id(&json!({ "a": 1 })), "");
        let long = format!("tor:{}", "a".repeat(MAX_TORRENT_ID_LENGTH));
        assert_eq!(normalize_torrent_id(&json!(long)), "");
    }

    #[test]
    fn normalize_uri_aceita_magnet_btih_e_http_publico() {
        let magnet = "magnet:?xt=urn:btih:481b6e3617be4c88f96cb25e47c9d8272130071e&dn=repack";
        assert_eq!(normalize_download_uri(&json!(magnet)), magnet);
        let upper = "MAGNET:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01";
        assert_eq!(
            normalize_download_uri(&json!(upper)),
            "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01"
        );
        assert_eq!(
            normalize_download_uri(&json!("https://tracker.example.com/file.torrent")),
            "https://tracker.example.com/file.torrent"
        );
        assert_eq!(
            normalize_download_uri(&json!("  http://example.com/a  ")),
            "http://example.com/a"
        );
    }

    #[test]
    fn normalize_uri_rejeita_magnet_invalido_credenciais_e_hosts_privados() {
        for invalid in [
            "magnet:?dn=sem-hash",
            "magnet:?xt=urn:sha1:abc",
            "magnet:?xt=urn:btih:",
            "magnet:?xt=urn:btih:a-b",
            "magnet://user:pass@tracker/?xt=urn:btih:abcdef",
            "http://localhost/x",
            "http://127.0.0.1/x",
            "http://10.0.0.2/x",
            "http://192.168.1.2/x",
            "http://172.16.0.1/x",
            "http://172.31.255.255/x",
            "http://[::1]/x",
            "http://foo.localhost/x",
            "http://user:pass@example.com/x",
            "http://example.com/x#hash",
            "ftp://example.com/x",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "http://exa mple.com/x",
            "http://example.com/aja\u{0}",
        ] {
            assert_eq!(normalize_download_uri(&json!(invalid)), "", "{invalid}");
        }
        assert_eq!(normalize_download_uri(&json!(123)), "");
        assert_eq!(normalize_download_uri(&json!(null)), "");
        assert_eq!(
            normalize_download_uri(&json!(format!("https://example.com/{}", "a".repeat(8192)))),
            ""
        );
        // 172.15/172.32 não são RFC1918.
        assert_eq!(
            normalize_download_uri(&json!("http://172.15.0.1/x")),
            "http://172.15.0.1/x"
        );
    }

    #[test]
    fn worker_command_inclui_script_e_porta_opcional() {
        let command = worker_command(
            DEFAULT_PYTHON,
            Path::new("app/electron/torrent_rpc/main.py"),
            None,
        );
        assert_eq!(command.cmd, "python3");
        assert_eq!(
            command.args,
            vec!["app/electron/torrent_rpc/main.py".to_string()]
        );
        let command = worker_command("python", Path::new("main.py"), Some(51413));
        assert_eq!(
            command.args,
            vec!["main.py".to_string(), "51413".to_string()]
        );
    }

    #[test]
    fn framing_de_start_e_pause_bate_com_o_json_lines_do_worker() {
        let mut client = TorrentRpcClient::new(FakeTransport::new(vec![
            "{\"id\":1,\"result\":null}",
            "{\"id\":2,\"result\":null}",
            "{\"id\":3,\"result\":null}",
        ]));
        client
            .start(
                "tor:x",
                "magnet:?xt=urn:btih:abcdef",
                "C:\\games",
                Some(&[0, 2]),
                None,
            )
            .unwrap();
        client.pause("tor:x").unwrap();
        client.cancel("tor:x").unwrap();

        let written = &client.transport().written;
        assert_eq!(
            written[0],
            "{\"id\":1,\"method\":\"action\",\"params\":{\"action\":\"start\",\"game_id\":\"tor:x\",\"url\":\"magnet:?xt=urn:btih:abcdef\",\"save_path\":\"C:\\\\games\",\"file_indices\":[0,2]}}"
        );
        assert_eq!(
            written[1],
            "{\"id\":2,\"method\":\"action\",\"params\":{\"action\":\"pause\",\"game_id\":\"tor:x\"}}"
        );
        assert_eq!(
            written[2],
            "{\"id\":3,\"method\":\"action\",\"params\":{\"action\":\"cancel\",\"game_id\":\"tor:x\"}}"
        );
    }

    #[test]
    fn file_indices_none_vira_null_e_trackers_sao_opcionais() {
        let mut client =
            TorrentRpcClient::new(FakeTransport::new(vec!["{\"id\":1,\"result\":null}"]));
        client
            .start("tor:x", "magnet:?xt=urn:btih:a", "/x", None, None)
            .unwrap();
        assert!(client.transport().written[0].contains("\"file_indices\":null"));
        assert!(!client.transport().written[0].contains("trackers"));
    }

    #[test]
    fn ready_e_ignorado_e_resultado_e_resolvido_pelo_id() {
        let mut client = TorrentRpcClient::new(FakeTransport::new(vec![
            "{\"event\":\"ready\",\"protocolVersion\":1}",
            "{\"id\":1,\"result\":{\"a\":1}}",
        ]));
        let result = client.rpc("status", &json!({})).unwrap();
        assert_eq!(result, json!({"a": 1}));
        assert_eq!(client.protocol_version(), Some(1));
    }

    #[test]
    fn wait_ready_le_o_evento_de_bootstrap() {
        let mut client = TorrentRpcClient::new(FakeTransport::new(vec![
            "linha inválida",
            "{\"event\":\"ready\",\"protocolVersion\":1}",
        ]));
        assert_eq!(client.wait_ready(), Ok(1));
        assert_eq!(client.protocol_version(), Some(1));
    }

    #[test]
    fn erro_do_worker_usa_code_como_no_oraculo() {
        let mut client = TorrentRpcClient::new(FakeTransport::new(vec![
            "{\"id\":1,\"error\":{\"code\":\"metadata_timeout\",\"message\":\"demorou\"}}",
            "{\"id\":2,\"error\":{\"code\":\"invalid_magnet\"}}",
        ]));
        assert_eq!(
            client.start("tor:x", "magnet:?xt=x", "/x", None, None),
            Err(TorrentRpcError {
                code: "metadata_timeout".to_string(),
                message: "demorou".to_string(),
            })
        );
        let error = client.pause("tor:x").unwrap_err();
        assert_eq!(error.code, "invalid_magnet");
        assert_eq!(error.to_string(), "invalid_magnet");
    }

    #[test]
    fn linha_json_invalida_e_ignorada_e_eof_rejeita() {
        let mut client = TorrentRpcClient::new(FakeTransport::new(vec![
            "não é json",
            "",
            "{\"id\":1,\"result\":42}",
        ]));
        assert_eq!(client.rpc("status", &json!({})), Ok(json!(42)));

        let mut client = TorrentRpcClient::new(FakeTransport::new(vec![]));
        assert_eq!(
            client.rpc("status", &json!({})),
            Err(TorrentRpcError::transport("worker torrent morreu"))
        );
    }

    #[test]
    fn status_do_worker_vira_progresso_da_ui() {
        let mut client = TorrentRpcClient::new(FakeTransport::new(vec![
            "{\"id\":1,\"result\":{\"tor:x\":{\"folderName\":\"jogo\",\"fileSize\":1000,\"progress\":0.25,\"downloadSpeed\":50,\"uploadSpeed\":1,\"numPeers\":3,\"numSeeds\":2,\"state\":\"downloading\",\"bytesDownloaded\":250}}}",
        ]));
        let statuses = client.status().unwrap();
        let status = &statuses["tor:x"];
        assert_eq!(status.folder_name, "jogo");
        assert_eq!(status.progress, 0.25);
        assert_eq!(status.bytes_downloaded, 250.0);
        assert_eq!(
            progress_patch(status),
            json!({
                "folderName": "jogo",
                "fileSize": 1000.0,
                "progress": 0.25,
                "downloadSpeed": 50.0,
                "uploadSpeed": 1.0,
                "numPeers": 3.0,
                "numSeeds": 2.0,
                "state": "downloading",
                "bytesDownloaded": 250.0,
                "completo": false,
            })
        );

        let item = json!({"gameId": "tor:x", "custom": {"preserve": true}});
        let merged = merge_progress(&item, status);
        assert_eq!(merged["custom"]["preserve"], true);
        assert_eq!(merged["progress"], 0.25);
    }

    #[test]
    fn progresso_100_marca_completo() {
        let status = TorrentStatus {
            progress: 1.0,
            ..TorrentStatus::default()
        };
        assert_eq!(progress_patch(&status)["completo"], true);
    }

    #[test]
    fn torrent_files_mapeia_payload_e_envia_timeout_quando_presente() {
        let response = value_to_string(&json!({
            "id": 1,
            "result": {
                "infoHash": "abc",
                "name": "jogo",
                "totalSize": 300,
                "files": [
                    {"index": 0, "path": "a.bin", "length": 100},
                    {"index": 1, "path": "b.bin", "length": 200}
                ]
            }
        }));
        let mut client = TorrentRpcClient::new(FakeTransport::new(vec![&response]));
        let files = client
            .torrent_files("magnet:?xt=urn:btih:abc", Some(60000), None)
            .unwrap();
        assert_eq!(files.info_hash, "abc");
        assert_eq!(files.name, "jogo");
        assert_eq!(files.total_size, 300.0);
        assert_eq!(files.files[1].path, "b.bin");
        assert_eq!(files.files[1].length, 200.0);
        let written = &client.transport().written[0];
        assert!(written.contains("\"timeout_ms\":60000"));
        assert!(!written.contains("trackers"));
    }

    #[test]
    fn limite_de_download_omite_valor_quando_none() {
        let mut client = TorrentRpcClient::new(FakeTransport::new(vec![
            "{\"id\":1,\"result\":null}",
            "{\"id\":2,\"result\":null}",
        ]));
        client.set_download_limit(Some(1024)).unwrap();
        client.set_download_limit(None).unwrap();
        assert!(
            client.transport().written[0].contains("\"max_download_speed_bytes_per_second\":1024")
        );
        assert!(!client.transport().written[1].contains("max_download_speed_bytes_per_second"));
    }

    #[test]
    fn line_transport_faz_framing_stdio_e_tcp() {
        let reader = Cursor::new("{\"id\":1}\r\nsegunda\n".as_bytes().to_vec());
        let mut transport = LineTransport::new(BufReader::new(reader), Vec::new());
        assert_eq!(
            transport.recv_line().unwrap().as_deref(),
            Some("{\"id\":1}")
        );
        assert_eq!(transport.recv_line().unwrap().as_deref(), Some("segunda"));
        assert_eq!(transport.recv_line().unwrap(), None);
        transport.send_line("saida").unwrap();
        let (_reader, writer) = transport.into_inner();
        assert_eq!(writer, b"saida\n");
    }

    #[test]
    fn estado_torrent_preserva_campos_desconhecidos_e_ignora_nao_objetos() {
        let original = json!([
            {"gameId": "tor:x", "custom": [1, 2], "pausado": false},
            {"gameId": "tor:y"},
            3,
            null,
            [1, 2]
        ]);
        let text = serialize_state(original.as_array().unwrap());
        let parsed = parse_state(&text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["custom"], json!([1, 2]));
        assert_eq!(parse_state("não é json"), Vec::<Value>::new());
        assert_eq!(parse_state("{}"), Vec::<Value>::new());
    }

    #[test]
    fn is_private_host_cobre_as_faixas_do_oraculo() {
        for host in [
            "localhost",
            "a.localhost",
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "172.16.0.1",
            "172.31.1.1",
            "::1",
            "[::1]",
            "fe80::1",
        ] {
            assert!(is_private_host(host), "{host}");
        }
        for host in ["example.com", "172.15.0.1", "172.32.0.1", "8.8.8.8"] {
            assert!(!is_private_host(host), "{host}");
        }
    }
}
