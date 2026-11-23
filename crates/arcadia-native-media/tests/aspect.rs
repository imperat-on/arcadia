//! O frame entregue à UI precisa manter a proporção do vídeo.
//!
//! O bug: o caps fixava 640×360 e o `videoscale` esticava os cards de seção
//! da Steam (1170×438, 2,67:1) num molde 16:9 — o vídeo saía achatado.
//!
//! Ignorado por padrão (rede + plugins). Rode com:
//! `cargo test -p arcadia-native-media --test aspect -- --ignored`

use arcadia_native_media::Player;

/// Proporção do card de seção do Black Myth: Wukong: 1170×438.
const CARD_RATIO: f32 = 1170.0 / 438.0;

#[test]
#[ignore = "depende de rede e dos plugins GStreamer"]
fn keeps_the_source_aspect_ratio() {
    let uri = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/6d5d441bd83627bcbc6d2b8f8d15f0ff.webm?t=1760601605";

    let mut player = Player::new().expect("player");
    player.open(uri).expect("open");

    // Espera o primeiro frame.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut frame = None;
    while std::time::Instant::now() < deadline {
        if let Some(candidate) = player.try_frame() {
            frame = Some(candidate);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let frame = frame.expect("primeiro frame");

    let ratio = frame.width as f32 / frame.height as f32;
    assert!(
        (ratio - CARD_RATIO).abs() < 0.05,
        "frame {}x{} tem proporção {ratio:.2}, esperado {CARD_RATIO:.2} \
         (o vídeo foi esticado)",
        frame.width,
        frame.height
    );
    assert_eq!(
        frame.width, 640,
        "a largura de entrega é fixa em 640 px"
    );
}
