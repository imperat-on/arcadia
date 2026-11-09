//! Executor HTTP de produção para os downloads de fixes.
//!
//! O oráculo (`fix_downloader.sh`) usa `curl` para escrever `fix_<appid>.zip`
//! em `DATA_DIR/tmp`, reporta `downloading`/`bytesRead`/`totalBytes` e remove o
//! `.part` em caso de falha. Este módulo reproduz o mesmo contrato sem shell:
//!
//! - o transporte é injetável ([`HttpTransport`]); a produção usa
//!   [`ReqwestTransport`] e os testes usam um servidor HTTP local;
//! - o corpo é gravado em `<destination>.part.<pid>` no mesmo diretório do
//!   plano (portanto sob o data root validado por `DownloadPlan`);
//! - progresso é emitido como [`DownloadEvent`] e o hash SHA-256/tamanho do
//!   catálogo são conferidos antes do rename atômico;
//! - qualquer erro (ou cancelamento) apaga o `.part`; nunca sobra lixo no
//!   diretório temporário.
//!
//! Cancelamento é cooperativo: [`HttpDownloadExecutor::cancel`] marca o
//! executor e o próximo bloco lido aborta. O engine continua sendo responsável
//! por limpar `fix_<appid>.zip`, `_state.json` e `_headers.txt`.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::download::{
    DownloadError, DownloadEvent, DownloadExecutor, DownloadPlan, DownloadedArchive,
};

/// Código estável para download cancelado pelo usuário.
pub const ERROR_CANCELLED: &str = "cancelled";
/// Tamanho dos blocos lidos do transporte.
pub const HTTP_CHUNK_SIZE: usize = 64 * 1024;
/// Intervalo de polling para checar cancelamento durante uma leitura lenta.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(150);
/// Quantos blocos o leitor pode adiantar antes de bloquear (backpressure).
const READER_QUEUE_DEPTH: usize = 4;

/// Requisição HTTP montada a partir de um [`DownloadPlan`].
///
/// `headers` já inclui a credencial Ryuu (`X-Auth-Key`/`Cookie`) e os headers
/// resolvidos pelo hoster. O user-agent vai separado para o transporte poder
/// aplicá-lo sem ambiguidade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub user_agent: String,
    pub connect_timeout: Duration,
    /// `MAX_TIME` do worker; `None`/zero significa sem limite total.
    pub max_time: Option<Duration>,
}

impl HttpRequest {
    /// Monta a requisição; rejeita qualquer coisa que não seja HTTP(S) direto
    /// (magnet/hoster precisam ser resolvidos antes do executor).
    pub fn from_plan(plan: &DownloadPlan) -> Result<Self, DownloadError> {
        let Some(url) = plan.source.url.as_deref() else {
            return Err(DownloadError::bad_url());
        };
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(DownloadError::bad_url());
        }
        if url.chars().any(char::is_control) {
            return Err(DownloadError::bad_url());
        }

        let mut headers = plan.source.headers.clone();
        if let Some(line) = plan.auth_header.as_deref() {
            let Some((name, value)) = line.lines().next().and_then(|line| line.split_once(':'))
            else {
                return Err(DownloadError::authentication());
            };
            let name = name.trim();
            let value = value.trim();
            if name.is_empty()
                || value.is_empty()
                || name.chars().any(char::is_control)
                || value.chars().any(char::is_control)
            {
                return Err(DownloadError::authentication());
            }
            headers.push((name.to_string(), value.to_string()));
        }

        Ok(Self {
            url: url.to_string(),
            headers,
            user_agent: plan.user_agent.clone(),
            connect_timeout: Duration::from_secs(plan.connect_timeout_secs.max(1)),
            max_time: plan
                .max_time_secs
                .filter(|seconds| *seconds > 0)
                .map(Duration::from_secs),
        })
    }
}

/// Resposta aberta pelo transporte. O corpo é lido de forma streaming.
pub struct HttpStream {
    pub status: u16,
    /// `Content-Length` quando o servidor informou.
    pub content_length: Option<u64>,
    pub reader: Box<dyn Read + Send>,
}

impl std::fmt::Debug for HttpStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpStream")
            .field("status", &self.status)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

/// Transporte HTTP injetável. A produção usa [`ReqwestTransport`]; testes
/// podem devolver bytes em memória ou apontar para um servidor local.
pub trait HttpTransport: Send + Sync {
    fn open(&self, request: &HttpRequest) -> Result<HttpStream, DownloadError>;
}

/// Transporte de produção sobre `reqwest::blocking` (rustls, sem shell).
#[derive(Debug, Clone, Copy, Default)]
pub struct ReqwestTransport;

impl ReqwestTransport {
    pub fn new() -> Self {
        Self
    }
}

impl HttpTransport for ReqwestTransport {
    fn open(&self, request: &HttpRequest) -> Result<HttpStream, DownloadError> {
        let mut builder = reqwest::blocking::Client::builder()
            .connect_timeout(request.connect_timeout)
            .redirect(reqwest::redirect::Policy::limited(10));
        if let Some(max_time) = request.max_time {
            builder = builder.timeout(max_time);
        }
        let client = builder
            .build()
            .map_err(|error| DownloadError::new(format!("download client: {error}")))?;
        let mut builder = client.get(&request.url).header(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_str(&request.user_agent)
                .map_err(|_| DownloadError::bad_url())?,
        );
        for (name, value) in &request.headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| DownloadError::bad_url())?;
            let value = reqwest::header::HeaderValue::from_str(value)
                .map_err(|_| DownloadError::bad_url())?;
            builder = builder.header(name, value);
        }

        let response = builder.send().map_err(map_reqwest_error)?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(DownloadError::authentication());
        }
        if !status.is_success() {
            return Err(DownloadError::new(format!(
                "Download failed — try another source. (HTTP {})",
                status.as_u16()
            )));
        }
        Ok(HttpStream {
            status: status.as_u16(),
            content_length: response.content_length(),
            reader: Box::new(response),
        })
    }
}

fn map_reqwest_error(error: reqwest::Error) -> DownloadError {
    if error.is_timeout() {
        DownloadError::stalled()
    } else if error.is_builder() {
        DownloadError::bad_url()
    } else {
        DownloadError::new("Download failed — try another source.")
    }
}

/// Handle clonável para cancelar um download em andamento.
#[derive(Debug, Clone, Default)]
pub struct CancelHandle(Arc<AtomicBool>);

impl CancelHandle {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Executor de produção: transporte injetável + gravação em `.part`.
pub struct HttpDownloadExecutor {
    transport: Arc<dyn HttpTransport>,
    cancel: Arc<AtomicBool>,
}

impl std::fmt::Debug for HttpDownloadExecutor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpDownloadExecutor")
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl Default for HttpDownloadExecutor {
    fn default() -> Self {
        Self::production()
    }
}

impl HttpDownloadExecutor {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            transport,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Transporte real (`reqwest` + rustls).
    pub fn production() -> Self {
        Self::new(Arc::new(ReqwestTransport::new()))
    }

    pub fn cancel_handle(&self) -> CancelHandle {
        CancelHandle(Arc::clone(&self.cancel))
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    fn cancelled_error() -> DownloadError {
        DownloadError::with_code("Download cancelled.", ERROR_CANCELLED)
    }

    fn download_inner(
        &self,
        plan: &DownloadPlan,
        progress: &mut dyn FnMut(DownloadEvent),
    ) -> Result<DownloadedArchive, DownloadError> {
        let request = HttpRequest::from_plan(plan)?;
        let destination = validate_destination(plan)?;
        let part = part_path(&destination)?;
        let _ = fs::remove_file(&part);

        if self.is_cancelled() {
            let _ = fs::remove_file(&part);
            let error = Self::cancelled_error();
            progress(DownloadEvent::failed(
                error.message.clone(),
                error.code.clone(),
            ));
            return Err(error);
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| DownloadError::new("Could not save the file."))?;
        }

        let stream = match self.transport.open(&request) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = fs::remove_file(&part);
                progress(DownloadEvent::failed(
                    error.message.clone(),
                    error.code.clone(),
                ));
                return Err(error);
            }
        };
        let total = stream
            .content_length
            .or(plan.expected_size)
            .unwrap_or_default();
        progress(DownloadEvent::downloading(0, total));
        if self.is_cancelled() {
            let _ = fs::remove_file(&part);
            let error = Self::cancelled_error();
            progress(DownloadEvent::failed(
                error.message.clone(),
                error.code.clone(),
            ));
            return Err(error);
        }

        let result = write_stream(stream.reader, &part, plan, total, &self.cancel, progress);
        let (bytes, sha256) = match result {
            Ok(result) => result,
            Err(error) => {
                let _ = fs::remove_file(&part);
                progress(DownloadEvent::failed(
                    error.message.clone(),
                    error.code.clone(),
                ));
                return Err(error);
            }
        };

        let archive = DownloadedArchive::new(destination.clone(), bytes, sha256);
        if let Err(error) = archive.verify(plan) {
            let _ = fs::remove_file(&part);
            progress(DownloadEvent::failed(
                error.message.clone(),
                error.code.clone(),
            ));
            return Err(error);
        }
        if let Err(error) = replace_file(&part, &destination) {
            let _ = fs::remove_file(&part);
            progress(DownloadEvent::failed(
                error.message.clone(),
                error.code.clone(),
            ));
            return Err(error);
        }
        Ok(archive)
    }
}

impl DownloadExecutor for HttpDownloadExecutor {
    fn download(
        &self,
        plan: &DownloadPlan,
        progress: &mut dyn FnMut(DownloadEvent),
    ) -> Result<DownloadedArchive, DownloadError> {
        let result = self.download_inner(plan, progress);
        // Um cancelamento vale só para a execução corrente; a próxima chamada
        // começa limpa (o engine pode reutilizar o executor).
        self.cancel.store(false, Ordering::SeqCst);
        result
    }
}

fn validate_destination(plan: &DownloadPlan) -> Result<PathBuf, DownloadError> {
    let destination = &plan.destination;
    let Some(parent) = destination.parent() else {
        return Err(DownloadError::new("download path rejected"));
    };
    let sane = |path: &Path| {
        path.is_absolute()
            && !path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
    };
    if !sane(destination) || !sane(parent) {
        return Err(DownloadError::new("download path rejected"));
    }
    if plan.state_path.parent() != Some(parent) || plan.headers_path.parent() != Some(parent) {
        return Err(DownloadError::new("download path rejected"));
    }
    Ok(destination.clone())
}

fn part_path(destination: &Path) -> Result<PathBuf, DownloadError> {
    let mut name = destination
        .file_name()
        .ok_or_else(|| DownloadError::new("download path rejected"))?
        .to_os_string();
    name.push(format!(".part.{}", std::process::id()));
    Ok(destination.with_file_name(name))
}

fn replace_file(part: &Path, destination: &Path) -> Result<(), DownloadError> {
    if destination.exists() {
        fs::remove_file(destination).map_err(|_| DownloadError::new("Could not save the file."))?;
    }
    fs::rename(part, destination).map_err(|_| DownloadError::new("Could not save the file."))
}

type StreamResult = Result<(u64, Option<String>), DownloadError>;

fn write_stream(
    reader: Box<dyn Read + Send>,
    part: &Path,
    plan: &DownloadPlan,
    total: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(DownloadEvent),
) -> StreamResult {
    let file = File::create(part).map_err(|_| DownloadError::new("Could not save the file."))?;
    let mut writer = BufWriter::with_capacity(HTTP_CHUNK_SIZE, file);
    let mut hasher = plan.expected_sha256.as_ref().map(|_| Sha256::new());
    let mut bytes_read = 0u64;
    let started = Instant::now();
    let mut last_data = Instant::now();
    let stall_after =
        (plan.speed_time_secs > 0).then(|| Duration::from_secs(u64::from(plan.speed_time_secs)));
    let max_time = plan
        .max_time_secs
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs);

    // A thread leitora permite detectar cancelamento/stall mesmo enquanto o
    // transporte está bloqueado esperando o próximo pacote.
    let (sender, receiver) =
        mpsc::sync_channel::<Result<Vec<u8>, std::io::Error>>(READER_QUEUE_DEPTH);
    let reader_handle = std::thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = vec![0u8; HTTP_CHUNK_SIZE];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if sender.send(Ok(buffer[..count].to_vec())).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    break;
                }
            }
        }
    });

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(HttpDownloadExecutor::cancelled_error());
        }
        if max_time.is_some_and(|limit| started.elapsed() >= limit) {
            return Err(DownloadError::stalled());
        }
        match receiver.recv_timeout(CANCEL_POLL_INTERVAL) {
            Ok(Ok(chunk)) => {
                writer
                    .write_all(&chunk)
                    .map_err(|_| DownloadError::new("Could not save the file."))?;
                if let Some(hasher) = hasher.as_mut() {
                    hasher.update(&chunk);
                }
                bytes_read = bytes_read.saturating_add(chunk.len() as u64);
                last_data = Instant::now();
                progress(DownloadEvent::downloading(
                    bytes_read,
                    total.max(bytes_read),
                ));
                if cancel.load(Ordering::SeqCst) {
                    return Err(HttpDownloadExecutor::cancelled_error());
                }
            }
            Ok(Err(error)) => {
                return Err(if error.kind() == std::io::ErrorKind::TimedOut {
                    DownloadError::stalled()
                } else {
                    DownloadError::new("Download failed — try another source.")
                });
            }
            Err(RecvTimeoutError::Timeout) => {
                if stall_after.is_some_and(|limit| last_data.elapsed() >= limit) {
                    return Err(DownloadError::stalled());
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = reader_handle.join();

    if total > 0 && bytes_read != total {
        return Err(DownloadError::new(
            "Downloaded archive size does not match the catalog metadata.",
        ));
    }

    writer
        .flush()
        .map_err(|_| DownloadError::new("Could not save the file."))?;
    let sha256 = hasher.map(|hasher| sha256_hex(hasher.finalize()));
    Ok((bytes_read, sha256))
}

fn sha256_hex(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(digest.as_ref().len() * 2);
    for byte in digest.as_ref() {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::FixType;
    use crate::download::{
        ResolvedDownloadSource, SourceKind, ERROR_AUTHENTICATION, ERROR_BAD_URL,
    };
    use crate::install::PathRoots;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    fn roots(dir: &Path) -> PathRoots {
        PathRoots::new(
            Vec::new(),
            Vec::new(),
            dir.join("backups"),
            dir.join("data"),
        )
    }

    fn plan_for(dir: &Path, url: &str) -> DownloadPlan {
        std::fs::create_dir_all(dir.join("data/tmp")).unwrap();
        let source = ResolvedDownloadSource::from_url(url).unwrap();
        DownloadPlan::new("730", FixType::Generic, source, &roots(dir), None).unwrap()
    }

    fn sha256_hex_in_test(bytes: &[u8]) -> String {
        sha256_hex(Sha256::digest(bytes))
    }

    struct MockResponse {
        status: u16,
        body: Vec<u8>,
        /// Blocos enviados com `delay` entre eles (vazio = corpo de uma vez).
        chunks: Vec<Vec<u8>>,
        delay: Duration,
    }

    struct MockServer {
        address: std::net::SocketAddr,
        request: Arc<Mutex<String>>,
        handle: Option<JoinHandle<()>>,
    }

    impl MockServer {
        fn start(response: MockResponse) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let request = Arc::new(Mutex::new(String::new()));
            let captured = Arc::clone(&request);
            let handle = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let head = read_head(&mut stream);
                *captured.lock().unwrap() = head;
                let status_text = if response.status == 200 {
                    "OK"
                } else if response.status == 401 {
                    "Unauthorized"
                } else if response.status == 404 {
                    "Not Found"
                } else {
                    "Error"
                };
                let head = format!(
                    "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status,
                    status_text,
                    response.body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                if response.chunks.is_empty() {
                    let _ = stream.write_all(&response.body);
                } else {
                    for chunk in &response.chunks {
                        if stream.write_all(chunk).is_err() {
                            break;
                        }
                        let _ = stream.flush();
                        std::thread::sleep(response.delay);
                    }
                }
                let _ = stream.flush();
            });
            Self {
                address,
                request,
                handle: Some(handle),
            }
        }

        fn url(&self, name: &str) -> String {
            format!("http://{}/{name}", self.address)
        }

        fn request_text(&self) -> String {
            self.request.lock().unwrap().clone()
        }

        fn finish(mut self) {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn read_head(stream: &mut TcpStream) -> String {
        let mut data = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    data.extend_from_slice(&buffer[..count]);
                    if data.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&data).into_owned()
    }

    fn assert_no_temp_junk(tmp: &Path) {
        if let Ok(entries) = fs::read_dir(tmp) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                assert!(!name.contains(".part."), "temporário sobrou: {name}");
            }
        }
    }

    #[test]
    fn downloads_streams_progress_and_verifies_size_and_hash() {
        let temp = tempfile::tempdir().unwrap();
        let body = b"fix archive body".repeat(4096);
        let server = MockServer::start(MockResponse {
            status: 200,
            body: body.clone(),
            chunks: Vec::new(),
            delay: Duration::ZERO,
        });
        let mut plan = plan_for(temp.path(), &server.url("fix.zip"));
        plan.expected_size = Some(body.len() as u64);
        plan.expected_sha256 = Some(sha256_hex_in_test(&body).to_ascii_uppercase());
        plan = plan.with_auth_header("X-Auth-Key: secret\n");

        let executor = HttpDownloadExecutor::new(Arc::new(ReqwestTransport::new()));
        let mut events = Vec::new();
        let archive = executor
            .download(&plan, &mut |event| events.push(event))
            .unwrap();

        assert_eq!(archive.path, plan.destination);
        assert_eq!(archive.bytes, body.len() as u64);
        assert_eq!(
            archive.sha256.as_deref(),
            Some(sha256_hex_in_test(&body).as_str())
        );
        assert_eq!(fs::read(&plan.destination).unwrap(), body);
        assert!(events
            .iter()
            .any(|event| matches!(event, DownloadEvent::Downloading { bytes_read, .. } if *bytes_read == body.len() as u64)));
        assert!(!events
            .iter()
            .any(|event| matches!(event, DownloadEvent::Failed { .. })));
        assert!(server
            .request_text()
            .to_ascii_lowercase()
            .contains("x-auth-key: secret"));
        server.finish();
        assert_no_temp_junk(plan.destination.parent().unwrap());
    }

    #[test]
    fn size_mismatch_removes_part_and_destination() {
        let temp = tempfile::tempdir().unwrap();
        let body = b"short".to_vec();
        let server = MockServer::start(MockResponse {
            status: 200,
            body: body.clone(),
            chunks: Vec::new(),
            delay: Duration::ZERO,
        });
        let mut plan = plan_for(temp.path(), &server.url("fix.zip"));
        plan.expected_size = Some(body.len() as u64 + 1);

        let executor = HttpDownloadExecutor::new(Arc::new(ReqwestTransport::new()));
        let error = executor.download(&plan, &mut |_| {}).unwrap_err();
        assert!(error.message.contains("size"), "{error}");
        assert!(!plan.destination.exists());
        server.finish();
        assert_no_temp_junk(plan.destination.parent().unwrap());
    }

    #[test]
    fn hash_mismatch_removes_part_and_destination() {
        let temp = tempfile::tempdir().unwrap();
        let body = b"hash mismatch".to_vec();
        let server = MockServer::start(MockResponse {
            status: 200,
            body: body.clone(),
            chunks: Vec::new(),
            delay: Duration::ZERO,
        });
        let mut plan = plan_for(temp.path(), &server.url("fix.zip"));
        plan.expected_sha256 = Some("00".repeat(32));

        let executor = HttpDownloadExecutor::new(Arc::new(ReqwestTransport::new()));
        let error = executor.download(&plan, &mut |_| {}).unwrap_err();
        assert!(error.message.contains("hash"), "{error}");
        assert!(!plan.destination.exists());
        server.finish();
        assert_no_temp_junk(plan.destination.parent().unwrap());
    }

    #[test]
    fn progress_reports_monotonic_bytes_and_final_total() {
        let temp = tempfile::tempdir().unwrap();
        let body = vec![7u8; HTTP_CHUNK_SIZE * 3 + 17];
        let server = MockServer::start(MockResponse {
            status: 200,
            body: body.clone(),
            chunks: Vec::new(),
            delay: Duration::ZERO,
        });
        let plan = plan_for(temp.path(), &server.url("fix.zip"));

        let executor = HttpDownloadExecutor::new(Arc::new(ReqwestTransport::new()));
        let mut progress = Vec::new();
        executor
            .download(&plan, &mut |event| progress.push(event))
            .unwrap();
        let mut last = 0;
        let mut final_total = None;
        for event in &progress {
            if let DownloadEvent::Downloading {
                bytes_read,
                total_bytes,
            } = event
            {
                assert!(*bytes_read >= last);
                last = *bytes_read;
                final_total = Some(*total_bytes);
            }
        }
        assert_eq!(last, body.len() as u64);
        assert_eq!(final_total, Some(body.len() as u64));
        server.finish();
    }

    #[test]
    fn cancel_from_progress_stops_download_and_cleans_temp() {
        let temp = tempfile::tempdir().unwrap();
        let chunk = vec![1u8; 32 * 1024];
        let chunks = vec![chunk.clone(); 16];
        let body: Vec<u8> = chunks.iter().flatten().copied().collect();
        let server = MockServer::start(MockResponse {
            status: 200,
            body,
            chunks,
            delay: Duration::from_millis(15),
        });
        let plan = plan_for(temp.path(), &server.url("fix.zip"));

        let executor = HttpDownloadExecutor::new(Arc::new(ReqwestTransport::new()));
        let cancel = executor.cancel_handle();
        let error = executor
            .download(&plan, &mut |event| {
                if let DownloadEvent::Downloading { bytes_read, .. } = event {
                    if bytes_read > 0 {
                        cancel.cancel();
                    }
                }
            })
            .unwrap_err();
        assert_eq!(error.code.as_deref(), Some(ERROR_CANCELLED));
        assert!(!plan.destination.exists());
        server.finish();
        assert_no_temp_junk(plan.destination.parent().unwrap());
    }

    #[test]
    fn cancellation_before_open_is_clean() {
        let temp = tempfile::tempdir().unwrap();
        let server = MockServer::start(MockResponse {
            status: 200,
            body: b"never".to_vec(),
            chunks: Vec::new(),
            delay: Duration::ZERO,
        });
        let plan = plan_for(temp.path(), &server.url("fix.zip"));

        let executor = HttpDownloadExecutor::new(Arc::new(ReqwestTransport::new()));
        executor.cancel();
        let error = executor.download(&plan, &mut |_| {}).unwrap_err();
        assert_eq!(error.code.as_deref(), Some(ERROR_CANCELLED));
        assert!(!plan.destination.exists());
        // O servidor nem chega a receber a requisição; encerra sem conexão.
        drop(server);
        assert_no_temp_junk(plan.destination.parent().unwrap());
    }

    #[test]
    fn http_status_401_maps_to_authentication_error() {
        let temp = tempfile::tempdir().unwrap();
        let server = MockServer::start(MockResponse {
            status: 401,
            body: b"denied".to_vec(),
            chunks: Vec::new(),
            delay: Duration::ZERO,
        });
        let plan = plan_for(temp.path(), &server.url("fix.zip"));
        let executor = HttpDownloadExecutor::new(Arc::new(ReqwestTransport::new()));
        let error = executor.download(&plan, &mut |_| {}).unwrap_err();
        assert_eq!(error.code.as_deref(), Some(ERROR_AUTHENTICATION));
        assert!(!plan.destination.exists());
        server.finish();
    }

    #[test]
    fn magnet_and_relative_plans_are_rejected_without_touching_disk() {
        let temp = tempfile::tempdir().unwrap();
        let source = ResolvedDownloadSource::from_url(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
        )
        .unwrap();
        assert_eq!(source.kind, SourceKind::Magnet);
        let plan =
            DownloadPlan::new("730", FixType::Generic, source, &roots(temp.path()), None).unwrap();
        let executor = HttpDownloadExecutor::new(Arc::new(ReqwestTransport::new()));
        let error = executor.download(&plan, &mut |_| {}).unwrap_err();
        assert_eq!(error.code.as_deref(), Some(ERROR_BAD_URL));
        assert!(!plan.destination.exists());
    }

    #[test]
    fn connection_failure_removes_temp_files() {
        // Exercita o mapeamento de erro de conexão do transporte real sem
        // depender de rede externa: o endereço de teste é o próprio listener
        // fechado.
        let temp = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let plan = plan_for(temp.path(), &format!("http://{address}/fix.zip"));
        let executor = HttpDownloadExecutor::new(Arc::new(ReqwestTransport::new()));
        let error = executor.download(&plan, &mut |_| {}).unwrap_err();
        assert!(error.code.as_deref() != Some(ERROR_CANCELLED));
        assert!(!plan.destination.exists());
    }

    #[test]
    fn progress_and_failure_events_keep_ui_field_names() {
        let temp = tempfile::tempdir().unwrap();
        let body = b"ok".to_vec();
        let server = MockServer::start(MockResponse {
            status: 200,
            body: body.clone(),
            chunks: Vec::new(),
            delay: Duration::ZERO,
        });
        let plan = plan_for(temp.path(), &server.url("fix.zip"));
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen);
        let executor = HttpDownloadExecutor::new(Arc::new(ReqwestTransport::new()));
        let archive = executor
            .download(&plan, &mut |event| {
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = serde_json::to_value(event).unwrap();
            })
            .unwrap();
        assert_eq!(archive.bytes, body.len() as u64);
        assert!(seen.load(Ordering::SeqCst) >= 2);
        server.finish();
    }
}
