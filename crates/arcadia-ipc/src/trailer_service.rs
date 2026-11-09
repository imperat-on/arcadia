//! Runtime de trailers: resolve `yt-dlp`, monta argv com os helpers do crate
//! de mídia e executa o processo. A UI recebe caminhos `file://`.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::{json, Value};

use arcadia_media::trailer::{
    download_args, download_url_args, local_path, parse_search_stdout, parse_stream_result,
    search_args, stream_url_args, trailers_dir, TrailerOptions,
};

use crate::AppState;

fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let direct = directory.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        #[cfg(windows)]
        {
            if !name.to_ascii_lowercase().ends_with(".exe") {
                let exe = directory.join(format!("{name}.exe"));
                if exe.is_file() {
                    return Some(exe);
                }
            }
        }
    }
    None
}

fn ytdlp() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("YTDLP_PATH") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    find_executable("yt-dlp")
}

fn options(state: &AppState) -> (PathBuf, String) {
    let directory = trailers_dir(&state.data_dir);
    let cookies = arcadia_store::config::read_config(&state.data_dir)
        .get("youtube_cookies")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && Path::new(value).is_file())
        .map(str::to_string)
        .unwrap_or_else(|| {
            let fallback = state.data_dir.join("youtube_cookies.txt");
            if fallback.is_file() {
                fallback.to_string_lossy().into_owned()
            } else {
                String::new()
            }
        });
    (directory, cookies)
}

async fn run(program: &Path, args: &[String]) -> Result<std::process::Output, String> {
    tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|error| error.to_string())
}

pub fn local_path_json(state: &AppState, id: &str) -> Value {
    let (directory, _) = options(state);
    match local_path(&directory, id) {
        Some(path) => json!({ "path": format!("file://{}", path.to_string_lossy()) }),
        None => json!({ "path": "" }),
    }
}

pub async fn search(_state: &AppState, args: &[Value]) -> Value {
    let Some(program) = ytdlp() else {
        return json!({ "ok": false, "error": "yt-dlp não está instalado — instale o pacote yt-dlp" });
    };
    let query = args
        .first()
        .and_then(|value| value.get("query"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output = match run(&program, &search_args(query)).await {
        Ok(output) => output,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && stdout.trim().is_empty() {
        return json!({ "ok": false, "error": stderr.trim() });
    }
    let results = parse_search_stdout(&stdout);
    json!({ "ok": true, "results": results })
}

pub async fn stream_url(state: &AppState, args: &[Value]) -> Value {
    let Some(program) = ytdlp() else {
        return json!({ "ok": false, "error": "pedido inválido" });
    };
    let url = args
        .first()
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if url.is_empty() {
        return json!({ "ok": false, "error": "pedido inválido" });
    }
    let (_, cookies) = options(state);
    let output = match run(&program, &stream_url_args(url, &cookies)).await {
        Ok(output) => output,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    match parse_stream_result(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
    ) {
        Ok(url) => json!({ "ok": true, "url": url }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

pub async fn download_url(state: &AppState, args: &[Value]) -> Value {
    let Some(program) = ytdlp() else {
        return json!({ "ok": false, "error": "pedido inválido" });
    };
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
    let url = payload.get("url").and_then(Value::as_str).unwrap_or_default();
    if id.is_empty() || url.is_empty() {
        return json!({ "ok": false, "error": "pedido inválido" });
    }
    let (directory, cookies) = options(state);
    let options = TrailerOptions {
        trailers_dir: &directory,
        ffmpeg_dir: "",
        cookies_path: &cookies,
    };
    let output = match run(&program, &download_url_args(&options, id, url)).await {
        Ok(output) => output,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    if !output.status.success() {
        return json!({
            "ok": false,
            "error": String::from_utf8_lossy(&output.stderr).trim()
        });
    }
    match local_path(&directory, id) {
        Some(path) => json!({ "ok": true, "path": format!("file://{}", path.to_string_lossy()) }),
        None => json!({ "ok": false, "error": "arquivo não encontrado após o download" }),
    }
}

pub async fn download(state: &AppState, args: &[Value]) -> Value {
    let Some(program) = ytdlp() else {
        return json!({ "ok": false, "error": "yt-dlp ausente" });
    };
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
    let title = payload.get("title").and_then(Value::as_str).unwrap_or_default();
    if id.is_empty() {
        return json!({ "ok": false, "error": "yt-dlp ausente" });
    }
    let (directory, cookies) = options(state);
    let options = TrailerOptions {
        trailers_dir: &directory,
        ffmpeg_dir: "",
        cookies_path: &cookies,
    };
    let output = match run(&program, &download_args(&options, id, title)).await {
        Ok(output) => output,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    if !output.status.success() {
        return json!({
            "ok": false,
            "error": String::from_utf8_lossy(&output.stderr).trim()
        });
    }
    match local_path(&directory, id) {
        Some(path) => json!({ "ok": true, "path": format!("file://{}", path.to_string_lossy()) }),
        None => json!({ "ok": false, "error": "arquivo não encontrado após o download" }),
    }
}

pub async fn download_all(state: &AppState) -> Value {
    let Some(program) = ytdlp() else {
        return json!({ "ok": false, "error": "yt-dlp ausente" });
    };
    let (directory, cookies) = options(state);
    let options = TrailerOptions {
        trailers_dir: &directory,
        ffmpeg_dir: "",
        cookies_path: &cookies,
    };
    let games = arcadia_store::library::read_games(&state.data_dir, state.account().as_deref());
    let mut downloaded = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for game in games {
        let id = game.get("id").and_then(Value::as_str).unwrap_or_default();
        if id.is_empty() || local_path(&directory, id).is_some() {
            continue;
        }
        let title = game.get("title").and_then(Value::as_str).unwrap_or_default();
        match run(&program, &download_args(&options, id, title)).await {
            Ok(output) if output.status.success() => {
                if local_path(&directory, id).is_some() {
                    downloaded += 1;
                }
            }
            Ok(output) => errors.push(String::from_utf8_lossy(&output.stderr).trim().to_string()),
            Err(error) => errors.push(error),
        }
    }
    json!({ "ok": true, "count": downloaded, "errors": errors })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_trailer_returns_empty_path() {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-trailer-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let state = crate::AppState::new(dir.clone());
        assert_eq!(local_path_json(&state, "steam:1")["path"], "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
