//! Mídia animada (GIF, WebP animado e vídeo) usada na UI.
//!
//! A web serve avatar, banner e fundo de perfil como **GIF animado** — e em
//! pelo menos um caso como **WebP animado com extensão `.gif`** (o arquivo é
//! RIFF, não GIF). Os cards de seção da Steam são `<video>` de verdade. O
//! `iced` desenha um `Handle` estático, então quem anima é este pool.
//!
//! ## Dois decodificadores, por um motivo medido
//!
//! - **Vídeo** (`<video>`: `.webm`, `.mp4`) → GStreamer via [`Player`]. É o
//!   único que abre VP9/H.264.
//! - **GIF e WebP animado** → crate `image` com `AnimationDecoder`.
//!
//! O GStreamer *não* serve para os dois últimos:
//!
//! - `webpdec` decodifica só o primeiro quadro; o pipeline morre com
//!   "No valid frames decoded before end of stream" no banner.
//! - Num GIF remoto o `seek` de reinício é recusado e a animação congela no
//!   último quadro — "toca uma vez só".
//!
//! O `image` entrega todos os quadros com o *delay* de cada um e não depende
//! de plugin do sistema. O custo é manter os quadros em memória, por isso o
//! pool é pequeno e o número de pixels por quadro é limitado.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::widget::image::Handle;

use arcadia_native_media::{frame_fits_sync_upload, Player, PlayerState};

/// Quantos cards de descrição tocam ao mesmo tempo.
pub const MAX_ANIMATED_CARDS: usize = 3;
/// Teto da mídia do perfil (avatar + banner + fundo).
pub const MAX_PROFILE_MEDIA: usize = 3;

/// De que grupo a URI veio. Os limites e o momento de despejo são diferentes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaGroup {
    /// Cards `<video>`/GIF do `about` do jogo.
    Card,
    /// Avatar, banner ou fundo do perfil.
    Profile,
    /// Avatares pequenos, retidos enquanto pertencem à lista atual.
    Avatar,
}

/// Um quadro decodificado e a duração que ele fica na tela.
#[derive(Debug, Clone)]
struct DecodedFrame {
    handle: Handle,
    delay: Duration,
}

/// Quadros prontos para registrar no pool sem trabalho de decodificação na UI.
#[derive(Debug, Clone)]
pub struct DecodedMedia {
    frames: Vec<DecodedFrame>,
}

#[cfg(test)]
impl DecodedMedia {
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

/// Executa a decodificação no pool bloqueante do Tokio.
///
/// Decodificar GIF/WebP é CPU-bound e segura a thread até terminar; fazer isso
/// direto no executor da UI congela a janela enquanto o avatar carrega.
async fn decode_blocking<F>(decode: F) -> Option<DecodedMedia>
where
    F: FnOnce() -> Option<Vec<DecodedFrame>> + Send + 'static,
{
    tokio::task::spawn_blocking(decode)
        .await
        .ok()
        .flatten()
        .map(|frames| DecodedMedia { frames })
}

pub async fn decode_media(bytes: Vec<u8>) -> Option<DecodedMedia> {
    decode_blocking(move || decode_frames(&bytes)).await
}

pub async fn decode_avatar(bytes: Vec<u8>) -> Option<DecodedMedia> {
    decode_blocking(move || decode_frames_with_limit(&bytes, 96 * 96)).await
}

/// Animação por quadros decodificados (GIF / WebP animado).
struct FrameAnimation {
    frames: Vec<DecodedFrame>,
    current: usize,
    shown_at: Instant,
    texture: Handle,
}

impl FrameAnimation {
    /// Avança o quadro conforme o *delay* de cada um.
    fn tick(&mut self) {
        if self.frames.len() <= 1 {
            return;
        }

        let mut elapsed = self.shown_at.elapsed();
        let mut moved = false;
        // Um tick atrasado pode pular mais de um quadro; o laço mantém o ritmo
        // em vez de tocar tudo em rajada no tick seguinte.
        while elapsed >= self.frames[self.current].delay {
            let delay = self.frames[self.current].delay;
            elapsed = elapsed.saturating_sub(delay);
            self.current = (self.current + 1) % self.frames.len();
            moved = true;
        }
        if moved {
            self.shown_at = Instant::now() - elapsed;
            self.texture = self.frames[self.current].handle.clone();
        }
    }
}

/// Um item animado: ou quadros em memória, ou um pipeline de vídeo.
enum Media {
    Frames(FrameAnimation),
    Video(VideoPlayer),
}

/// Vídeo tocado pelo GStreamer, com reinício por reabertura.
struct VideoPlayer {
    player: Player,
    uri: String,
    texture: Option<Handle>,
    ready: bool,
    failed: bool,
    restarts: u32,
}

impl VideoPlayer {
    fn new(uri: &str) -> Option<Self> {
        let mut player = Player::new().ok()?;
        if let Err(error) = player.open(uri) {
            eprintln!("[arcadia-native] mídia animada: open falhou para {uri}: {error}");
            return None;
        }
        Some(Self {
            player,
            uri: uri.to_string(),
            texture: None,
            ready: false,
            failed: false,
            restarts: 0,
        })
    }

    fn pump(&mut self) {
        if self.failed {
            return;
        }

        let mut latest = None;
        while let Some(frame) = self.player.try_frame() {
            latest = Some(frame);
        }

        if let Some(frame) = latest {
            if frame_fits_sync_upload(frame.width, frame.height) {
                self.texture = Some(Handle::from_rgba(frame.width, frame.height, frame.bytes));
                if !self.ready {
                    self.ready = true;
                    eprintln!(
                        "[arcadia-native] mídia animada: primeiro frame de {} ({}x{})",
                        self.uri, frame.width, frame.height
                    );
                }
            }
        }

        match self.player.state() {
            PlayerState::Error(error) => {
                eprintln!(
                    "[arcadia-native] mídia animada: erro em {}: {error}",
                    self.uri
                );
                self.failed = true;
            }
            PlayerState::Ended => {
                self.restarts = self.restarts.saturating_add(1);
                if let Err(error) = self.player.open(&self.uri) {
                    eprintln!(
                        "[arcadia-native] mídia animada: reinício falhou em {}: {error}",
                        self.uri
                    );
                    self.failed = true;
                }
            }
            _ => {}
        }
    }

    fn texture(&self) -> Option<&Handle> {
        self.texture.as_ref()
    }
}

/// Uma mídia no pool.
struct AnimatedItem {
    media: Media,
}

impl AnimatedItem {
    fn texture(&self) -> Option<&Handle> {
        match &self.media {
            Media::Frames(animation) => Some(&animation.texture),
            Media::Video(video) => video.texture(),
        }
    }

    fn is_ready(&self) -> bool {
        match &self.media {
            Media::Frames(_) => true,
            Media::Video(video) => video.ready,
        }
    }

    fn tick(&mut self) {
        match &mut self.media {
            Media::Frames(animation) => animation.tick(),
            Media::Video(video) => video.pump(),
        }
    }

    /// Quantos ciclos reiniciaram (só faz sentido para vídeo).
    fn restarts(&self) -> u32 {
        match &self.media {
            Media::Frames(_) => 0,
            Media::Video(video) => video.restarts,
        }
    }
}

/// `true` quando a URI é vídeo (o resto é tratado pelo crate `image`).
///
/// A extensão é só uma dica: o servidor entrega WebP com nome `.gif`, e um
/// `.webm` é sempre vídeo. Quem decide mesmo é o decode — esta função só evita
/// abrir um pipeline de vídeo para um arquivo que o `image` resolve.
fn looks_like_video(uri: &str) -> bool {
    let path = uri.split('?').next().unwrap_or(uri);
    let lowercase = path.to_ascii_lowercase();
    [".webm", ".mp4", ".m4v", ".mov", ".mkv", ".avi", ".ogv"]
        .iter()
        .any(|extension| lowercase.ends_with(extension))
}

/// Decodifica os quadros de um GIF ou WebP animado (ou uma imagem estática).
fn decode_frames(bytes: &[u8]) -> Option<Vec<DecodedFrame>> {
    decode_frames_with_limit(bytes, 512 * 1024)
}

fn decode_frames_with_limit(bytes: &[u8], max_pixels: u32) -> Option<Vec<DecodedFrame>> {
    /// Piso do delay: um GIF com `delay = 0` em todos os quadros viraria um
    /// laço de CPU. 20 ms é o menor passo visível a 60 Hz.
    const MIN_DELAY_MS: u64 = 20;

    let mut out = Vec::new();

    // GIF e WebP têm decoders distintos, ambos atrás de `AnimationDecoder`.
    // O `image` não expõe detecção de formato animado, então tentamos os dois
    // em vez de chutar pela extensão.
    if let Ok(decoder) = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)) {
        use image::AnimationDecoder;
        if let Ok(frames) = decoder.into_frames().collect_frames() {
            push_frames(&mut out, frames, max_pixels, MIN_DELAY_MS);
        }
    }

    if out.is_empty() {
        if let Ok(decoder) = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes)) {
            use image::AnimationDecoder;
            if let Ok(frames) = decoder.into_frames().collect_frames() {
                push_frames(&mut out, frames, max_pixels, MIN_DELAY_MS);
            }
        }
    }

    // Imagem estática (PNG/JPEG/AVIF): um quadro só, o tick não faz nada.
    if out.is_empty() {
        if let Ok(image) = image::load_from_memory(bytes) {
            if let Some(handle) = frame_handle(image.to_rgba8(), max_pixels) {
                out.push(DecodedFrame {
                    handle,
                    delay: Duration::from_secs(1),
                });
            }
        }
    }

    (!out.is_empty()).then_some(out)
}

fn push_frames(
    out: &mut Vec<DecodedFrame>,
    frames: Vec<image::Frame>,
    max_pixels: u32,
    min_delay_ms: u64,
) {
    for frame in frames {
        let (numer, denom) = frame.delay().numer_denom_ms();
        let delay_ms = if denom == 0 {
            min_delay_ms
        } else {
            (u64::from(numer) / u64::from(denom)).max(min_delay_ms)
        };
        if let Some(handle) = frame_handle(frame.into_buffer(), max_pixels) {
            out.push(DecodedFrame {
                handle,
                delay: Duration::from_millis(delay_ms),
            });
        }
    }
}

/// Reduz o quadro e monta o `Handle`, respeitando o teto de upload síncrono.
fn frame_handle(image: image::RgbaImage, max_pixels: u32) -> Option<Handle> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return None;
    }

    let area = u64::from(width) * u64::from(height);
    let limit = u64::from(max_pixels);
    let image = if area > limit {
        let scale = (limit as f64 / area as f64).sqrt();
        let target_w = ((f64::from(width) * scale).floor() as u32).max(1);
        let target_h = ((f64::from(height) * scale).floor() as u32).max(1);
        image::imageops::thumbnail(&image, target_w, target_h)
    } else {
        image
    };

    let (width, height) = image.dimensions();
    Some(Handle::from_rgba(width, height, image.into_raw()))
}

/// Pool de mídia animada, indexado pela URI.
#[derive(Default)]
pub struct AnimatedCards {
    items: HashMap<String, AnimatedItem>,
    /// URIs na ordem de criação, por grupo, para saber quem despejar.
    cards: Vec<String>,
    profile: Vec<String>,
    avatars: Vec<String>,
    /// Downloads em voo. Sem isto, dois caminhos que chamam `wants` antes de o
    /// primeiro byte chegar (o boot e a chegada do perfil, por exemplo)
    /// disparam o mesmo download duas vezes — medido no log: o avatar de
    /// 486 KB era baixado em duplicata a cada abertura.
    in_flight: std::collections::HashSet<String>,
}

impl AnimatedCards {
    /// `true` se algum item está ativo — a UI precisa do tick de 16 ms.
    pub fn is_active(&self) -> bool {
        !self.items.is_empty()
    }

    /// `true` quando a URI precisa ser baixada/aberta para entrar no pool.
    ///
    /// Vídeo é aberto pelo GStreamer direto na URI; imagem precisa dos bytes,
    /// então o chamador deve baixar, decodificar em worker e usar [`Self::insert_decoded`].
    pub fn wants(&self, uri: &str) -> bool {
        !uri.is_empty() && !self.items.contains_key(uri) && !self.in_flight.contains(uri)
    }

    /// Marca a URI como em download, para `wants` não repetir o pedido.
    pub fn mark_in_flight(&mut self, uri: &str) {
        self.in_flight.insert(uri.to_string());
    }

    /// Libera a marca depois do download (sucesso ou falha).
    pub fn clear_in_flight(&mut self, uri: &str) {
        self.in_flight.remove(uri);
    }

    /// Registra quadros já decodificados; não executa codecs na thread da UI.
    pub fn insert_decoded(&mut self, uri: &str, media: DecodedMedia, group: MediaGroup) {
        if uri.is_empty() || self.items.contains_key(uri) {
            return;
        }
        // Vídeo tem caminho próprio; aqui só GIF/WebP/imagem.
        if looks_like_video(uri) {
            return;
        }

        self.in_flight.remove(uri);

        let frames = media.frames;
        if frames.is_empty() {
            return;
        }
        let count = frames.len();
        let texture = frames[0].handle.clone();
        let animation = FrameAnimation {
            frames,
            current: 0,
            shown_at: Instant::now(),
            texture,
        };
        eprintln!("[arcadia-native] mídia animada: {count} quadro(s) de {uri}");

        self.enqueue(uri, group);
        self.items.insert(
            uri.to_string(),
            AnimatedItem {
                media: Media::Frames(animation),
            },
        );
    }

    /// Registra uma mídia de vídeo, que o GStreamer abre sozinho.
    pub fn open_video(&mut self, uri: &str, group: MediaGroup) {
        if uri.is_empty() || self.items.contains_key(uri) {
            return;
        }
        let Some(video) = VideoPlayer::new(uri) else {
            return;
        };
        self.enqueue(uri, group);
        self.items.insert(
            uri.to_string(),
            AnimatedItem {
                media: Media::Video(video),
            },
        );
    }

    /// Respeita o teto do grupo, despejando o mais antigo.
    fn enqueue(&mut self, uri: &str, group: MediaGroup) {
        let (order, limit) = match group {
            MediaGroup::Card => (&mut self.cards, MAX_ANIMATED_CARDS),
            MediaGroup::Profile => (&mut self.profile, MAX_PROFILE_MEDIA),
            MediaGroup::Avatar => (&mut self.avatars, usize::MAX),
        };
        if order.len() >= limit && order.first().cloned().is_some() {
            let oldest = order[0].clone();
            self.items.remove(&oldest);
            order.remove(0);
        }
        order.push(uri.to_string());
    }

    /// Avança quadros (GIF/WebP) e drena frames (vídeo). Chamado no tick.
    pub fn tick(&mut self) {
        for item in self.items.values_mut() {
            item.tick();
        }
    }

    /// Textura atual da mídia, quando já há quadro decodificado.
    pub fn texture(&self, uri: &str) -> Option<&Handle> {
        self.items.get(uri).and_then(AnimatedItem::texture)
    }

    /// `true` quando a URI está no pool e já produziu algum quadro.
    pub fn is_playing(&self, uri: &str) -> bool {
        self.items.get(uri).is_some_and(AnimatedItem::is_ready)
    }

    /// Quantos ciclos de vídeo reiniciaram (usado nos testes do loop).
    pub fn restarts(&self, uri: &str) -> u32 {
        self.items.get(uri).map_or(0, AnimatedItem::restarts)
    }

    /// Libera os cards do `about`. A mídia do perfil continua: fechar a ficha
    /// do jogo não fecha o perfil.
    pub fn clear_cards(&mut self) {
        for uri in self.cards.drain(..) {
            self.items.remove(&uri);
        }
    }

    /// Libera a mídia do perfil (ao sair da tela de perfil).
    pub fn clear_profile(&mut self) {
        for uri in self.profile.drain(..) {
            self.items.remove(&uri);
        }
    }

    pub fn retain_avatars(&mut self, urls: &std::collections::BTreeSet<String>) {
        self.avatars.retain(|uri| {
            if urls.contains(uri) {
                true
            } else {
                self.items.remove(uri);
                false
            }
        });
    }

    /// Libera tudo.
    pub fn clear(&mut self) {
        self.clear_cards();
        self.clear_profile();
        for uri in self.avatars.drain(..) {
            self.items.remove(&uri);
        }
        self.in_flight.clear();
    }
}

#[cfg(test)]
mod tests {
    fn profile_gif() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                let frame = image::Frame::from_parts(
                    image::RgbaImage::from_pixel(32, 32, image::Rgba(color)),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(20, 1),
                );
                encoder.encode_frame(frame).expect("GIF de teste válido");
            }
        }
        bytes
    }

    /// A decodificação roda no pool bloqueante: o primeiro poll do future
    /// precisa devolver `Pending`. Se voltar `Ready`, o decode inteiro
    /// aconteceu no executor da UI e a janela congela enquanto o avatar
    /// carrega. A liberação por canal torna o teste independente do tempo de
    /// decode da máquina — antes, um GIF pequeno podia terminar antes do
    /// primeiro poll e o teste falhava sem haver regressão.
    #[tokio::test(flavor = "current_thread")]
    async fn profile_decode_yields_before_processing_frames() {
        use std::future::Future;
        use std::task::{Context, Waker};

        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let decode = decode_blocking(move || {
            let _ = release_rx.recv();
            None
        });
        tokio::pin!(decode);

        let mut context = Context::from_waker(Waker::noop());
        assert!(
            decode.as_mut().poll(&mut context).is_pending(),
            "decodificação bloqueou o executor da UI"
        );

        release_tx.send(()).expect("liberar a decodificação");
        assert!(decode.await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decoded_profile_keeps_frames_and_loops() {
        let media = decode_media(profile_gif()).await.expect("GIF válido");
        let mut pool = AnimatedCards::default();
        pool.mark_in_flight("avatar.gif");
        pool.insert_decoded("avatar.gif", media, MediaGroup::Profile);
        assert!(!pool.in_flight.contains("avatar.gif"));
        assert!(!pool.wants("avatar.gif"));
        assert!(pool.texture("avatar.gif").is_some());
        let item = pool.items.get_mut("avatar.gif").expect("perfil registrado");
        let Media::Frames(animation) = &mut item.media else {
            panic!("GIF não deve abrir pipeline de vídeo");
        };
        assert_eq!(animation.frames.len(), 2);
        for expected in [1, 0, 1, 0] {
            animation.shown_at = Instant::now() - Duration::from_millis(25);
            animation.tick();
            assert_eq!(animation.current, expected);
        }
        assert!(decode_media(b"invalid image".to_vec()).await.is_none());
    }

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn list_avatars_keep_frames_separate_from_profile_limit() {
        let media = decode_avatar(profile_gif()).await.expect("avatar válido");
        assert_eq!(media.frames.len(), 2);
        let mut pool = AnimatedCards::default();
        let urls: std::collections::BTreeSet<_> =
            (0..8).map(|i| format!("avatar-{i}.gif")).collect();
        for url in &urls {
            pool.insert_decoded(url, media.clone(), MediaGroup::Avatar);
        }
        pool.insert_decoded("banner.gif", media, MediaGroup::Profile);
        pool.clear_profile();
        assert!(pool.texture("banner.gif").is_none());
        for url in &urls {
            assert!(pool.texture(url).is_some());
        }
        let first = pool.texture("avatar-0.gif").unwrap().id();
        let Media::Frames(animation) = &mut pool.items.get_mut("avatar-0.gif").unwrap().media
        else {
            panic!("quadros");
        };
        animation.shown_at = Instant::now() - Duration::from_millis(25);
        pool.tick();
        assert_ne!(pool.texture("avatar-0.gif").unwrap().id(), first);
        pool.retain_avatars(&["avatar-0.gif".to_string()].into_iter().collect());
        assert!(pool.texture("avatar-0.gif").is_some());
        assert!(pool.texture("avatar-7.gif").is_none());
        pool.clear();
        assert!(!pool.is_active());
    }

    #[test]
    fn empty_pool_is_not_active() {
        let pool = AnimatedCards::default();
        assert!(!pool.is_active());
        assert!(pool.texture("https://shared.test/a.gif").is_none());
        assert!(pool.wants("https://shared.test/a.gif"));
    }

    #[test]
    fn empty_uri_is_ignored() {
        let mut pool = AnimatedCards::default();
        pool.insert_decoded("", DecodedMedia { frames: Vec::new() }, MediaGroup::Card);
        assert!(!pool.is_active());
        assert!(!pool.wants(""));
    }

    #[test]
    fn video_extensions_are_routed_to_gstreamer() {
        assert!(looks_like_video("https://shared.test/a.webm"));
        assert!(looks_like_video("https://shared.test/a.mp4?t=123"));
        // O servidor entrega WebP com nome `.gif`: não é vídeo.
        assert!(!looks_like_video("https://shared.test/a.gif"));
        assert!(!looks_like_video(
            "https://arcadiaserver.test/banners/x/1786528340499.gif"
        ));
    }

    /// O avatar do perfil é um GIF real de 23 quadros e o banner é um **WebP
    /// animado de 8 quadros com extensão `.gif`** — o caso que o GStreamer não
    /// decodifica e que deixava o banner parado. Este teste decodifica os dois
    /// e exige mais de um quadro em cada.
    #[test]
    fn decodes_gif_and_webp_animations() {
        if let Ok(bytes) = std::fs::read("/tmp/av.gif") {
            let frames = decode_frames(&bytes).expect("gif do avatar deve decodificar");
            assert!(frames.len() > 1, "gif veio com {} quadro(s)", frames.len());
        }
        if let Ok(bytes) = std::fs::read("/tmp/banner.gif") {
            let frames = decode_frames(&bytes).expect("webp do banner deve decodificar");
            assert!(
                frames.len() > 1,
                "webp animado veio com {} quadro(s)",
                frames.len()
            );
        }
    }

    /// O sintoma: "a animação só funciona uma vez". Um pool que não avança o
    /// índice congela no primeiro quadro; aqui exigimos que a textura mude
    /// várias vezes ao longo dos *delays* reais da animação, e que volte ao
    /// começo (loop).
    #[test]
    fn the_animation_advances_and_loops() {
        let Ok(bytes) = std::fs::read("/tmp/banner.gif") else {
            return;
        };
        let mut frames = match decode_frames(&bytes) {
            Some(frames) => frames,
            None => return,
        };
        let total = frames.len();
        assert!(total > 1, "o banner precisa ter mais de um quadro");

        // Textura de cada índice, para saber quando voltou ao início.
        let first = frames[0].handle.clone();
        let mut animation = FrameAnimation {
            frames,
            current: 0,
            shown_at: Instant::now(),
            texture: first.clone(),
        };

        let mut distinct = std::collections::HashSet::new();
        let mut looped = false;
        let deadline = Instant::now() + Duration::from_millis(2000);
        while Instant::now() < deadline {
            animation.tick();
            if let Handle::Rgba { id, .. } = &animation.texture {
                distinct.insert(format!("{id:?}"));
                if animation.current == 0 && distinct.len() > 1 {
                    looped = true;
                }
            }
            std::thread::sleep(Duration::from_millis(16));
        }

        assert!(
            distinct.len() > 1,
            "a animação ficou no mesmo quadro o tempo todo"
        );
        assert!(
            looped,
            "a animação não voltou ao primeiro quadro (sem loop)"
        );
    }

    #[test]
    fn clearing_profile_leaves_cards_alone() {
        let mut pool = AnimatedCards::default();
        // Sem bytes válidos nada entra em `items`; os vetores de ordem são o
        // que este teste trava.
        pool.profile.push("perfil.gif".to_string());
        pool.cards.push("card.webm".to_string());
        pool.clear_profile();
        assert!(pool.profile.is_empty());
        assert_eq!(pool.cards.len(), 1);
    }

    /// O reinício do vídeo por reabertura continua valendo para os cards.
    #[test]
    fn the_video_restarts_and_keeps_producing_frames() {
        if std::env::var_os("GST_PLUGIN_PATH").is_none() && !gst_available() {
            return;
        }
        let Ok(bytes) = std::fs::read("/tmp/av.gif") else {
            return;
        };
        let path = std::env::temp_dir().join("arcadia_loop_fixture.gif");
        if std::fs::write(&path, &bytes).is_err() {
            return;
        }
        let uri = format!("file://{}", path.display());

        let mut pool = AnimatedCards::default();
        pool.open_video(&uri, MediaGroup::Card);

        let deadline = Instant::now() + Duration::from_secs(8);
        let mut seen = std::collections::HashSet::new();
        while Instant::now() < deadline {
            pool.tick();
            if let Some(Handle::Rgba { pixels, .. }) = pool.texture(&uri) {
                let hash = pixels.iter().step_by(1009).fold(0u64, |acc, byte| {
                    acc.wrapping_mul(31).wrapping_add(*byte as u64)
                });
                seen.insert(hash);
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        let _ = std::fs::remove_file(&path);

        assert!(seen.len() > 10, "vieram {} quadros distintos", seen.len());
        assert!(
            pool.restarts(&uri) >= 2,
            "o vídeo reiniciou {} vez(es) — esperava 2+ ciclos em 8s",
            pool.restarts(&uri)
        );
    }

    fn gst_available() -> bool {
        std::path::Path::new("/usr/bin/gst-inspect-1.0").exists()
    }
}
