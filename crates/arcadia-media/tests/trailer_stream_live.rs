// language: Rust, file: tests/trailer_stream_live.rs
// Suite de integração: exercita o yt-dlp REAL contra o YouTube.
//
// Os testes unitários de `arcadia-media` travam a FORMA do seletor de formato,
// mas forma não prova que o YouTube aceita. Este teste chama o binário de
// verdade e exige uma URL de volta — é o único jeito de pegar a próxima vez que
// o YouTube mudar o que serve, que foi exatamente a causa do buffering
// infinito no player.
//
// Roda só quando `ARCADIA_LIVE_TESTS=1` está definido: a suíte normal precisa
// ser hermética e não depender de rede. O CI liga a flag num job próprio.
//
// Uso:
//   ARCADIA_LIVE_TESTS=1 cargo test -p arcadia-media --test trailer_stream_live

use std::process::Command;

use arcadia_media::trailer::{parse_stream_result, stream_url_args, STREAM_FORMAT};

/// Vídeo público e estável, usado só para exercitar o seletor.
const VIDEO: &str = "https://www.youtube.com/watch?v=aqz-KE-bpKQ";

fn yt_dlp() -> Option<String> {
    if let Ok(p) = std::env::var("ARCADIA_YTDLP") {
        return Some(p);
    }
    for candidato in ["yt-dlp", "youtube-dl"] {
        let status = Command::new(candidato).arg("--version").output();
        if status.map(|o| o.status.success()).unwrap_or(false) {
            return Some(candidato.to_string());
        }
    }
    None
}

fn ligado() -> bool {
    std::env::var("ARCADIA_LIVE_TESTS").ok().as_deref() == Some("1")
}

#[test]
fn stream_url_devolve_url_que_o_player_consegue_tocar() {
    if !ligado() {
        eprintln!("pulado: defina ARCADIA_LIVE_TESTS=1 para rodar contra a rede");
        return;
    }
    let Some(program) = yt_dlp() else {
        panic!("yt-dlp não encontrado no PATH — o app depende dele para trailers");
    };

    let args = stream_url_args(VIDEO, "");
    let saida = Command::new(&program)
        .args(&args)
        .output()
        .expect("falha ao executar yt-dlp");

    let stdout = String::from_utf8_lossy(&saida.stdout);
    let stderr = String::from_utf8_lossy(&saida.stderr);

    // A mensagem exata que o seletor antigo produzia, e que virava buffering
    // infinito porque a UI não tinha erro para mostrar.
    assert!(
        !stderr.contains("Requested format is not available"),
        "o seletor {STREAM_FORMAT} não casa com nenhum formato que o YouTube \
         serve. Era essa a causa do buffering infinito.\nstderr: {stderr}"
    );

    let url = parse_stream_result(&stdout, &stderr).unwrap_or_else(|erro| {
        panic!(
            "stream_url não devolveu URL (erro {erro:?}).\nargs: {args:?}\n\
             stdout: {stdout}\nstderr: {stderr}"
        )
    });

    assert!(
        url.starts_with("https://"),
        "URL não é https — o WebKit recusa: {url}"
    );
    assert!(
        !url.contains('\n'),
        "URL veio com quebra de linha e quebra o <video>: {url:?}"
    );
}

/// O caminho que a UI usa hoje: baixar e tocar do disco. Se o download
/// funcionar e o arquivo existir, o player tem o que tocar.
#[test]
fn download_do_trailer_produz_arquivo_reproduzivel() {
    if !ligado() {
        eprintln!("pulado: defina ARCADIA_LIVE_TESTS=1 para rodar contra a rede");
        return;
    }
    let Some(program) = yt_dlp() else {
        panic!("yt-dlp não encontrado no PATH");
    };

    let dir = std::env::temp_dir().join("arcadia-trailer-live");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("criar diretório temporário");

    let alvo = dir.join("trailer.%(ext)s");
    let saida = Command::new(&program)
        .args([
            VIDEO,
            "--no-playlist",
            "--no-warnings",
            "--no-continue",
            "--no-part",
            "-f",
            arcadia_media::trailer::VIDEO_FORMAT,
            "--remux-video",
            "mp4",
            "-o",
            &alvo.to_string_lossy(),
        ])
        .output()
        .expect("falha ao executar yt-dlp");

    assert!(
        saida.status.success(),
        "download falhou — o player ficaria sem arquivo.\nstderr: {}",
        String::from_utf8_lossy(&saida.stderr)
    );

    let baixado = std::fs::read_dir(&dir)
        .expect("ler diretório")
        .filter_map(Result::ok)
        .find(|e| e.path().extension().map(|x| x == "mp4").unwrap_or(false))
        .map(|e| e.path())
        .expect("nenhum .mp4 produzido — o <video> não teria o que tocar");

    let tamanho = std::fs::metadata(&baixado)
        .expect("metadata do arquivo")
        .len();
    assert!(
        tamanho > 100_000,
        "arquivo de {} bytes é pequeno demais para ser um trailer \
         (provável falha silenciosa): {:?}",
        tamanho,
        baixado
    );

    // Cabeçalho de MP4: uma caixa `ftyp` nos primeiros bytes. Garante que o
    // arquivo é contêiner de vídeo e não uma página de erro salva como .mp4.
    let bytes = std::fs::read(&baixado).expect("ler arquivo");
    assert!(
        bytes.len() > 12 && &bytes[4..8] == b"ftyp",
        "arquivo não tem cabeçalho ftyp de MP4 — o WebKit não vai tocar"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
