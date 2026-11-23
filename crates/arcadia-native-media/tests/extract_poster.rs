//! Prova que a extração de poster funciona contra o vídeo real da Steam.
//!
//! Ignorado por padrão: depende de rede e do plugin VP9/H.264 instalados.
//! Rode com `cargo test -p arcadia-native-media --test extract_poster -- --ignored`.

use arcadia_native_media::{extract_first_visible_frame, is_black_placeholder};

#[test]
#[ignore = "depende de rede e dos plugins GStreamer"]
fn extracts_a_visible_frame_from_the_wukong_webm() {
    // Card de seção do `about` do Black Myth: Wukong. O `.poster.avif` do
    // mesmo hash é preto puro (336 bytes, luminância 0); o vídeo tem arte.
    let uri = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/6d5d441bd83627bcbc6d2b8f8d15f0ff.webm?t=1760601605";

    let (width, height, rgba) = extract_first_visible_frame(uri).expect("frame extraído");
    assert!(width > 0 && height > 0);
    assert_eq!(rgba.len(), width as usize * height as usize * 4);

    assert!(
        !is_black_placeholder(width, height, &rgba),
        "o frame extraído é preto: {width}x{height}"
    );
}
