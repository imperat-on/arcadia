//! Teste de integração local: `boot.mp4` na raiz do repositório precisa
//! produzir pelo menos um frame RGBA. Não usa rede.

use std::path::Path;
use std::time::{Duration, Instant};

use arcadia_native_media::{Player, PlayerState};

const TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn boot_mp4_produz_frame_rgba() {
    let boot = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("boot.mp4");
    let boot = std::fs::canonicalize(&boot)
        .unwrap_or_else(|error| panic!("boot.mp4 não encontrado em {}: {error}", boot.display()));

    let mut player = Player::new().expect("GStreamer 1.x com plugins básicos");
    player
        .open(boot.to_str().expect("caminho UTF-8"))
        .expect("abrir boot.mp4");

    let deadline = Instant::now() + TIMEOUT;
    let mut frame = None;
    while Instant::now() < deadline {
        match player.frames().recv_timeout(Duration::from_millis(200)) {
            Ok(received) => {
                frame = Some(received);
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let PlayerState::Error(message) = player.state() {
                    panic!("pipeline falhou antes do primeiro frame: {message}");
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                panic!("canal de frames fechou antes do primeiro frame");
            }
        }
    }

    let frame = frame.unwrap_or_else(|| {
        panic!(
            "nenhum frame RGBA em {TIMEOUT:?}; estado atual: {:?}",
            player.state()
        )
    });

    assert!(frame.width > 0, "largura zerada: {frame:?}");
    assert!(frame.height > 0, "altura zerada: {frame:?}");
    assert_eq!(frame.stride, frame.width as usize * 4);
    assert_eq!(
        frame.bytes.len(),
        frame.stride * frame.height as usize,
        "buffer RGBA com tamanho inconsistente"
    );
    assert!(
        frame
            .bytes
            .iter()
            .skip(3)
            .step_by(4)
            .any(|alpha| *alpha != 0),
        "todos os pixels vieram com alpha zero"
    );

    // A duração do boot.mp4 é ~9 s; o playbin a informa no primeiro preroll.
    let duration = player.duration().expect("duração do MP4");
    assert!(duration > Duration::ZERO, "duração zerada: {duration:?}");
}
