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

/// Block de bot do YouTube: IPs de datacenter (runners de CI, VPS) recebem
/// "Sign in to confirm you're not a bot" em vez de vídeo. Isso NÃO é regressão
/// do nosso seletor — é o YouTube recusando o IP. Distinguir os dois é o que
/// evita o CI falhar para sempre por um motivo que não está no código, e ao
/// mesmo tempo evita marcar como "pulado" uma falha real de formato.
fn bloqueio_de_bot(stderr: &str) -> bool {
    stderr.contains("Sign in to confirm")
        || stderr.contains("confirm you’re not a bot")
        || stderr.contains("confirm you're not a bot")
        || stderr.contains("cookies-from-browser")
}

/// Falha que É do nosso código: o YouTube respondeu, mas o seletor não casou
/// com nenhum formato. Esse é o erro que precisa quebrar o build.
fn seletor_invalido(stderr: &str) -> bool {
    stderr.contains("Requested format is not available")
}

#[test]
fn classificador_separa_block_de_bot_de_seletor_invalido() {
    // Estes dois erros chegam os dois como "sem stream" no parse, mas a causa é
    // oposta: um é o ambiente (IP de datacenter bloqueado), o outro é bug nosso.
    // Confundir os dois é o que faz o CI falhar para sempre OU passar em
    // silêncio com o seletor quebrado.
    let bot = "ERROR: [youtube] x: Sign in to confirm you’re not a bot. \
               Use --cookies-from-browser or --cookies for the authentication.";
    assert!(bloqueio_de_bot(bot), "block de bot não reconhecido");
    assert!(!seletor_invalido(bot), "block de bot marcado como bug do seletor");

    let formato = "ERROR: [youtube] x: Requested format is not available. \
                   Use --list-formats for a list of available formats";
    assert!(seletor_invalido(formato), "erro de formato não reconhecido");
    assert!(
        !bloqueio_de_bot(formato),
        "erro de formato marcado como block de IP — mascararia a regressão"
    );
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

    // Bloqueio de bot: o IP do runner é recusado. Não é regressão do seletor —
    // é o ambiente. Sai sem falhar, mas grita no log para não passar batido.
    if bloqueio_de_bot(&stderr) {
        eprintln!(
            "PULADO: o YouTube recusou este IP com desafio de bot.\n\
             Isso acontece em runners de CI e VPS. O seletor não foi exercitado.\n\
             stderr: {stderr}"
        );
        return;
    }

    // A mensagem exata que o seletor antigo produzia, e que virava buffering
    // infinito porque a UI não tinha erro para mostrar. Essa é SEMPRE falha.
    assert!(
        !seletor_invalido(&stderr),
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

    let stderr_dl = String::from_utf8_lossy(&saida.stderr);
    if bloqueio_de_bot(&stderr_dl) {
        eprintln!(
            "PULADO: o YouTube recusou este IP com desafio de bot (runner de CI).\n\
             O download real não foi exercitado. stderr: {stderr_dl}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    assert!(
        saida.status.success(),
        "download falhou — o player ficaria sem arquivo.\nstderr: {stderr_dl}"
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
