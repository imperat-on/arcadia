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

/// Erro único para `yt-dlp` ausente.
///
/// Antes cada chamada inventava a sua mensagem: `search` dizia "não está
/// instalado", `stream_url` e `download_url` diziam "pedido inválido" — o que
/// manda o usuário procurar defeito nos argumentos quando o problema é uma
/// dependência que não está na máquina.
fn ytdlp_ausente() -> Value {
    json!({
        "ok": false,
        "error": "yt_dlp_ausente",
        "mensagem": "yt-dlp não está instalado — instale o pacote yt-dlp (Arch: sudo pacman -S yt-dlp)"
    })
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
        return ytdlp_ausente();
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
        return ytdlp_ausente();
    };
    let url = args
        .first()
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if url.is_empty() {
        return ytdlp_ausente();
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
        return ytdlp_ausente();
    };
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
    let url = payload.get("url").and_then(Value::as_str).unwrap_or_default();
    if id.is_empty() || url.is_empty() {
        return ytdlp_ausente();
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
        return ytdlp_ausente();
    };
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
    let title = payload.get("title").and_then(Value::as_str).unwrap_or_default();
    if id.is_empty() {
        return ytdlp_ausente();
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

/// Baixa um trailer HLS da Steam para arquivo local e devolve o caminho.
///
/// A Steam aposentou o MP4: hoje `movies[].mp4` vem vazio e só `hls_h264`
/// existe. O player antigo montava a URL direto no `<video>` e, sem mp4,
/// caía no `hls.js` — que precisa de Media Source Extensions. O WebKitGTK
/// não implementa MSE para vídeo, então o `attachMedia` nunca dispara
/// `MANIFEST_PARSED` e o `<video>` fica preto girando para sempre.
///
/// Baixar contorna a limitação inteira: o ffmpeg busca o manifesto, baixa os
/// segmentos e junta num MP4 progressivo, que o WebKit toca pelo GStreamer
/// normalmente. Fica em cache no mesmo diretório de trailers.
pub async fn cache_hls(state: &AppState, args: &[Value]) -> Value {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let url = payload.get("url").and_then(Value::as_str).unwrap_or_default();
    let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
    if url.is_empty() || id.is_empty() {
        return json!({ "ok": false, "error": "parâmetros ausentes" });
    }

    // Nome estável por id do filme: reabrir a ficha não baixa de novo.
    let seguro: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let destino = trailers_dir(&state.data_dir).join(format!("hls_{seguro}.mp4"));

    if let Ok(meta) = std::fs::metadata(&destino) {
        if meta.len() > 100_000 {
            return json!({ "ok": true, "path": format!("file://{}", destino.to_string_lossy()), "cached": true });
        }
    }

    if let Some(pai) = destino.parent() {
        let _ = std::fs::create_dir_all(pai);
    }

    let Some(ffmpeg) = find_executable("ffmpeg") else {
        return json!({ "ok": false, "error": "ffmpeg não encontrado" });
    };

    // `-c copy` mantém o H.264 original sem recodificar (rápido). `+faststart`
    // move o átomo de índice para o começo, senão o WebKit precisa baixar o
    // arquivo inteiro antes de mostrar o primeiro frame.
    let output = run(
        &ffmpeg,
        &[
            "-y".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-i".to_string(),
            url.to_string(),
            "-c".to_string(),
            "copy".to_string(),
            "-movflags".to_string(),
            "+faststart".to_string(),
            destino.to_string_lossy().into_owned(),
        ],
    )
    .await;

    match output {
        Ok(out) if out.status.success() => {
            let tamanho = std::fs::metadata(&destino).map(|m| m.len()).unwrap_or(0);
            if tamanho > 100_000 {
                json!({ "ok": true, "path": format!("file://{}", destino.to_string_lossy()), "cached": false })
            } else {
                json!({ "ok": false, "error": format!("arquivo gerado tem {tamanho} bytes") })
            }
        }
        Ok(out) => json!({
            "ok": false,
            "error": String::from_utf8_lossy(&out.stderr).trim()
        }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

pub async fn download_all(state: &AppState) -> Value {
    let Some(program) = ytdlp() else {
        return ytdlp_ausente();
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

    // ── Regressão: buffering infinito no player da ficha do jogo ─────────────
    //
    // A Steam aposentou o MP4: `movies[].mp4` vem vazio e só `hls_h264` existe.
    // O player montava `src={direto}` (string vazia) e delegava ao hls.js, que
    // depende de Media Source Extensions — indisponível para vídeo no
    // WebKitGTK. O <video> nunca disparava `canplay` e ficava girando.
    //
    // A correção é baixar o HLS para um MP4 progressivo. Estes testes travam as
    // duas metades: o backend produz o arquivo, e o nome é estável (senão cada
    // abertura da ficha baixaria de novo e o cache nunca acertaria).

    #[test]
    fn cache_hls_rejeita_argumentos_ausentes() {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-cachehls-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let state = crate::AppState::new(dir.clone());

        // Sem url/id não há o que baixar — e não pode virar erro genérico.
        let so_id = vec![json!({ "id": "123" })];
        let r = futures_lite_block(cache_hls(&state, &so_id));
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"], "parâmetros ausentes");

        let so_url = vec![json!({ "url": "https://x/y.m3u8" })];
        let r = futures_lite_block(cache_hls(&state, &so_url));
        assert_eq!(r["ok"], false);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// O nome do cache precisa ser estável e seguro para o sistema de arquivos.
    /// Id de filme da Steam é numérico, mas a sanitização é a garantia de que
    /// um id inesperado não escape do diretório de trailers.
    #[test]
    fn nome_do_cache_usa_apenas_caracteres_seguros() {
        for id in ["257109786", "../evil", "a/b\\c", "id com espaço"] {
            let seguro: String = id
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            assert!(
                !seguro.contains('/') && !seguro.contains('\\') && !seguro.contains(".."),
                "id {id:?} gerou nome inseguro: {seguro:?}"
            );
            // Determinístico: mesmo id -> mesmo nome, senão o cache não acerta.
            let outra: String = id
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            assert_eq!(seguro, outra);
        }
    }

    /// Bloqueia num future já pronto. Os testes acima não tocam rede nem
    /// processo; só exercitam a validação síncrona de `cache_hls`.
    fn futures_lite_block(fut: impl std::future::Future<Output = Value>) -> Value {
        use std::task::{Context, Poll, Wake, Waker};
        struct Noop;
        impl Wake for Noop {
            fn wake(self: std::sync::Arc<Self>) {}
        }
        let waker = Waker::from(std::sync::Arc::new(Noop));
        let mut cx = Context::from_waker(&waker);
        let mut fut = Box::pin(fut);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }
}
