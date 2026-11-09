//! Comandos do serviço de trailers (sem executar processos).
//!
//! Port das partes puras de `app/electron/trailer-service.js`: nome seguro,
//! caminho local, argumentos de `yt-dlp` para busca/stream/download, cookies e
//! parsing do `--dump-json`/progresso. Spawn de processo fica no chamador
//! (Tauri), fora desta camada.

use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::value_number;

/// Extensões procuradas por `localPath`, na ordem do cliente.
pub const TRAILER_EXTENSIONS: [&str; 3] = [".mp4", ".webm", ".mkv"];

/// Formato de download usado por `download` e `downloadUrl`.
pub const VIDEO_FORMAT: &str = "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b";

/// Opções de diretório/ambiente dos comandos.
#[derive(Debug, Clone, Copy)]
pub struct TrailerOptions<'a> {
    pub trailers_dir: &'a Path,
    pub ffmpeg_dir: &'a str,
    pub cookies_path: &'a str,
}

/// Resultado de `search` em `--dump-json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrailerResult {
    pub id: String,
    pub url: String,
    pub title: String,
    pub duration: f64,
    pub channel: String,
    pub thumbnail: String,
}

/// Evento de progresso do `downloadUrl`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub percent: f64,
    pub stage: String,
}

/// `safeName`: só `[a-z0-9._-]` sobrevive.
pub fn safe_name(id: &str) -> String {
    id.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// `localPath` com sonda de existência injetada.
pub fn local_path_with<F>(trailers_dir: &Path, id: &str, exists: F) -> Option<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    let base = trailers_dir.join(safe_name(id));
    for extension in TRAILER_EXTENSIONS {
        let mut candidate = base.as_os_str().to_owned();
        candidate.push(extension);
        let candidate = PathBuf::from(candidate);
        if exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// `localPath` no sistema de arquivos real.
pub fn local_path(trailers_dir: &Path, id: &str) -> Option<PathBuf> {
    local_path_with(trailers_dir, id, |path| path.exists())
}

/// Nomes de arquivo que `clearFiles` remove para um id.
pub fn files_to_clear<'a, I>(names: I, safe: &str) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    names
        .into_iter()
        .filter(|name| *name == safe || name.starts_with(&format!("{safe}.")))
        .map(ToString::to_string)
        .collect()
}

/// `clearFiles` no sistema de arquivos real. Erros são ignorados, como no JS.
pub fn clear_files(trailers_dir: &Path, id: &str) {
    let safe = safe_name(id);
    let Ok(entries) = std::fs::read_dir(trailers_dir) else {
        return;
    };
    let names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    for name in files_to_clear(names.iter().map(String::as_str), &safe) {
        let _ = std::fs::remove_file(trailers_dir.join(name));
    }
}

/// `yt-dlp` existe no caminho configurado?
pub fn is_available(ytdlp_path: &Path) -> bool {
    !ytdlp_path.as_os_str().is_empty() && ytdlp_path.exists()
}

/// `cookieArgs` com sonda de existência injetada.
pub fn cookie_args_with<F>(cookies_path: &str, exists: F) -> Vec<String>
where
    F: Fn(&Path) -> bool,
{
    let cookies = cookies_path.trim();
    if cookies.is_empty() || !exists(Path::new(cookies)) {
        Vec::new()
    } else {
        vec!["--cookies".to_string(), cookies.to_string()]
    }
}

/// `cookieArgs` no sistema de arquivos real.
pub fn cookie_args(cookies_path: &str) -> Vec<String> {
    cookie_args_with(cookies_path, |path| path.exists())
}

fn ffmpeg_args(ffmpeg_dir: &str) -> Vec<String> {
    if ffmpeg_dir.is_empty() {
        Vec::new()
    } else {
        vec!["--ffmpeg-location".to_string(), ffmpeg_dir.to_string()]
    }
}

fn output_template(options: &TrailerOptions<'_>, id: &str) -> String {
    options
        .trailers_dir
        .join(format!("{}.%(ext)s", safe_name(id)))
        .to_string_lossy()
        .into_owned()
}

/// Texto de busca do cliente: `${title} trailer` (sem trim).
pub fn search_query(title: &str) -> String {
    format!("{title} trailer")
}

/// Especificação de busca do yt-dlp (`ytsearchN:`).
pub fn yt_search_spec(count: u32, query: &str) -> String {
    format!("ytsearch{count}:{query}")
}

/// URL de busca do YouTube para abrir no navegador (não usada pelo yt-dlp).
pub fn youtube_search_url(query: &str) -> String {
    format!(
        "https://www.youtube.com/results?search_query={}",
        crate::http::encode_uri_component(query)
    )
}

/// Argumentos de `download(id, title)`: `ytsearch5`, filtro de duração e
/// remux para mp4.
pub fn download_args_with_cookies(
    options: &TrailerOptions<'_>,
    id: &str,
    title: &str,
    cookies: &[String],
) -> Vec<String> {
    let mut args = vec![
        yt_search_spec(5, &search_query(title)),
        "--no-playlist".to_string(),
        "--no-warnings".to_string(),
        "--no-continue".to_string(),
        "--no-part".to_string(),
        "--match-filter".to_string(),
        "duration > 20 & duration < 360".to_string(),
        "-f".to_string(),
        VIDEO_FORMAT.to_string(),
        "--remux-video".to_string(),
        "mp4".to_string(),
    ];
    args.extend(ffmpeg_args(options.ffmpeg_dir));
    args.extend(cookies.iter().cloned());
    args.push("-o".to_string());
    args.push(output_template(options, id));
    args
}

/// `download`: variante que consulta `cookies_path` no disco.
pub fn download_args(options: &TrailerOptions<'_>, id: &str, title: &str) -> Vec<String> {
    download_args_with_cookies(options, id, title, &cookie_args(options.cookies_path))
}

/// Argumentos de `downloadUrl(id, url)`.
pub fn download_url_args_with_cookies(
    options: &TrailerOptions<'_>,
    id: &str,
    url: &str,
    cookies: &[String],
) -> Vec<String> {
    let mut args = vec![
        url.to_string(),
        "--no-playlist".to_string(),
        "--no-warnings".to_string(),
        "--no-continue".to_string(),
        "--no-part".to_string(),
        "--newline".to_string(),
        "-f".to_string(),
        VIDEO_FORMAT.to_string(),
        "--remux-video".to_string(),
        "mp4".to_string(),
    ];
    args.extend(ffmpeg_args(options.ffmpeg_dir));
    args.extend(cookies.iter().cloned());
    args.push("-o".to_string());
    args.push(output_template(options, id));
    args
}

/// `downloadUrl`: variante que consulta `cookies_path` no disco.
pub fn download_url_args(options: &TrailerOptions<'_>, id: &str, url: &str) -> Vec<String> {
    download_url_args_with_cookies(options, id, url, &cookie_args(options.cookies_path))
}

/// Argumentos de `streamUrl(url)`: primeira URL de stream em até 720p.
pub fn stream_url_args_with_cookies(url: &str, cookies: &[String]) -> Vec<String> {
    let mut args = vec![
        "-g".to_string(),
        "-f".to_string(),
        "best[height<=720][ext=mp4]/22/18/best[ext=mp4]/best".to_string(),
        "--no-warnings".to_string(),
    ];
    args.extend(cookies.iter().cloned());
    args.push(url.to_string());
    args
}

/// `streamUrl`: variante que consulta `cookies_path` no disco.
pub fn stream_url_args(url: &str, cookies_path: &str) -> Vec<String> {
    stream_url_args_with_cookies(url, &cookie_args(cookies_path))
}

/// Argumentos de `search(query)`.
pub fn search_args(query: &str) -> Vec<String> {
    vec![
        yt_search_spec(12, &search_query(query)),
        "--flat-playlist".to_string(),
        "--dump-json".to_string(),
        "--no-warnings".to_string(),
    ]
}

/// `search`: parseia as linhas JSON, ignorando avisos do yt-dlp.
pub fn parse_search_stdout(stdout: &str) -> Vec<TrailerResult> {
    let mut results = Vec::new();
    for line in stdout.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(data) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let id = crate::http::value_str(data.get("id"));
        let url = match data.get("url") {
            Some(url) if crate::http::value_truthy(Some(url)) => crate::http::value_str(Some(url)),
            _ => format!("https://www.youtube.com/watch?v={id}"),
        };
        let channel = match data.get("channel") {
            Some(channel) if crate::http::value_truthy(Some(channel)) => {
                crate::http::value_str(Some(channel))
            }
            _ => crate::http::value_str(data.get("uploader")),
        };
        let thumbnail = match data.get("thumbnail") {
            Some(thumbnail) if crate::http::value_truthy(Some(thumbnail)) => {
                crate::http::value_str(Some(thumbnail))
            }
            _ => data
                .get("thumbnails")
                .and_then(Value::as_array)
                .and_then(|thumbnails| thumbnails.last())
                .and_then(|thumbnail| thumbnail.get("url"))
                .map(|url| crate::http::value_str(Some(url)))
                .unwrap_or_default(),
        };
        results.push(TrailerResult {
            id,
            url,
            title: crate::http::value_str(data.get("title")),
            duration: value_number(data.get("duration")),
            channel,
            thumbnail,
        });
    }
    results
}

/// Mensagem de erro de `search` (primeira linha com `error`, ou código).
pub fn search_error_message(stderr: &str, code: &str, message: &str) -> String {
    static ERROR: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let regex = ERROR.get_or_init(|| Regex::new(r"(?i)error").expect("regex de erro"));
    if let Some(line) = stderr.split('\n').find(|line| regex.is_match(line)) {
        return line.to_string();
    }
    if code == "ENOENT" {
        "yt-dlp não encontrado".to_string()
    } else if !code.is_empty() {
        format!("yt-dlp falhou ({code})")
    } else {
        format!("yt-dlp falhou ({message})")
    }
}

/// Resultado de `streamUrl` a partir da saída do processo.
pub fn parse_stream_result(stdout: &str, stderr: &str) -> Result<String, String> {
    if let Some(link) = stdout.split('\n').find(|line| line.starts_with("http")) {
        return Ok(link.to_string());
    }
    static AGE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let age = AGE
        .get_or_init(|| Regex::new(r"(?i)confirm your age|inappropriate").expect("regex de idade"));
    if age.is_match(stderr) {
        Err("age".to_string())
    } else {
        Err("sem stream".to_string())
    }
}

/// Eventos emitidos por um chunk de `downloadUrl`.
pub fn progress_events(output: &str) -> Vec<ProgressEvent> {
    static PERCENT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let mut events = Vec::new();
    if let Some(capture) = PERCENT
        .get_or_init(|| Regex::new(r"\[download\]\s+([0-9.]+)%").expect("regex de progresso"))
        .captures(output)
    {
        if let Ok(percent) = capture[1].parse::<f64>() {
            events.push(ProgressEvent {
                percent,
                stage: "download".to_string(),
            });
        }
    }
    if output.contains("[VideoRemuxer]") || output.contains("Merging") {
        events.push(ProgressEvent {
            percent: 100.0,
            stage: "processando".to_string(),
        });
    }
    events
}

/// Mensagem final de `downloadUrl` quando nenhum arquivo apareceu.
pub fn download_error_message(stderr: &str) -> String {
    static AGE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static ERROR: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let age = AGE
        .get_or_init(|| Regex::new(r"(?i)confirm your age|inappropriate").expect("regex de idade"));
    if age.is_match(stderr) {
        return "age".to_string();
    }
    let line = stderr
        .split('\n')
        .rev()
        .find(|line| {
            ERROR
                .get_or_init(|| Regex::new(r"(?i)error|ffmpeg").expect("regex de erro"))
                .is_match(line)
        })
        .map(str::trim)
        .unwrap_or("");
    if line.is_empty() {
        "falha ao baixar".to_string()
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options<'a>(dir: &'a Path, ffmpeg: &'a str, cookies: &'a str) -> TrailerOptions<'a> {
        TrailerOptions {
            trailers_dir: dir,
            ffmpeg_dir: ffmpeg,
            cookies_path: cookies,
        }
    }

    #[test]
    fn safe_name_never_escapes_the_trailers_directory() {
        assert_eq!(safe_name("steam:10"), "steam_10");
        assert_eq!(safe_name("../../outside"), ".._.._outside");
        assert_eq!(safe_name("a b/c\\d"), "a_b_c_d");
        let path = local_path_with(Path::new("/trailers"), "../../outside", |_| true).unwrap();
        assert_eq!(path, Path::new("/trailers").join(".._.._outside.mp4"));
    }

    #[test]
    fn local_path_checks_extensions_in_order() {
        let existing = |path: &Path| path.extension().and_then(|ext| ext.to_str()) == Some("webm");
        let found = local_path_with(Path::new("/t"), "steam:10", existing).unwrap();
        assert_eq!(found, Path::new("/t").join("steam_10.webm"));

        let existing = |path: &Path| {
            matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("mp4") | Some("webm")
            )
        };
        assert_eq!(
            local_path_with(Path::new("/t"), "steam:10", existing).unwrap(),
            Path::new("/t").join("steam_10.mp4")
        );
        assert_eq!(
            local_path_with(Path::new("/t"), "steam:10", |_| false),
            None
        );
    }

    #[test]
    fn clear_files_selects_only_the_safe_prefix() {
        let names = [
            "steam_10.mp4",
            "steam_10.part",
            "steam_100.mp4",
            "other.mp4",
        ];
        assert_eq!(
            files_to_clear(names, "steam_10"),
            vec!["steam_10.mp4".to_string(), "steam_10.part".to_string()]
        );
    }

    #[test]
    fn cookie_args_only_when_the_file_exists_and_is_not_empty() {
        assert_eq!(cookie_args_with("", |_| true), Vec::<String>::new());
        assert_eq!(cookie_args_with("   ", |_| true), Vec::<String>::new());
        assert_eq!(cookie_args_with("c.txt", |_| false), Vec::<String>::new());
        assert_eq!(
            cookie_args_with(" c.txt ", |_| true),
            vec!["--cookies".to_string(), "c.txt".to_string()]
        );
    }

    #[test]
    fn search_args_match_the_client() {
        assert_eq!(
            search_args("Portal 2"),
            vec![
                "ytsearch12:Portal 2 trailer",
                "--flat-playlist",
                "--dump-json",
                "--no-warnings",
            ]
        );
    }

    #[test]
    fn youtube_helpers_build_the_same_query_and_an_openable_url() {
        assert_eq!(search_query("Portal 2"), "Portal 2 trailer");
        assert_eq!(
            yt_search_spec(5, "Portal 2 trailer"),
            "ytsearch5:Portal 2 trailer"
        );
        assert_eq!(
            youtube_search_url("Portal 2"),
            "https://www.youtube.com/results?search_query=Portal%202"
        );
    }

    #[test]
    fn download_args_keep_every_flag_in_order() {
        let dir = Path::new("/trailers");
        let options = options(dir, "/ffmpeg", "cookies.txt");
        let cookies = vec!["--cookies".to_string(), "cookies.txt".to_string()];
        let args = download_args_with_cookies(&options, "steam:10", "Portal 2", &cookies);
        assert_eq!(args[0], "ytsearch5:Portal 2 trailer");
        assert_eq!(args[1], "--no-playlist");
        assert_eq!(args[4], "--no-part");
        assert_eq!(args[5], "--match-filter");
        assert_eq!(args[6], "duration > 20 & duration < 360");
        assert_eq!(args[7], "-f");
        assert_eq!(args[8], VIDEO_FORMAT);
        assert_eq!(args[9], "--remux-video");
        assert_eq!(args[10], "mp4");
        assert_eq!(args[11], "--ffmpeg-location");
        assert_eq!(args[12], "/ffmpeg");
        assert_eq!(args[13], "--cookies");
        assert_eq!(args[14], "cookies.txt");
        assert_eq!(args[15], "-o");
        assert_eq!(
            args[16],
            dir.join("steam_10.%(ext)s").to_string_lossy().to_string()
        );
        assert_eq!(args.len(), 17);
    }

    #[test]
    fn download_args_without_ffmpeg_or_cookies_stays_minimal() {
        let dir = Path::new("/trailers");
        let options = options(dir, "", "");
        let args = download_args(&options, "steam:10", "Portal 2");
        assert!(!args.iter().any(|arg| arg == "--ffmpeg-location"));
        assert!(!args.iter().any(|arg| arg == "--cookies"));
        assert_eq!(
            args.last().unwrap(),
            &dir.join("steam_10.%(ext)s").to_string_lossy().to_string()
        );
    }

    #[test]
    fn download_url_args_match_the_client() {
        let dir = Path::new("/trailers");
        let options = options(dir, "", "c.txt");
        let args =
            download_url_args_with_cookies(&options, "steam:10", "https://youtu.be/abc", &[]);
        assert_eq!(args[0], "https://youtu.be/abc");
        assert_eq!(args[1], "--no-playlist");
        assert_eq!(args[5], "--newline");
        assert_eq!(args[6], "-f");
        assert_eq!(args[7], VIDEO_FORMAT);
        assert_eq!(args[8], "--remux-video");
        assert_eq!(args[9], "mp4");
        assert_eq!(args[10], "-o");
    }

    #[test]
    fn stream_args_match_the_client_and_append_cookies_before_the_url() {
        let args = stream_url_args_with_cookies(
            "https://youtu.be/abc",
            &["--cookies".to_string(), "c.txt".to_string()],
        );
        assert_eq!(
            args,
            vec![
                "-g",
                "-f",
                "best[height<=720][ext=mp4]/22/18/best[ext=mp4]/best",
                "--no-warnings",
                "--cookies",
                "c.txt",
                "https://youtu.be/abc",
            ]
        );
        assert_eq!(
            stream_url_args("https://youtu.be/abc", ""),
            vec![
                "-g",
                "-f",
                "best[height<=720][ext=mp4]/22/18/best[ext=mp4]/best",
                "--no-warnings",
                "https://youtu.be/abc",
            ]
        );
    }

    #[test]
    fn search_output_ignores_noise_and_fills_fallbacks() {
        let stdout = [
            "not json",
            r#"{"id":"abc","title":"Trailer","duration":42,"uploader":"Canal","thumbnails":[{"url":"thumb"}]}"#,
            r#"{"id":"def","url":"https://youtu.be/def","title":"T2","thumbnails":[{"url":"one"},{"url":"two"}]}"#,
            "",
        ]
        .join("\n");
        let results = parse_search_stdout(&stdout);
        assert_eq!(
            results,
            vec![
                TrailerResult {
                    id: "abc".into(),
                    url: "https://www.youtube.com/watch?v=abc".into(),
                    title: "Trailer".into(),
                    duration: 42.0,
                    channel: "Canal".into(),
                    thumbnail: "thumb".into(),
                },
                TrailerResult {
                    id: "def".into(),
                    url: "https://youtu.be/def".into(),
                    title: "T2".into(),
                    duration: 0.0,
                    channel: "".into(),
                    thumbnail: "two".into(),
                },
            ]
        );
    }

    #[test]
    fn search_error_prefers_stderr_then_the_code() {
        assert_eq!(
            search_error_message("WARNING: x\nERROR: timeout\n", "ETIMEDOUT", "network"),
            "ERROR: timeout"
        );
        assert_eq!(
            search_error_message("", "ENOENT", ""),
            "yt-dlp não encontrado"
        );
        assert_eq!(search_error_message("", "1", ""), "yt-dlp falhou (1)");
        assert_eq!(search_error_message("", "", "boom"), "yt-dlp falhou (boom)");
    }

    #[test]
    fn stream_result_finds_the_first_http_line_and_age_errors() {
        assert_eq!(
            parse_stream_result("https://cdn/a.mp4\nhttps://cdn/b.mp4", "").unwrap(),
            "https://cdn/a.mp4"
        );
        assert_eq!(
            parse_stream_result("", "ERROR: Please confirm your age").unwrap_err(),
            "age"
        );
        assert_eq!(
            parse_stream_result("", "ERROR: unavailable").unwrap_err(),
            "sem stream"
        );
    }

    #[test]
    fn progress_events_cover_download_and_remux() {
        assert_eq!(
            progress_events("[download]  42.5% of 10MiB"),
            vec![ProgressEvent {
                percent: 42.5,
                stage: "download".into()
            }]
        );
        assert_eq!(
            progress_events("[VideoRemuxer] Merging"),
            vec![ProgressEvent {
                percent: 100.0,
                stage: "processando".into()
            }]
        );
        assert!(progress_events("[download] 1.0%").len() == 1);
        assert!(progress_events("nada").is_empty());
    }

    #[test]
    fn download_error_prefers_age_and_the_last_relevant_line() {
        assert_eq!(
            download_error_message("ERROR: This video is inappropriate"),
            "age"
        );
        assert_eq!(
            download_error_message("linha\nERROR: primeiro\nERROR: último\n"),
            "ERROR: último"
        );
        assert_eq!(download_error_message("nada útil"), "falha ao baixar");
    }
}
