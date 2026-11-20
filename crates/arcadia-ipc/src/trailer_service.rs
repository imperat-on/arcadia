//! Runtime de trailers: resolve `yt-dlp`, monta argv com os helpers do crate
//! de mídia e executa o processo. A UI recebe caminhos `file://`.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use arcadia_media::trailer::{
    download_args, download_url_args, local_path, parse_search_stdout, parse_stream_result,
    progress_events, search_args, stream_url_args, trailers_dir, TrailerOptions,
};

use crate::{AppState, Shell};

/// Conteúdo de um trailer local em base64, para a UI montar um blob URL.
///
/// O `<video>` do WebKitGTK não toca `file://` a partir da origem `tauri://`
/// (erro 4), e o assetProtocol só serve imagens. Blob é o único caminho para
/// arquivo local — é o mesmo do `boot:video`, que funciona.
///
/// `Vec<u8>` direto no `json!` não escala: o serde transforma cada byte num
/// `Value` de 32 bytes, e um trailer de 72 MB virava ~2,3 GB de RSS — sozinho
/// estourava o `MemoryMax` de 4 GiB do worker que roda o app. Base64 custa 4/3
/// do arquivo (96 MB no mesmo trailer) e a UI decodifica para `Uint8Array`.
fn trailer_em_base64(caminho: &Path) -> std::io::Result<String> {
    use base64::Engine;
    let bytes = std::fs::read(caminho)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn mime_do_trailer(caminho: &Path) -> &'static str {
    match caminho
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        _ => "video/mp4",
    }
}

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

/// Executa o processo entregando cada linha do stdout ao chamador.
///
/// `downloadUrl` precisa do progresso em tempo real: `.output()` só devolve
/// quando o yt-dlp termina, e a UI ficava com a barra em 0% até o fim. O
/// stderr é lido em paralelo para não encher o pipe e travar o processo.
async fn run_com_progresso<F>(
    program: &Path,
    args: &[String],
    mut ao_linha: F,
) -> Result<std::process::Output, String>
where
    F: FnMut(&str),
{
    let mut filho = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;

    let stderr = filho
        .stderr
        .take()
        .ok_or_else(|| "stderr indisponível".to_string())?;
    let leitura_erro = tokio::spawn(async move {
        let mut buffer = Vec::new();
        let mut leitor = BufReader::new(stderr);
        let _ = leitor.read_to_end(&mut buffer).await;
        buffer
    });

    let stdout = filho
        .stdout
        .take()
        .ok_or_else(|| "stdout indisponível".to_string())?;
    let mut linhas = BufReader::new(stdout).lines();
    while let Some(linha) = linhas
        .next_line()
        .await
        .map_err(|error| error.to_string())?
    {
        ao_linha(&linha);
    }

    let status = filho.wait().await.map_err(|error| error.to_string())?;
    let stderr = leitura_erro.await.unwrap_or_default();
    Ok(std::process::Output {
        status,
        stdout: Vec::new(),
        stderr,
    })
}

pub fn local_path_json(state: &AppState, id: &str) -> Value {
    let (directory, _) = options(state);
    match local_path(&directory, id) {
        Some(path) => json!({ "path": format!("file://{}", path.to_string_lossy()) }),
        None => json!({ "path": "" }),
    }
}

/// Conteúdo do trailer baixado, em base64, para a UI montar um blob URL.
///
/// O console e o fundo do Big Picture usavam `file://` direto; a origem
/// `tauri://` recusa esse esquema no `<video>` (erro 4). Como o assetProtocol
/// só serve imagens, o caminho que funciona é o mesmo do boot: bytes no IPC +
/// blob na UI.
pub fn local_data(state: &AppState, args: &[Value]) -> Value {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
    if id.is_empty() {
        return json!({ "ok": false, "error": "id ausente" });
    }
    let (directory, _) = options(state);
    let Some(path) = local_path(&directory, id) else {
        return json!({ "ok": false, "error": "ausente" });
    };
    match trailer_em_base64(&path) {
        Ok(data) => json!({
            "ok": true,
            "data": data,
            "mime": mime_do_trailer(&path),
            "path": format!("file://{}", path.to_string_lossy())
        }),
        Err(erro) => json!({ "ok": false, "error": erro.to_string() }),
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

pub async fn download_url(state: &AppState, shell: &impl Shell, args: &[Value]) -> Value {
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
    let output = match run_com_progresso(&program, &download_url_args(&options, id, url), |linha| {
        for evento in progress_events(linha) {
            shell.emit(
                "trailer:dlprogress",
                json!({ "id": id, "percent": evento.percent, "stage": evento.stage }),
            );
        }
    })
    .await
    {
        Ok(output) => output,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    if !output.status.success() {
        return json!({
            "ok": false,
            "error": String::from_utf8_lossy(&output.stderr).trim()
        });
    }
    shell.emit(
        "trailer:dlprogress",
        json!({ "id": id, "percent": 100.0, "stage": "done" }),
    );
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

    /// O trailer baixado precisa chegar à UI como base64 + MIME.
    ///
    /// O console e o fundo do Big Picture não conseguem tocar `file://` a
    /// partir de `tauri://`; o blob montado com estes bytes é o caminho que
    /// funciona. E `data` tem que ser string: `Vec<u8>` no `json!` virava um
    /// `Value` por byte e estourava a memória em trailers grandes.
    #[test]
    fn local_data_devolve_base64_e_mime() {
        use base64::Engine;
        let dir = std::env::temp_dir().join(format!(
            "arcadia-trailer-local-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        std::fs::create_dir_all(trailers_dir(&dir)).unwrap();
        let state = crate::AppState::new(dir.clone());
        let conteudo = vec![7u8; 32];
        std::fs::write(trailers_dir(&dir).join("steam_10.mp4"), &conteudo).unwrap();

        let r = local_data(&state, &[json!({ "id": "steam:10" })]);
        assert_eq!(r["ok"], true);
        assert_eq!(r["mime"], "video/mp4");
        let data = r["data"].as_str().expect("data precisa ser string base64");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap(),
            conteudo
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_data_ausente_nao_e_erro_generico() {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-trailer-ausente-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let state = crate::AppState::new(dir.clone());
        let r = local_data(&state, &[json!({ "id": "steam:404" })]);
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"], "ausente");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
