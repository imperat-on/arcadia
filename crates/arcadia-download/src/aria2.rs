//! Cliente do engine aria2c (BitTorrent/magnet no Windows nativo).
//!
//! Port 1:1 de `app/electron/aria2.js` no que é observável sem processo real:
//! injeção de trackers, infohash de magnet, o envelope JSON-RPC com token,
//! os métodos usados pelo `torrent.js`, a política de reutilização de GID por
//! infohash/diretório e o mapeamento de `tellStatus` para a UI.
//!
//! O daemon só escuta em 127.0.0.1 e o transporte é injetado; os testes usam
//! transporte falso e um servidor HTTP local. Nenhum processo é spawnado.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::js::{js_number, js_string, js_trim, js_truthy};

pub const ARIA2_RELEASE_URL: &str =
    "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip";
pub const ARIA2_SHA256: &str = "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288";
pub const EXE_NAME: &str = "aria2c.exe";

/// Trackers públicos injetados em magnets (mesma lista verificada do Electron).
pub const TRACKERS_PADRAO: &[&str] = &[
    "http://tracker.opentrackr.org:1337/announce",
    "https://tracker.zhuqiy.com:443/announce",
    "https://tracker.bt4g.com:443/announce",
    "http://t.nyaatracker.com:80/announce",
    "http://tracker.mywaifu.best:6969/announce",
    "http://nyaa.tracker.wf:7777/announce",
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
];

/// Campos pedidos por `tellStatus`/`tellActive`/`tellWaiting`/`tellStopped`.
pub const TELL_KEYS: &[&str] = &[
    "gid",
    "status",
    "totalLength",
    "completedLength",
    "downloadSpeed",
    "uploadSpeed",
    "connections",
    "numSeeders",
    "errorCode",
    "errorMessage",
    "files",
    "bittorrent",
    "followedBy",
];

const INFO_HASH_KEYS: &[&str] = &["gid", "infoHash", "status", "dir"];

// ---------------------------------------------------------------------------
// Transporte
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    Io(String),
    Http { status: u16, body: String },
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => f.write_str(message),
            Self::Http { status, body } => {
                write!(f, "aria2: HTTP {status}")?;
                if !body.is_empty() {
                    write!(f, ": {body}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for TransportError {}

/// Transporte injetável: recebe endpoint + corpo JSON e devolve o corpo da
/// resposta. Pode ser HTTP local, um mock ou um canal em memória.
pub trait Aria2Transport: Send + Sync {
    fn post(&self, endpoint: &str, body: &str) -> Result<String, TransportError>;
}

/// Cliente HTTP/1.1 mínimo para o JSON-RPC local do aria2c.
///
/// O daemon escuta apenas em loopback e fala HTTP sem TLS; um cliente TCP
/// próprio evita arrastar um runtime HTTP inteiro para o crate. Suporta
/// `Content-Length` e `Transfer-Encoding: chunked`.
#[derive(Debug, Clone)]
pub struct HttpTransport {
    timeout: Duration,
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new(Duration::from_secs(20))
    }
}

impl HttpTransport {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Aria2Transport for HttpTransport {
    fn post(&self, endpoint: &str, body: &str) -> Result<String, TransportError> {
        let (host, port, path) = parse_http_endpoint(endpoint)?;
        let address = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|error| TransportError::Io(error.to_string()))?
            .next()
            .ok_or_else(|| TransportError::Io("aria2: endpoint sem endereço".to_string()))?;
        let mut stream = TcpStream::connect_timeout(&address, self.timeout)
            .map_err(|error| TransportError::Io(error.to_string()))?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|error| TransportError::Io(error.to_string()))?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|error| TransportError::Io(error.to_string()))?;

        let host_header = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {host_header}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .and_then(|_| stream.flush())
            .map_err(|error| TransportError::Io(error.to_string()))?;

        let (status, response_body) = read_http_response(&mut stream)?;
        if !(200..300).contains(&status) {
            return Err(TransportError::Http {
                status,
                body: response_body,
            });
        }
        Ok(response_body)
    }
}

fn parse_http_endpoint(endpoint: &str) -> Result<(String, u16, String), TransportError> {
    let rest = endpoint
        .strip_prefix("http://")
        .ok_or_else(|| TransportError::Io("aria2: endpoint deve usar http://".to_string()))?;
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(TransportError::Io("aria2: endpoint sem host".to_string()));
    }
    let (host, port_text) = if let Some(after_bracket) = authority.strip_prefix('[') {
        let end = after_bracket
            .find(']')
            .ok_or_else(|| TransportError::Io("aria2: IPv6 inválido".to_string()))?;
        let host = &after_bracket[..end];
        let remainder = &after_bracket[end + 1..];
        let port = remainder.strip_prefix(':').unwrap_or("");
        (host, port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, port),
            Some(_) => {
                return Err(TransportError::Io(
                    "aria2: IPv6 deve vir entre colchetes".to_string(),
                ))
            }
            None => (authority, ""),
        }
    };
    if host.is_empty() {
        return Err(TransportError::Io("aria2: endpoint sem host".to_string()));
    }
    let port = if port_text.is_empty() {
        80
    } else {
        port_text
            .parse::<u16>()
            .map_err(|_| TransportError::Io("aria2: porta inválida".to_string()))?
    };
    Ok((host.to_string(), port, path.to_string()))
}

fn read_http_response(stream: &mut TcpStream) -> Result<(u16, String), TransportError> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| TransportError::Io(error.to_string()))?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(header_end) = find_bytes(&buffer, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..header_end]).to_ascii_lowercase();
            if head.contains("transfer-encoding: chunked") {
                if find_bytes(&buffer[header_end + 4..], b"\r\n0\r\n\r\n").is_some()
                    || buffer.ends_with(b"0\r\n\r\n")
                {
                    break;
                }
            } else if let Some(length) = content_length(&head) {
                if buffer.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
    }
    parse_http_response(&buffer)
}

fn content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse().ok()
        } else {
            None
        }
    })
}

fn parse_http_response(buffer: &[u8]) -> Result<(u16, String), TransportError> {
    let header_end = find_bytes(buffer, b"\r\n\r\n")
        .ok_or_else(|| TransportError::Io("aria2: resposta HTTP sem cabeçalho".to_string()))?;
    let head = String::from_utf8_lossy(&buffer[..header_end]);
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| TransportError::Io("aria2: status HTTP inválido".to_string()))?;
    let body = &buffer[header_end + 4..];
    let lower = head.to_ascii_lowercase();
    let body = if lower.contains("transfer-encoding: chunked") {
        decode_chunked(body)?
    } else if let Some(length) = content_length(&lower) {
        if body.len() < length {
            return Err(TransportError::Io(
                "aria2: resposta HTTP truncada".to_string(),
            ));
        }
        body[..length].to_vec()
    } else {
        body.to_vec()
    };
    let body = String::from_utf8(body)
        .map_err(|_| TransportError::Io("aria2: resposta HTTP não é UTF-8".to_string()))?;
    Ok((status, body))
}

fn decode_chunked(body: &[u8]) -> Result<Vec<u8>, TransportError> {
    let mut output = Vec::new();
    let mut position = 0;
    loop {
        let line_end = find_bytes(&body[position.min(body.len())..], b"\r\n")
            .map(|offset| position + offset)
            .ok_or_else(|| TransportError::Io("aria2: chunk truncado".to_string()))?;
        let line = std::str::from_utf8(&body[position..line_end])
            .map_err(|_| TransportError::Io("aria2: chunk inválido".to_string()))?;
        let size_text = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| TransportError::Io("aria2: chunk inválido".to_string()))?;
        position = line_end + 2;
        if size == 0 {
            break;
        }
        let end = position
            .checked_add(size)
            .filter(|end| *end <= body.len())
            .ok_or_else(|| TransportError::Io("aria2: chunk truncado".to_string()))?;
        output.extend_from_slice(&body[position..end]);
        position = end;
        if body.get(position..position + 2) == Some(b"\r\n") {
            position += 2;
        }
    }
    Ok(output)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ---------------------------------------------------------------------------
// Erros
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aria2Error {
    Transport(String),
    Http { status: u16, body: String },
    InvalidResponse(String),
    Rpc { code: i64, message: String },
}

impl std::fmt::Display for Aria2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(message) => f.write_str(message),
            Self::Http { status, body } => write!(f, "aria2: HTTP {status}: {body}"),
            Self::InvalidResponse(message) => write!(f, "aria2: resposta inválida: {message}"),
            Self::Rpc { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for Aria2Error {}

impl From<TransportError> for Aria2Error {
    fn from(error: TransportError) -> Self {
        match error {
            TransportError::Io(message) => Self::Transport(message),
            TransportError::Http { status, body } => Self::Http { status, body },
        }
    }
}

#[derive(Debug, Deserialize)]
struct RpcErrorBody {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<RpcErrorBody>,
}

// ---------------------------------------------------------------------------
// Cliente JSON-RPC
// ---------------------------------------------------------------------------

/// Cliente JSON-RPC do aria2c. Cada requisição leva `token:<secret>` como
/// primeiro parâmetro, exatamente como o cliente Electron.
pub struct Aria2Client<T: Aria2Transport> {
    endpoint: String,
    secret: String,
    transport: T,
    next_id: AtomicU64,
}

impl<T: Aria2Transport> Aria2Client<T> {
    pub fn new(endpoint: impl Into<String>, secret: impl Into<String>, transport: T) -> Self {
        Self {
            endpoint: endpoint.into(),
            secret: secret.into(),
            transport,
            next_id: AtomicU64::new(1),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Envelope JSON-RPC cru, com o token já prefixado. Útil para métodos
    /// novos do daemon sem duplicar o transporte.
    pub fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, Aria2Error> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut parameters = Vec::with_capacity(params.len() + 1);
        parameters.push(Value::String(format!("token:{}", self.secret)));
        parameters.extend(params);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": parameters,
        });
        let body = serde_json::to_string(&request)
            .map_err(|error| Aria2Error::InvalidResponse(error.to_string()))?;
        let raw = self.transport.post(&self.endpoint, &body)?;
        let response: RpcResponse = serde_json::from_str(&raw)
            .map_err(|error| Aria2Error::InvalidResponse(error.to_string()))?;
        if let Some(error) = response.error {
            let message = if error.message.is_empty() {
                "aria2_rpc_error".to_string()
            } else {
                error.message
            };
            return Err(Aria2Error::Rpc {
                code: error.code,
                message,
            });
        }
        Ok(response.result)
    }

    pub fn get_version(&self) -> Result<Value, Aria2Error> {
        self.call("aria2.getVersion", Vec::new())
    }

    pub fn add_uri(&self, uris: &[String], options: &Value) -> Result<String, Aria2Error> {
        let result = self.call("aria2.addUri", vec![json!(uris), options.clone()])?;
        result
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Aria2Error::InvalidResponse("addUri sem gid".to_string()))
    }

    pub fn force_remove(&self, gid: &str) -> Result<String, Aria2Error> {
        let result = self.call("aria2.forceRemove", vec![json!(gid)])?;
        Ok(result.as_str().unwrap_or_default().to_string())
    }

    pub fn remove_download_result(&self, gid: &str) -> Result<(), Aria2Error> {
        self.call("aria2.removeDownloadResult", vec![json!(gid)])?;
        Ok(())
    }

    /// `remove(gid)`: força remoção e limpa o resultado, tolerando falhas em
    /// cada etapa (como o `try/catch` do oráculo).
    pub fn remove(&self, gid: &str) -> Result<(), Aria2Error> {
        let _ = self.force_remove(gid);
        let _ = self.remove_download_result(gid);
        Ok(())
    }

    pub fn force_pause(&self, gid: &str) -> Result<String, Aria2Error> {
        let result = self.call("aria2.forcePause", vec![json!(gid)])?;
        Ok(result.as_str().unwrap_or_default().to_string())
    }

    pub fn unpause(&self, gid: &str) -> Result<String, Aria2Error> {
        let result = self.call("aria2.unpause", vec![json!(gid)])?;
        Ok(result.as_str().unwrap_or_default().to_string())
    }

    pub fn tell_status(&self, gid: &str, keys: Option<&[&str]>) -> Result<Value, Aria2Error> {
        let mut params = vec![json!(gid)];
        if let Some(keys) = keys {
            params.push(json!(keys));
        }
        self.call("aria2.tellStatus", params)
    }

    pub fn tell_active(&self, keys: Option<&[&str]>) -> Result<Value, Aria2Error> {
        let params = match keys {
            Some(keys) => vec![json!(keys)],
            None => Vec::new(),
        };
        self.call("aria2.tellActive", params)
    }

    pub fn tell_waiting(
        &self,
        offset: u32,
        num: u32,
        keys: Option<&[&str]>,
    ) -> Result<Value, Aria2Error> {
        let mut params = vec![json!(offset), json!(num)];
        if let Some(keys) = keys {
            params.push(json!(keys));
        }
        self.call("aria2.tellWaiting", params)
    }

    pub fn tell_stopped(
        &self,
        offset: u32,
        num: u32,
        keys: Option<&[&str]>,
    ) -> Result<Value, Aria2Error> {
        let mut params = vec![json!(offset), json!(num)];
        if let Some(keys) = keys {
            params.push(json!(keys));
        }
        self.call("aria2.tellStopped", params)
    }

    /// Todos os registros do daemon com o infohash informado (active/waiting/
    /// stopped). Falhas de listagem viram lista vazia, como no oráculo.
    pub fn find_all_by_info_hash(&self, info_hash: &str) -> Vec<Value> {
        if info_hash.is_empty() {
            return Vec::new();
        }
        let wanted = info_hash.to_ascii_lowercase();
        let lists = [
            self.tell_active(Some(INFO_HASH_KEYS))
                .unwrap_or_else(|_| Value::Array(Vec::new())),
            self.tell_waiting(0, 100, Some(INFO_HASH_KEYS))
                .unwrap_or_else(|_| Value::Array(Vec::new())),
            self.tell_stopped(0, 100, Some(INFO_HASH_KEYS))
                .unwrap_or_else(|_| Value::Array(Vec::new())),
        ];
        let mut found = Vec::new();
        for list in lists {
            let Value::Array(items) = list else {
                continue;
            };
            for item in items {
                let value = item
                    .get("infoHash")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if value.to_ascii_lowercase() == wanted {
                    found.push(item);
                }
            }
        }
        found
    }

    /// `addMagnet(magnet, { dir, fileIndices })`.
    ///
    /// Injeta os trackers públicos, monta as opções do oráculo, deduplica por
    /// infohash e só reutiliza um GID ativo/pausado que esteja no MESMO
    /// diretório. Qualquer outro registro é removido antes do `addUri`.
    pub fn add_magnet(
        &self,
        magnet: &str,
        dir: Option<&str>,
        file_indices: Option<&[Value]>,
    ) -> Result<String, Aria2Error> {
        let uri = inject_default_trackers(magnet);
        let mut options = Map::new();
        if let Some(dir) = dir {
            options.insert("dir".to_string(), Value::String(dir.to_string()));
        }
        options.insert("seed-time".to_string(), Value::String("0".to_string()));
        options.insert(
            "summary-interval".to_string(),
            Value::String("0".to_string()),
        );
        options.insert(
            "max-connection-per-server".to_string(),
            Value::String("16".to_string()),
        );
        options.insert("split".to_string(), Value::String("16".to_string()));
        if let Some(indices) = file_indices.filter(|indices| !indices.is_empty()) {
            let select = indices
                .iter()
                .filter_map(|value| {
                    let number = js_number(value)? + 1.0;
                    if number.is_finite() && number.fract() == 0.0 && number > 0.0 {
                        Some(format!("{number:.0}"))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            options.insert("select-file".to_string(), Value::String(select));
        }

        let info_hash = infohash_from_magnet(magnet);
        let existing = self.find_all_by_info_hash(&info_hash);
        let mut reuse: Option<Value> = None;
        for entry in existing {
            let status = entry.get("status").map(js_string).unwrap_or_default();
            let running = matches!(status.as_str(), "active" | "waiting" | "paused");
            let same_dir = same_install_dir(
                entry.get("dir").and_then(Value::as_str).unwrap_or_default(),
                dir.unwrap_or_default(),
            );
            if running && same_dir {
                reuse = Some(entry);
                continue;
            }
            if running {
                let gid = entry.get("gid").and_then(Value::as_str).unwrap_or_default();
                let _ = self.force_remove(gid);
            }
            if let Some(gid) = entry.get("gid").and_then(Value::as_str) {
                let _ = self.remove_download_result(gid);
            }
        }
        if let Some(entry) = reuse {
            if entry.get("status").and_then(Value::as_str) == Some("paused") {
                if let Some(gid) = entry.get("gid").and_then(Value::as_str) {
                    let _ = self.unpause(gid);
                }
            }
            return Ok(entry
                .get("gid")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string());
        }

        self.add_uri(&[uri], &Value::Object(options))
    }

    /// `tell(gid)`: segue a cadeia `followedBy` (metadata -> payload), mapeia
    /// os campos da UI e zera a fase de metadata.
    pub fn tell(&self, gid: &str) -> Result<TellSnapshot, Aria2Error> {
        let mut current = gid.to_string();
        let mut status = self.tell_status(&current, Some(TELL_KEYS))?;
        for _ in 0..3 {
            let next = status
                .get("followedBy")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(Value::as_str)
                .filter(|next| !next.is_empty());
            let Some(next) = next else {
                break;
            };
            current = next.to_string();
            status = self.tell_status(&current, Some(TELL_KEYS))?;
        }
        let mut mapped = map_tell(&status);
        mapped.gid = current;
        let state = status.get("status").and_then(Value::as_str).unwrap_or("");
        if state != "error" && state != "removed" && mapped.file_name.starts_with("[METADATA]") {
            mapped.progress = 0.0;
            mapped.bytes_downloaded = 0.0;
            mapped.file_size = 0.0;
            mapped.download_speed = 0.0;
            mapped.completo = false;
            mapped.state = "active".to_string();
        }
        Ok(mapped)
    }
}

// ---------------------------------------------------------------------------
// Infohash, trackers e diretório
// ---------------------------------------------------------------------------

/// `infohashFromMagnet`: hex 40 (normalizado) ou base32 32 -> hex 40.
pub fn infohash_from_magnet(magnet: &str) -> String {
    let lower = magnet.to_ascii_lowercase();
    let marker = "xt=urn:btih:";
    let Some(index) = lower.find(marker) else {
        return String::new();
    };
    let value: String = magnet[index + marker.len()..]
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .collect();
    if value.len() == 40 && value.chars().all(|character| character.is_ascii_hexdigit()) {
        return value.to_ascii_lowercase();
    }
    if value.len() == 32 {
        return base32_to_hex(&value).unwrap_or_default();
    }
    String::new()
}

fn base32_to_hex(value: &str) -> Option<String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut accumulator: u32 = 0;
    let mut bits = 0u32;
    let mut hex = String::with_capacity(40);
    for character in value.to_ascii_uppercase().bytes() {
        let index = ALPHABET.iter().position(|byte| *byte == character)? as u32;
        accumulator = (accumulator << 5) | index;
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            let byte = ((accumulator >> bits) & 0xff) as u8;
            hex.push_str(&format!("{byte:02x}"));
        }
        if bits == 0 {
            accumulator = 0;
        } else {
            accumulator &= (1 << bits) - 1;
        }
    }
    if hex.len() >= 40 {
        Some(hex[..40].to_string())
    } else {
        None
    }
}

/// `encodeURIComponent` para os caracteres que aparecem em trackers/URLs.
pub fn encode_uri_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => output.push(byte as char),
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

/// `injetarTrackers(magnet, trackers)`.
pub fn inject_trackers(magnet: &str, trackers: &[&str]) -> String {
    if !magnet.to_ascii_lowercase().starts_with("magnet:") {
        return magnet.to_string();
    }
    let missing: Vec<&str> = trackers
        .iter()
        .copied()
        .filter(|tracker| {
            !magnet.contains(tracker) && !magnet.contains(&encode_uri_component(tracker))
        })
        .collect();
    if missing.is_empty() {
        return magnet.to_string();
    }
    let separator = if magnet.contains('?') { '&' } else { '?' };
    let mut output = magnet.to_string();
    for (index, tracker) in missing.iter().enumerate() {
        output.push(if index == 0 { separator } else { '&' });
        output.push_str("tr=");
        output.push_str(&encode_uri_component(tracker));
    }
    output
}

pub fn inject_default_trackers(magnet: &str) -> String {
    inject_trackers(magnet, TRACKERS_PADRAO)
}

/// `mesmoDir(a, b)`: `path.resolve(...).toLowerCase()` dos dois lados.
pub fn same_install_dir(left: &str, right: &str) -> bool {
    lexical_absolute_lower(left) == lexical_absolute_lower(right)
}

fn lexical_absolute_lower(value: &str) -> String {
    let replaced = value.replace('\\', "/");
    let absolute = if replaced.starts_with('/')
        || replaced.as_bytes().get(1).is_some_and(|byte| *byte == b':')
    {
        replaced
    } else {
        let current = std::env::current_dir()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        format!("{current}/{replaced}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in absolute.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let mut normalized = String::new();
    if absolute.starts_with('/') {
        normalized.push('/');
    }
    normalized.push_str(&parts.join("/"));
    normalized.to_ascii_lowercase()
}

// ---------------------------------------------------------------------------
// Mapeamento de status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TellSnapshot {
    pub gid: String,
    pub progress: f64,
    pub bytes_downloaded: f64,
    pub file_size: f64,
    pub download_speed: f64,
    pub upload_speed: f64,
    pub num_peers: f64,
    pub num_seeds: f64,
    pub state: String,
    pub pausado: bool,
    pub folder_name: String,
    pub file_name: String,
    pub erro: String,
    pub completo: bool,
}

fn number_field(value: Option<&Value>) -> f64 {
    value
        .filter(|value| js_truthy(value))
        .and_then(js_number)
        .filter(|number| number.is_finite())
        .unwrap_or(0.0)
}

fn map_tell(status: &Value) -> TellSnapshot {
    let total = number_field(status.get("totalLength"));
    let done = number_field(status.get("completedLength"));
    let complete = status.get("status").and_then(Value::as_str) == Some("complete");
    let progress = if complete {
        1.0
    } else if total > 0.0 {
        (done / total).min(1.0)
    } else {
        0.0
    };
    let first_file = status
        .get("files")
        .and_then(Value::as_array)
        .and_then(|files| files.first());
    let file_name = first_file
        .and_then(|file| file.get("path"))
        .and_then(Value::as_str)
        .map(file_basename)
        .unwrap_or_default();
    let error_message = status
        .get("errorMessage")
        .filter(|value| js_truthy(value))
        .map(js_string);
    let erro = if status.get("status").and_then(Value::as_str) == Some("error") {
        let raw = error_message.unwrap_or_else(|| {
            let code = status
                .get("errorCode")
                .filter(|value| js_truthy(value))
                .map(js_string)
                .unwrap_or_default();
            format!("aria2 erro {code}")
        });
        js_trim(&raw)
    } else {
        String::new()
    };
    TellSnapshot {
        gid: String::new(),
        progress,
        bytes_downloaded: done,
        file_size: total,
        download_speed: number_field(status.get("downloadSpeed")),
        upload_speed: number_field(status.get("uploadSpeed")),
        num_peers: number_field(status.get("connections")),
        num_seeds: number_field(status.get("numSeeders")),
        state: status.get("status").map(js_string).unwrap_or_default(),
        pausado: status.get("status").and_then(Value::as_str) == Some("paused"),
        folder_name: status
            .get("bittorrent")
            .and_then(|torrent| torrent.get("name"))
            .map(js_string)
            .unwrap_or_default(),
        file_name,
        erro,
        completo: progress >= 1.0,
    }
}

fn file_basename(path: &str) -> String {
    path.split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    // Transporte falso: respostas enfileiradas e corpos gravados.
    #[derive(Clone, Default)]
    struct FakeTransport {
        inner: Arc<Mutex<FakeInner>>,
    }

    #[derive(Default)]
    struct FakeInner {
        responses: VecDeque<Result<String, TransportError>>,
        calls: Vec<(String, String)>,
    }

    impl FakeTransport {
        fn new(responses: Vec<Result<String, TransportError>>) -> Self {
            let transport = Self::default();
            transport.inner.lock().unwrap().responses = responses.into();
            transport
        }

        fn json(results: Vec<Value>) -> Self {
            Self::new(
                results
                    .into_iter()
                    .map(|value| Ok(value.to_string()))
                    .collect(),
            )
        }

        fn calls(&self) -> Vec<(String, String)> {
            self.inner.lock().unwrap().calls.clone()
        }
    }

    impl Aria2Transport for FakeTransport {
        fn post(&self, endpoint: &str, body: &str) -> Result<String, TransportError> {
            let mut inner = self.inner.lock().unwrap();
            inner.calls.push((endpoint.to_string(), body.to_string()));
            inner
                .responses
                .pop_front()
                .unwrap_or_else(|| Err(TransportError::Io("fake: sem resposta".to_string())))
        }
    }

    fn client(responses: Vec<Value>) -> Aria2Client<FakeTransport> {
        Aria2Client::new(
            "http://127.0.0.1:6800/jsonrpc",
            "s3cret",
            FakeTransport::json(responses),
        )
    }

    fn bodies(client: &Aria2Client<FakeTransport>) -> Vec<Value> {
        client
            .transport()
            .calls()
            .into_iter()
            .map(|(_, body)| serde_json::from_str(&body).unwrap())
            .collect()
    }

    #[test]
    fn infohash_hex_40_e_base32_32_sao_normalizados() {
        let hex = "481B6E3617BE4C88F96CB25E47C9D8272130071E";
        assert_eq!(
            infohash_from_magnet(&format!("magnet:?xt=urn:btih:{hex}&dn=x")),
            hex.to_ascii_lowercase()
        );
        // Base32 de 20 bytes (32 chars) -> 40 hex.
        let base32 = "JBSNZGCL7YEWSD5GXJOXSPR4Ec4BMA3M";
        let decoded = infohash_from_magnet(&format!("magnet:?xt=urn:btih:{base32}"));
        assert_eq!(decoded.len(), 40);
        assert!(decoded
            .chars()
            .all(|character| character.is_ascii_hexdigit()));
        assert_eq!(infohash_from_magnet("https://example.test"), "");
        assert_eq!(infohash_from_magnet("magnet:?xt=urn:btih:xyz"), "");
    }

    #[test]
    fn trackers_sao_injetados_sem_duplicar_as_duas_formas() {
        let magnet = "magnet:?xt=urn:btih:481b6e3617be4c88f96cb25e47c9d8272130071e&dn=repack";
        let out = inject_default_trackers(magnet);
        assert!(out.starts_with(magnet));
        for tracker in TRACKERS_PADRAO {
            assert!(out.contains(&format!("tr={}", encode_uri_component(tracker))));
        }

        let first = TRACKERS_PADRAO[0];
        for form in [first.to_string(), encode_uri_component(first)] {
            let original = format!("magnet:?xt=urn:btih:{}&tr={form}", "a".repeat(40));
            let out = inject_trackers(&original, TRACKERS_PADRAO);
            let raw = out.matches(first).count();
            let encoded = out.matches(&encode_uri_component(first)).count();
            assert_eq!(raw + encoded, 1, "tracker duplicado: {first}");
        }
        assert_eq!(
            inject_default_trackers("https://example.test/file"),
            "https://example.test/file"
        );
    }

    #[test]
    fn request_jsonrpc_usa_token_id_sequencial_e_metodo() {
        let client = client(vec![
            json!({"result": {"version": "1.37.0"}}),
            json!({"result": "gid-1"}),
        ]);
        client.get_version().unwrap();
        let gid = client
            .add_uri(&["magnet:?xt=x".to_string()], &json!({"dir": "/games"}))
            .unwrap();
        assert_eq!(gid, "gid-1");

        let bodies = bodies(&client);
        assert_eq!(
            bodies[0],
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "aria2.getVersion",
                "params": ["token:s3cret"],
            })
        );
        assert_eq!(
            bodies[1],
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "aria2.addUri",
                "params": ["token:s3cret", ["magnet:?xt=x"], {"dir": "/games"}],
            })
        );
        assert_eq!(bodies[0]["jsonrpc"], "2.0");
        assert_eq!(bodies[0]["params"][0], "token:s3cret");
    }

    #[test]
    fn erro_rpc_vira_codigo_e_mensagem() {
        let client = client(vec![json!({
            "id": 1,
            "error": {"code": 1, "message": "Unauthorized"}
        })]);
        assert_eq!(
            client.get_version(),
            Err(Aria2Error::Rpc {
                code: 1,
                message: "Unauthorized".to_string(),
            })
        );

        let second = self::client(vec![json!({"id": 1, "error": {"code": 2}})]);
        assert_eq!(
            second.get_version(),
            Err(Aria2Error::Rpc {
                code: 2,
                message: "aria2_rpc_error".to_string(),
            })
        );
    }

    #[test]
    fn resposta_invalida_e_falha_de_transporte_sao_reportadas() {
        let transport = FakeTransport::new(vec![Ok("não é json".to_string())]);
        let client = Aria2Client::new("http://127.0.0.1:1/jsonrpc", "x", transport);
        assert!(matches!(
            client.get_version(),
            Err(Aria2Error::InvalidResponse(_))
        ));

        let transport = FakeTransport::new(vec![Err(TransportError::Io(
            "aria2_rpc_timeout".to_string(),
        ))]);
        let client = Aria2Client::new("http://127.0.0.1:1/jsonrpc", "x", transport);
        assert_eq!(
            client.get_version(),
            Err(Aria2Error::Transport("aria2_rpc_timeout".to_string()))
        );

        let third = self::client(vec![json!({"id": 1, "result": null})]);
        assert_eq!(third.get_version(), Ok(Value::Null));
    }

    #[test]
    fn metodos_tell_usam_as_mesmas_chaves_e_parametros() {
        let client = client(vec![
            json!({"result": "OK"}),
            json!({"result": "OK"}),
            json!({"result": "OK"}),
            json!({"result": "OK"}),
            json!({"result": "OK"}),
            json!({"result": "OK"}),
        ]);
        client.force_pause("g1").unwrap();
        client.unpause("g1").unwrap();
        client.remove_download_result("g1").unwrap();
        client.tell_status("g1", Some(TELL_KEYS)).unwrap();
        client.tell_waiting(0, 100, Some(INFO_HASH_KEYS)).unwrap();
        client.tell_stopped(5, 10, None).unwrap();

        let bodies = bodies(&client);
        assert_eq!(bodies[0]["method"], "aria2.forcePause");
        assert_eq!(bodies[0]["params"], json!(["token:s3cret", "g1"]));
        assert_eq!(bodies[1]["method"], "aria2.unpause");
        assert_eq!(bodies[2]["method"], "aria2.removeDownloadResult");
        assert_eq!(bodies[3]["method"], "aria2.tellStatus");
        assert_eq!(bodies[3]["params"][2], json!(TELL_KEYS));
        assert_eq!(
            bodies[4]["params"],
            json!(["token:s3cret", 0, 100, INFO_HASH_KEYS])
        );
        assert_eq!(bodies[5]["params"], json!(["token:s3cret", 5, 10]));
    }

    #[test]
    fn remove_e_best_effort_mesmo_com_falhas() {
        let transport = FakeTransport::new(vec![
            Err(TransportError::Io("dead".to_string())),
            Err(TransportError::Io("dead".to_string())),
        ]);
        let client = Aria2Client::new("http://127.0.0.1:1/jsonrpc", "x", transport);
        assert_eq!(client.remove("g1"), Ok(()));
        assert_eq!(client.transport().calls().len(), 2);
    }

    #[test]
    fn add_magnet_monta_opcoes_e_selecao_1_based() {
        let client = client(vec![
            json!({"result": []}),
            json!({"result": []}),
            json!({"result": []}),
            json!({"result": "gid-new"}),
        ]);
        let gid = client
            .add_magnet(
                "magnet:?xt=urn:btih:481b6e3617be4c88f96cb25e47c9d8272130071e",
                Some("/games/x"),
                Some(&[json!(0), json!(2.0), json!(-2), json!(1.5)]),
            )
            .unwrap();
        assert_eq!(gid, "gid-new");

        let bodies = bodies(&client);
        let add = &bodies[3];
        assert_eq!(add["method"], "aria2.addUri");
        let uri = add["params"][1][0].as_str().unwrap();
        assert!(uri.starts_with("magnet:?xt=urn:btih:"));
        for tracker in TRACKERS_PADRAO {
            assert!(uri.contains(&encode_uri_component(tracker)));
        }
        let options = &add["params"][2];
        assert_eq!(options["dir"], "/games/x");
        assert_eq!(options["seed-time"], "0");
        assert_eq!(options["summary-interval"], "0");
        assert_eq!(options["max-connection-per-server"], "16");
        assert_eq!(options["split"], "16");
        // 0 -> 1, 2 -> 3; -2 -> -1 e 1.5 não são inteiros positivos.
        assert_eq!(options["select-file"], "1,3");
    }

    #[test]
    fn add_magnet_reutiliza_gid_pausado_no_mesmo_diretorio() {
        let info_hash = "481b6e3617be4c88f96cb25e47c9d8272130071e";
        let client = client(vec![
            json!({"result": [{
                "gid": "g-old",
                "infoHash": info_hash,
                "status": "paused",
                "dir": "/games/x",
            }]}),
            json!({"result": []}),
            json!({"result": []}),
            json!({"result": "OK"}),
        ]);
        let gid = client
            .add_magnet(
                &format!("magnet:?xt=urn:btih:{info_hash}"),
                Some("/games/x"),
                None,
            )
            .unwrap();
        assert_eq!(gid, "g-old");
        let bodies = bodies(&client);
        assert_eq!(bodies[3]["method"], "aria2.unpause");
        assert_eq!(bodies.len(), 4);
    }

    #[test]
    fn add_magnet_remove_registros_de_outro_diretorio_antes_de_readicionar() {
        let info_hash = "481b6e3617be4c88f96cb25e47c9d8272130071e";
        let client = client(vec![
            json!({"result": [{
                "gid": "g-active",
                "infoHash": info_hash,
                "status": "active",
                "dir": "/other",
            }]}),
            json!({"result": []}),
            json!({"result": [{
                "gid": "g-stopped",
                "infoHash": info_hash,
                "status": "complete",
                "dir": "/games/x",
            }]}),
            json!({"result": "OK"}),
            json!({"result": "OK"}),
            json!({"result": "OK"}),
            json!({"result": "gid-new"}),
        ]);
        let gid = client
            .add_magnet(
                &format!("magnet:?xt=urn:btih:{info_hash}"),
                Some("/games/x"),
                None,
            )
            .unwrap();
        assert_eq!(gid, "gid-new");
        let methods: Vec<String> = bodies(&client)
            .iter()
            .map(|body| body["method"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            methods,
            vec![
                "aria2.tellActive",
                "aria2.tellWaiting",
                "aria2.tellStopped",
                "aria2.forceRemove",
                "aria2.removeDownloadResult",
                "aria2.removeDownloadResult",
                "aria2.addUri",
            ]
        );
    }

    #[test]
    fn tell_mapeia_campos_da_ui_e_erro() {
        let client = client(vec![json!({"result": {
            "gid": "g1",
            "status": "error",
            "totalLength": "1000",
            "completedLength": "250",
            "downloadSpeed": "99",
            "uploadSpeed": "7",
            "connections": "3",
            "numSeeders": "2",
            "errorCode": "1",
            "errorMessage": "  falhou  ",
            "files": [{"path": "/downloads/jogo/arquivo.bin"}],
            "bittorrent": {"name": "jogo"},
        }})]);
        let snapshot = client.tell("g1").unwrap();
        assert_eq!(snapshot.gid, "g1");
        assert_eq!(snapshot.progress, 0.25);
        assert_eq!(snapshot.bytes_downloaded, 250.0);
        assert_eq!(snapshot.file_size, 1000.0);
        assert_eq!(snapshot.download_speed, 99.0);
        assert_eq!(snapshot.upload_speed, 7.0);
        assert_eq!(snapshot.num_peers, 3.0);
        assert_eq!(snapshot.num_seeds, 2.0);
        assert_eq!(snapshot.state, "error");
        assert!(!snapshot.pausado);
        assert_eq!(snapshot.folder_name, "jogo");
        assert_eq!(snapshot.file_name, "arquivo.bin");
        assert_eq!(snapshot.erro, "falhou");
        assert!(!snapshot.completo);
    }

    #[test]
    fn tell_segue_followed_by_e_zera_fase_de_metadata() {
        let client = client(vec![
            json!({"result": {"gid": "meta", "status": "active", "followedBy": ["real"]}}),
            json!({"result": {
                "gid": "real",
                "status": "complete",
                "totalLength": "10",
                "completedLength": "10",
                "files": [{"path": "/x/[METADATA]jogo"}],
            }}),
        ]);
        let snapshot = client.tell("meta").unwrap();
        assert_eq!(snapshot.gid, "real");
        assert_eq!(snapshot.progress, 0.0);
        assert_eq!(snapshot.bytes_downloaded, 0.0);
        assert_eq!(snapshot.file_size, 0.0);
        assert!(!snapshot.completo);
        assert_eq!(snapshot.state, "active");
        assert_eq!(snapshot.file_name, "[METADATA]jogo");
    }

    #[test]
    fn completed_sem_total_conta_como_100_por_cento() {
        let client = client(vec![json!({"result": {
            "status": "complete", "totalLength": "0", "completedLength": "0"
        }})]);
        let snapshot = client.tell("g").unwrap();
        assert_eq!(snapshot.progress, 1.0);
        assert!(snapshot.completo);
    }

    #[test]
    fn find_all_by_info_hash_filtra_case_insensitive() {
        let client = client(vec![
            json!({"result": [
                {"gid": "a", "infoHash": "ABCDEF", "status": "active", "dir": "/x"},
                {"gid": "b", "infoHash": "zzz", "status": "active", "dir": "/x"},
            ]}),
            json!({"result": [
                {"gid": "c", "infoHash": "abcdef", "status": "paused", "dir": "/x"},
            ]}),
            json!({"result": []}),
        ]);
        let found = client.find_all_by_info_hash("abcdef");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0]["gid"], "a");
        assert_eq!(found[1]["gid"], "c");
    }

    // --- Servidor HTTP local (sem rede externa) ---

    fn read_request_body(stream: &mut TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let read = stream.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(header_end) = find_bytes(&buffer, b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buffer[..header_end]).to_ascii_lowercase();
                let length = content_length(&head).unwrap_or(0);
                if buffer.len() >= header_end + 4 + length {
                    return String::from_utf8(
                        buffer[header_end + 4..header_end + 4 + length].to_vec(),
                    )
                    .unwrap();
                }
            }
        }
        String::new()
    }

    fn mock_server(
        responses: Vec<(u16, String)>,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let requests = Arc::clone(&recorded);
        let handle = thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request_body(&mut stream);
                requests.lock().unwrap().push(request);
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                stream.flush().unwrap();
            }
        });
        (format!("http://{address}/jsonrpc"), recorded, handle)
    }

    #[test]
    fn transporte_http_fala_jsonrpc_com_content_length() {
        let (endpoint, recorded, handle) = mock_server(vec![
            (
                200,
                json!({"id": 1, "result": {"version": "1.37.0"}}).to_string(),
            ),
            (200, json!({"id": 2, "result": "gid"}).to_string()),
        ]);
        let client = Aria2Client::new(endpoint, "tok", HttpTransport::default());
        assert_eq!(client.get_version().unwrap()["version"], "1.37.0");
        assert_eq!(
            client
                .add_uri(&["magnet:?xt=x".to_string()], &json!({}))
                .unwrap(),
            "gid"
        );
        handle.join().unwrap();

        let requests = recorded.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let first: Value = serde_json::from_str(&requests[0]).unwrap();
        assert_eq!(first["method"], "aria2.getVersion");
        assert_eq!(first["params"], json!(["token:tok"]));
    }

    #[test]
    fn transporte_http_mapeia_status_nao_2xx() {
        let (endpoint, _recorded, handle) = mock_server(vec![(500, "boom".to_string())]);
        let client = Aria2Client::new(endpoint, "tok", HttpTransport::default());
        assert_eq!(
            client.get_version(),
            Err(Aria2Error::Http {
                status: 500,
                body: "boom".to_string(),
            })
        );
        handle.join().unwrap();
    }

    #[test]
    fn transporte_http_decodifica_chunked() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let payload = json!({"id": 1, "result": {"a": 1}}).to_string();
        let chunk = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
            payload.len(),
            payload
        );
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request_body(&mut stream);
            stream.write_all(chunk.as_bytes()).unwrap();
            stream.flush().unwrap();
        });
        let client = Aria2Client::new(
            format!("http://{address}/jsonrpc"),
            "tok",
            HttpTransport::default(),
        );
        assert_eq!(client.get_version().unwrap(), json!({"a": 1}));
        handle.join().unwrap();
    }

    #[test]
    fn endpoint_http_invalido_falha_antes_de_conectar() {
        let transport = HttpTransport::default();
        assert!(matches!(
            transport.post("https://127.0.0.1/jsonrpc", "{}"),
            Err(TransportError::Io(_))
        ));
        assert!(matches!(
            transport.post("http://[::1/jsonrpc", "{}"),
            Err(TransportError::Io(_))
        ));
    }

    #[test]
    fn encode_uri_component_preserva_o_conjunto_do_javascript() {
        assert_eq!(
            encode_uri_component("http://a.b:1/ann?x=1"),
            "http%3A%2F%2Fa.b%3A1%2Fann%3Fx%3D1"
        );
        assert_eq!(encode_uri_component("a-b_c.d!~*'()"), "a-b_c.d!~*'()");
    }

    #[test]
    fn mesmo_diretorio_normaliza_separadores_e_caixa() {
        assert!(same_install_dir("/Games/X", "/games/x/"));
        assert!(same_install_dir("C:\\Games\\X", "c:/games/x"));
        assert!(!same_install_dir("/games/x", "/games/y"));
        assert!(same_install_dir("/games/a/../x", "/games/x"));
        assert!(!same_install_dir("/games/x", ""));
    }

    #[test]
    fn basename_aceita_separadores_dos_dois_sistemas() {
        assert_eq!(file_basename("/a/b/c.bin"), "c.bin");
        assert_eq!(file_basename("C:\\a\\b\\c.bin"), "c.bin");
        assert_eq!(file_basename("/a/b/"), "b");
    }
}
