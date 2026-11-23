//! Extração do primeiro frame visível de um vídeo, para usar como imagem.
//!
//! A Steam serve, para os cards de seção do `about`, um `<video poster="….avif">`
//! onde o `poster` costuma ser um placeholder **preto**: o Black Myth: Wukong
//! manda 336 bytes de puros zeros com todos os pixels `(0,0,0)`. Desenhar esse
//! poster na ficha mostra um retângulo preto no lugar da arte.
//!
//! O vídeo em si tem a arte real (o `.webm` do mesmo card abre com luminância
//! média 19,5). Este módulo abre o stream em `PAUSED`, puxa o pré-roll do
//! `appsink` e devolve o primeiro frame que não seja preto.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::player::init;

/// Quantos frames examinar antes de desistir. O primeiro frame de um fade-in
/// pode ser preto; os seguintes normalmente já têm conteúdo.
const MAX_FRAMES: usize = 16;
/// Timeout de cada `try_pull_*` do appsink, em nanossegundos.
const PULL_TIMEOUT_NS: u64 = 500_000_000;
/// Luminância média mínima (0-255) para considerar que o frame tem arte.
/// O poster preto do Wukong mede 0.0; um frame real fica acima de 6.
const MIN_MEAN_LUMA: f32 = 6.0;

/// Primeiro frame não-preto do vídeo, em RGBA.
///
/// Devolve `(largura, altura, pixels)`. `None` quando o vídeo não abre, não
/// tem trilha de vídeo, ou todos os frames examinados são pretos — nesse caso
/// a UI mantém o poster original, que é o que resta.
pub fn extract_first_visible_frame(uri: &str) -> Option<(u32, u32, Vec<u8>)> {
    init().ok()?;

    let playbin = gst::ElementFactory::make("playbin").build().ok()?;
    let app_sink = make_video_sink()?;

    playbin.set_property("video-sink", &app_sink.element);
    playbin.set_property(
        "audio-sink",
        gst::ElementFactory::make("fakesink").build().ok()?,
    );
    playbin.set_property("uri", uri);

    let result = extract_from(&playbin, &app_sink.sink);
    let _ = playbin.set_state(gst::State::Null);
    result
}

struct VideoSink {
    element: gst::Element,
    sink: gst_app::AppSink,
}

/// `appsink` com caps RGBA, ligado depois de um `videoconvert`.
fn make_video_sink() -> Option<VideoSink> {
    let convert = gst::ElementFactory::make("videoconvert").build().ok()?;
    let scale = gst::ElementFactory::make("videoscale").build().ok()?;
    let sink_element = gst::ElementFactory::make("appsink").build().ok()?;
    let sink = sink_element
        .clone()
        .dynamic_cast::<gst_app::AppSink>()
        .ok()?;

    // RGBA direto: é o formato do `Handle::from_rgba` do iced.
    sink.set_caps(Some(
        &gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .build(),
    ));
    // O frame mais recente, não a fila inteira: queremos o começo do vídeo.
    sink.set_max_buffers(1);
    sink.set_sync(false);
    sink.set_drop(true);

    let bin = gst::Bin::new();
    bin.add_many([&convert, &scale, &sink_element]).ok()?;
    gst::Element::link_many([&convert, &scale, &sink_element]).ok()?;

    // `playbin` exige que o `video-sink` exponha um pad de entrada chamado
    // "sink". Um `Bin` não tem pads próprios: sem o ghost pad o playbin
    // recusa o sink com "custom video sink has no pad named sink" e o
    // pipeline fica em READY sem entregar frame.
    let sink_pad = convert.static_pad("sink")?;
    let ghost = gst::GhostPad::with_target(&sink_pad).ok()?;
    bin.add_pad(&ghost).ok()?;

    Some(VideoSink {
        element: bin.upcast(),
        sink,
    })
}

fn extract_from(playbin: &gst::Element, sink: &gst_app::AppSink) -> Option<(u32, u32, Vec<u8>)> {
    playbin.set_state(gst::State::Paused).ok()?;

    // Em PAUSED o pipeline entrega o pré-roll: o primeiro frame decodificado.
    // É exatamente a imagem que o card quer.
    if let Some(sample) = sink.try_pull_preroll(gst::ClockTime::from_nseconds(PULL_TIMEOUT_NS)) {
        if let Some(frame) = sample_to_rgba(&sample) {
            if mean_luma(&frame.2) >= MIN_MEAN_LUMA {
                return Some(frame);
            }
        }
    }

    // Pré-roll preto (fade-in): segue para PLAYING e olha os primeiros frames.
    let _ = playbin.set_state(gst::State::Playing);
    for _ in 0..MAX_FRAMES {
        let Some(sample) = sink.try_pull_sample(gst::ClockTime::from_nseconds(PULL_TIMEOUT_NS))
        else {
            break;
        };
        let Some(frame) = sample_to_rgba(&sample) else {
            continue;
        };
        if mean_luma(&frame.2) >= MIN_MEAN_LUMA {
            return Some(frame);
        }
    }

    None
}

/// Converte um `gst::Sample` em `(largura, altura, RGBA)`.
fn sample_to_rgba(sample: &gst::Sample) -> Option<(u32, u32, Vec<u8>)> {
    let caps = sample.caps()?;
    let info = gstreamer_video::VideoInfo::from_caps(caps).ok()?;
    let buffer = sample.buffer()?;
    let map = buffer.map_readable().ok()?;

    let width = info.width();
    let height = info.height();
    let stride = info.stride()[0] as usize;
    let row_bytes = width as usize * 4;
    let data = map.as_slice();

    // O buffer pode ter padding de linha; compacta para `width * 4` por linha.
    let mut rgba = Vec::with_capacity(row_bytes * height as usize);
    for row in 0..height as usize {
        let start = row * stride;
        let end = start + row_bytes;
        if end > data.len() {
            return None;
        }
        rgba.extend_from_slice(&data[start..end]);
    }

    Some((width, height, rgba))
}

/// Luminância média (0-255) de um buffer RGBA.
fn mean_luma(rgba: &[u8]) -> f32 {
    if rgba.is_empty() {
        return 0.0;
    }
    let mut sum: f64 = 0.0;
    let mut count: u64 = 0;
    for pixel in rgba.chunks_exact(4) {
        // Coeficientes ITU-R BT.601.
        let luma = 0.299 * pixel[0] as f64 + 0.587 * pixel[1] as f64 + 0.114 * pixel[2] as f64;
        sum += luma;
        count += 1;
    }
    if count == 0 {
        0.0
    } else {
        (sum / count as f64) as f32
    }
}

/// `true` quando a imagem é preta o suficiente para ser um placeholder.
///
/// A UI usa isso para decidir se vale a pena tentar o frame do vídeo: um
/// poster real nunca é totalmente preto, e o placeholder da Steam mede 0.
pub fn is_black_placeholder(width: u32, height: u32, rgba: &[u8]) -> bool {
    if width == 0 || height == 0 || rgba.is_empty() {
        return true;
    }
    mean_luma(rgba) < MIN_MEAN_LUMA
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_buffer_is_placeholder() {
        let rgba = vec![0u8; 16 * 16 * 4];
        assert!(is_black_placeholder(16, 16, &rgba));
    }

    #[test]
    fn real_image_is_not_placeholder() {
        // Cinza médio: o card real do Wukong tem média ~54.
        let rgba = vec![90u8; 16 * 16 * 4];
        assert!(!is_black_placeholder(16, 16, &rgba));
    }

    #[test]
    fn empty_buffer_counts_as_placeholder() {
        assert!(is_black_placeholder(0, 0, &[]));
        assert!(is_black_placeholder(8, 8, &[]));
    }

    #[test]
    fn mean_luma_follows_rec601_weights() {
        // Vermelho puro: 0.299 * 255 ≈ 76.2
        let rgba = vec![255, 0, 0, 255];
        assert!((mean_luma(&rgba) - 76.245).abs() < 0.5);

        // Verde puro: 0.587 * 255 ≈ 149.7
        let rgba = vec![0, 255, 0, 255];
        assert!((mean_luma(&rgba) - 149.685).abs() < 0.5);

        // Branco: 255
        let rgba = vec![255, 255, 255, 255];
        assert!((mean_luma(&rgba) - 255.0).abs() < 0.5);
    }

    #[test]
    fn a_fade_in_first_frame_is_still_a_placeholder() {
        // Preto quase puro: um fade-in de 1/255 não é arte.
        let rgba = vec![1u8, 1, 1, 255].repeat(64);
        assert!(is_black_placeholder(8, 8, &rgba));
    }
}
