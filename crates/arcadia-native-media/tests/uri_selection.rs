//! Seleção de pipeline/URI sem rede: HLS, MP4 HTTP, arquivo local e esquema
//! não suportado. Nenhum teste aqui abre soquete.

use std::time::Duration;

use arcadia_native_media::{Error, MediaKind, Player, PlayerState};

#[test]
fn distingue_hls_de_mp4_e_de_arquivo_local() {
    assert_eq!(
        MediaKind::classify(
            "https://cdn.akamai.steamstatic.com/steam/apps/620/movie480.m3u8?token=x"
        ),
        MediaKind::Hls
    );
    assert_eq!(
        MediaKind::classify("https://cdn.exemplo/trailer.mp4"),
        MediaKind::Mp4
    );
    assert_eq!(
        MediaKind::classify("file:///home/jogador/trailers/620.mp4"),
        MediaKind::LocalFile
    );
    assert_eq!(
        MediaKind::classify("/home/jogador/trailers/620.mp4"),
        MediaKind::LocalFile
    );
    // Sem extensão reconhecível, o typefind do playbin decide pelo conteúdo.
    assert_eq!(
        MediaKind::classify("https://cdn.exemplo/stream?id=1"),
        MediaKind::Other
    );
}

#[test]
fn hls_exige_extensao_no_caminho_e_ignora_query() {
    assert_eq!(
        MediaKind::classify("https://cdn.exemplo/a.m3u8#fragmento"),
        MediaKind::Hls
    );
    assert_eq!(
        MediaKind::classify("https://cdn.exemplo/a.mp4?formato=m3u8"),
        MediaKind::Mp4
    );
}

#[test]
fn open_rejeita_esquema_desconhecido_antes_de_abrir_pipeline() {
    let mut player = Player::new().expect("GStreamer 1.x");
    let error = player
        .open("ftp://cdn.exemplo/trailer.mp4")
        .expect_err("ftp não é suportado");
    assert!(matches!(error, Error::InvalidUri(_)), "veio: {error:?}");
    assert_eq!(player.state(), PlayerState::Idle);
    assert!(player.uri().is_none());
}

#[test]
fn controles_sem_midia_falham_com_no_media() {
    let mut player = Player::new().expect("GStreamer 1.x");
    assert!(matches!(player.play(), Err(Error::NoMedia)));
    assert!(matches!(player.pause(), Err(Error::NoMedia)));
    assert!(matches!(
        player.seek(Duration::from_secs(1)),
        Err(Error::NoMedia)
    ));
    assert_eq!(player.state(), PlayerState::Idle);
    assert!(player.stop().is_ok());
}
