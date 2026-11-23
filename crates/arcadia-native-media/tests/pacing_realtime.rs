//! Garante que o player entrega frames em tempo real, não na velocidade do
//! decoder. Sem o pacing, o `appsink` com `sync(false)` despeja o arquivo o
//! mais rápido possível (medido: ~5x num trailer local).
//!
//! Fica `#[ignore]`: precisa de GStreamer + um arquivo real, que a CI não tem.

use arcadia_native_media::Player;
use std::time::{Duration, Instant};

fn sample_file() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("ARCADIA_MEDIA_SAMPLE").map(std::path::PathBuf::from)?;
    if dir.is_file() {
        Some(dir)
    } else {
        None
    }
}

#[test]
#[ignore = "precisa de GStreamer e um vídeo local em ARCADIA_MEDIA_SAMPLE"]
fn playback_advances_at_wall_clock_rate() {
    let Some(file) = sample_file() else {
        eprintln!("ARCADIA_MEDIA_SAMPLE não definido; pulando");
        return;
    };

    let mut player = Player::new().expect("init gstreamer");
    player
        .open(&format!("file://{}", file.display()))
        .expect("abrir mídia");

    let started = Instant::now();
    let mut last_seen = Duration::ZERO;

    // Drena até 6 s de parede, guardando a última posição informada.
    while started.elapsed() < Duration::from_secs(6) {
        while player.try_frame().is_some() {}
        if let Some(pos) = player.position() {
            last_seen = pos;
        }
        std::thread::sleep(Duration::from_millis(16));
    }

    let wall = started.elapsed().as_secs_f64();
    let media = last_seen.as_secs_f64();
    let ratio = media / wall;

    assert!(
        (0.8..=1.25).contains(&ratio),
        "mídia avançou {media:.2}s em {wall:.2}s de parede (ratio {ratio:.2}); \
         esperado ~1.0 — o pacing de tempo real não está ativo"
    );
}
