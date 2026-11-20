//! Player GStreamer independente de UI.
//!
//! O `playbin` decodifica em threads próprias do GStreamer; o callback do
//! `appsink` copia cada frame para RGBA e o entrega por um canal limitado. A
//! thread da UI nunca é bloqueada por I/O, decode ou espera de clock: ela só
//! chama [`Player::frames`] e drena o que estiver pronto.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;

use crate::error::{Error, Result};
use crate::frame::Frame;
use crate::uri::{self, MediaKind, ResolvedSource};

/// Quantos buffers o `appsink` mantém antes de descartar os antigos.
const APP_SINK_MAX_BUFFERS: u32 = 2;
/// Capacidade do canal de frames. Se a UI demorar, frames são descartados em
/// vez de crescer memória sem limite (um frame 4K RGBA tem ~33 MB).
const FRAME_CHANNEL_CAPACITY: usize = 2;
/// Intervalo de acordar da thread do bus para checar o encerramento.
const BUS_POLL_INTERVAL_MS: u64 = 100;

/// Estado observável do player.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PlayerState {
    /// Nenhuma mídia aberta.
    #[default]
    Idle,
    /// Pipeline montado; abrindo a mídia, buscando playlist ou pré-roll.
    Opening,
    /// Reproduzindo.
    Playing,
    /// Pausado explicitamente.
    Paused,
    /// Parado por [`Player::stop`]; a mídia continua aberta e pode ser
    /// reproduzida de novo com [`Player::play`].
    Stopped,
    /// Fim do stream.
    Ended,
    /// Erro reportado pelo bus do GStreamer. A string traz a mensagem e o
    /// debug detalhado; abrir outra mídia limpa o estado.
    Error(String),
}

/// Player de vídeo/áudio baseado em `playbin` + `appsink`.
///
/// - `open` aceita HTTP(S) HLS/MP4, `file://` e caminho local; inicia a
///   reprodução automaticamente.
/// - Os frames RGBA chegam por [`Player::frames`] em um canal com capacidade
///   limitada (frames antigos são descartados quando a UI não acompanha).
/// - Áudio é consumido por um `fakesink`: esta camada não abre dispositivo de
///   som.
pub struct Player {
    shared: Arc<Shared>,
    session: Mutex<Option<Session>>,
    frame_tx: SyncSender<Frame>,
    frame_rx: Receiver<Frame>,
    uri: Option<String>,
}

impl Player {
    /// Inicializa o GStreamer (uma vez por processo) e cria o player vazio.
    pub fn new() -> Result<Self> {
        init()?;
        let (frame_tx, frame_rx) = mpsc::sync_channel(FRAME_CHANNEL_CAPACITY);
        Ok(Self {
            shared: Arc::new(Shared::default()),
            session: Mutex::new(None),
            frame_tx,
            frame_rx,
            uri: None,
        })
    }

    /// Abre uma mídia e começa a reproduzir.
    ///
    /// Aceita:
    /// - `https://.../trailer.m3u8` (HLS da Steam) ou `http://`;
    /// - `https://.../trailer.mp4` e outros HTTP(S) que o typefind reconheça;
    /// - `file:///...` e caminhos locais (relativos ou absolutos).
    ///
    /// Antes de abrir HLS, confere `hlsdemux` e uma fonte HTTP
    /// (`souphttpsrc`, `curlhttpsrc` ou `neonhttpsrc`) e devolve
    /// [`Error::MissingPlugin`] com o nome exato do plugin que falta.
    /// A chamada apenas solicita `PLAYING` e retorna sem esperar o pré-roll; o
    /// buffering e o decode continuam em threads do GStreamer.
    pub fn open(&mut self, uri: &str) -> Result<()> {
        let source = uri::resolve(uri)?;
        ensure_plugins(source.kind)?;

        self.close_session();
        self.drain_frames();

        let session = self.build_session(&source)?;
        *lock(&self.session) = Some(session);
        self.uri = Some(source.uri);
        self.shared.set_state(PlayerState::Opening);
        if let Err(error) = self.start_pipeline() {
            self.shared.set_state(PlayerState::Error(error.to_string()));
            return Err(error);
        }
        Ok(())
    }

    /// Retoma a reprodução (ou recomeça do início depois de [`Self::stop`]).
    pub fn play(&self) -> Result<()> {
        let guard = lock(&self.session);
        let Some(session) = guard.as_ref() else {
            return Err(Error::NoMedia);
        };

        let state = self.shared.state();
        if let PlayerState::Error(message) = &state {
            return Err(Error::Pipeline(format!(
                "pipeline em erro; abra a mídia de novo: {message}"
            )));
        }

        if state == PlayerState::Ended {
            session
                .pipeline
                .seek_simple(
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::ClockTime::ZERO,
                )
                .map_err(|_| Error::Seek(Duration::ZERO))?;
        }
        if state != PlayerState::Playing {
            self.shared.set_state(PlayerState::Opening);
        }
        set_pipeline_state(&session.pipeline, gst::State::Playing)
    }

    /// Pausa a reprodução; o último frame continua disponível para a UI.
    pub fn pause(&self) -> Result<()> {
        let guard = lock(&self.session);
        let Some(session) = guard.as_ref() else {
            return Err(Error::NoMedia);
        };
        if matches!(
            self.shared.state(),
            PlayerState::Error(_) | PlayerState::Ended | PlayerState::Stopped
        ) {
            return Ok(());
        }
        set_pipeline_state(&session.pipeline, gst::State::Paused)?;
        self.shared.set_state(PlayerState::Paused);
        Ok(())
    }

    /// Faz seek absoluto (a partir do início do stream).
    ///
    /// Usa `FLUSH | ACCURATE`: descarta o que estava em voo e posiciona no
    /// frame pedido. Depois de um `Ended`, o seek volta a produzir frames;
    /// chame [`Self::play`] se o pipeline estiver pausado.
    pub fn seek(&self, position: Duration) -> Result<()> {
        let guard = lock(&self.session);
        let Some(session) = guard.as_ref() else {
            return Err(Error::NoMedia);
        };
        let position_clock =
            gst::ClockTime::try_from(position).map_err(|_| Error::Seek(position))?;
        session
            .pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                position_clock,
            )
            .map_err(|_| Error::Seek(position))?;
        if self.shared.state() == PlayerState::Ended {
            self.shared.set_state(PlayerState::Opening);
        }
        Ok(())
    }

    /// Para a reprodução e volta o pipeline para `READY`.
    ///
    /// A URI continua aberta (e a duração conhecida é mantida); um
    /// [`Self::play`] posterior recomeça do início. Para liberar tudo, abra
    /// outra mídia ou descarte o [`Player`].
    pub fn stop(&mut self) -> Result<()> {
        {
            let guard = lock(&self.session);
            let Some(session) = guard.as_ref() else {
                return Ok(());
            };
            set_pipeline_state(&session.pipeline, gst::State::Ready)?;
            self.shared.set_state(PlayerState::Stopped);
        }
        self.drain_frames();
        Ok(())
    }

    /// Estado atual (instantâneo, sem bloquear no pipeline).
    pub fn state(&self) -> PlayerState {
        self.shared.state()
    }

    /// Duração total, quando o container/playlist já informou.
    pub fn duration(&self) -> Option<Duration> {
        let queried = {
            let guard = lock(&self.session);
            guard
                .as_ref()
                .and_then(|session| session.pipeline.query_duration::<gst::ClockTime>())
                .map(Duration::from)
        };
        if let Some(duration) = queried {
            self.shared.set_duration(Some(duration));
            return Some(duration);
        }
        self.shared.duration()
    }

    /// Posição atual, quando o pipeline consegue responder.
    ///
    /// Depois de [`PlayerState::Ended`] devolve a duração (fim do stream).
    pub fn position(&self) -> Option<Duration> {
        let guard = lock(&self.session);
        let session = guard.as_ref()?;
        if let Some(position) = session.pipeline.query_position::<gst::ClockTime>() {
            return Some(Duration::from(position));
        }
        if self.shared.state() == PlayerState::Ended {
            return self.shared.duration();
        }
        None
    }

    /// Canal de frames RGBA.
    ///
    /// Use `try_recv` (ou [`Self::try_frame`]) na thread da UI; nunca
    /// `recv()` bloqueante no loop de eventos. O canal tem capacidade 2 e os
    /// frames mais novos são descartados se a UI não drenar, mantendo a
    /// memória limitada.
    pub fn frames(&self) -> &Receiver<Frame> {
        &self.frame_rx
    }

    /// Atalho não bloqueante para o próximo frame, se houver.
    pub fn try_frame(&self) -> Option<Frame> {
        self.frame_rx.try_recv().ok()
    }

    /// URI normalizada atualmente aberta.
    pub fn uri(&self) -> Option<&str> {
        self.uri.as_deref()
    }

    fn start_pipeline(&self) -> Result<()> {
        let guard = lock(&self.session);
        let Some(session) = guard.as_ref() else {
            return Err(Error::NoMedia);
        };
        set_pipeline_state(&session.pipeline, gst::State::Playing)
    }

    fn build_session(&self, source: &ResolvedSource) -> Result<Session> {
        let pipeline = gst::Pipeline::new();
        let playbin = make_element("playbin")?;

        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .build();
        let appsink = gst_app::AppSink::builder()
            .max_buffers(APP_SINK_MAX_BUFFERS)
            .drop(true)
            .sync(true)
            .caps(&caps)
            .build();

        let audio_sink = make_element("fakesink")?;
        audio_sink.set_property("sync", true);
        audio_sink.set_property("enable-last-sample", false);

        playbin.set_property("video-sink", &appsink);
        playbin.set_property("audio-sink", &audio_sink);
        playbin.set_property("uri", source.uri.as_str());

        pipeline.add(&playbin).map_err(|_| {
            Error::Pipeline("não foi possível adicionar o playbin ao pipeline".to_string())
        })?;

        let frame_tx = self.frame_tx.clone();
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let Ok(sample) = sink.pull_sample() else {
                        return Err(gst::FlowError::Eos);
                    };
                    if let Some(frame) = frame_from_sample(&sample) {
                        // Canal cheio: a UI está atrasada; descartar é melhor
                        // que bloquear a thread de streaming do GStreamer.
                        let _ = frame_tx.try_send(frame);
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        let bus = pipeline
            .bus()
            .ok_or_else(|| Error::Pipeline("pipeline sem bus".to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let bus_thread = thread::Builder::new()
            .name("arcadia-native-media-bus".to_string())
            .spawn({
                let pipeline = pipeline.clone();
                let shared = Arc::clone(&self.shared);
                let stop = Arc::clone(&stop);
                move || bus_loop(bus, pipeline, shared, stop)
            })
            .map_err(|error| Error::Pipeline(format!("falha ao criar a thread do bus: {error}")))?;

        Ok(Session {
            pipeline,
            stop,
            bus_thread: Some(bus_thread),
        })
    }

    fn close_session(&mut self) {
        let old = lock(&self.session).take();
        drop(old);
        self.uri = None;
        self.shared.reset();
    }

    fn drain_frames(&self) {
        while self.frame_rx.try_recv().is_ok() {}
    }
}

impl Default for Player {
    /// Cria o player assumindo que o GStreamer está instalado.
    ///
    /// Panica se o GStreamer não inicializar. Em código que precisa tratar
    /// essa falha, use [`Player::new`].
    fn default() -> Self {
        Self::new().unwrap_or_else(|error| {
            panic!("arcadia-native-media exige GStreamer 1.x instalado: {error}")
        })
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let old = lock(&self.session).take();
        drop(old);
    }
}

/// Inicializa o GStreamer uma única vez por processo.
///
/// É idempotente e chamado por [`Player::new`]; exposto para quem quer falhar
/// cedo (por exemplo, na inicialização do app) antes de construir o player.
pub fn init() -> Result<()> {
    static INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    match INIT.get_or_init(|| gst::init().map_err(|error| error.to_string())) {
        Ok(()) => Ok(()),
        Err(message) => Err(Error::Init(message.clone())),
    }
}

struct Session {
    pipeline: gst::Pipeline,
    stop: Arc<AtomicBool>,
    bus_thread: Option<JoinHandle<()>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(bus) = self.pipeline.bus() {
            bus.set_flushing(true);
        }
        let _ = self.pipeline.set_state(gst::State::Null);
        if let Some(handle) = self.bus_thread.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Default)]
struct Shared {
    state: Mutex<PlayerState>,
    duration: Mutex<Option<Duration>>,
}

impl Shared {
    fn state(&self) -> PlayerState {
        lock(&self.state).clone()
    }

    fn set_state(&self, state: PlayerState) {
        *lock(&self.state) = state;
    }

    /// Erros do bus não podem ser apagados por mensagens de estado atrasadas.
    fn set_state_if_not_error(&self, state: PlayerState) {
        let mut current = lock(&self.state);
        if !matches!(*current, PlayerState::Error(_)) {
            *current = state;
        }
    }

    fn reset(&self) {
        *lock(&self.state) = PlayerState::Idle;
        *lock(&self.duration) = None;
    }

    fn duration(&self) -> Option<Duration> {
        *lock(&self.duration)
    }

    fn set_duration(&self, duration: Option<Duration>) {
        if duration.is_some() {
            *lock(&self.duration) = duration;
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn make_element(name: &'static str) -> Result<gst::Element> {
    gst::ElementFactory::make(name)
        .build()
        .map_err(|_| Error::MissingElement(name))
}

fn set_pipeline_state(pipeline: &gst::Pipeline, state: gst::State) -> Result<()> {
    pipeline
        .set_state(state)
        .map(|_| ())
        .map_err(|_| Error::Pipeline(format!("o pipeline recusou o estado {state:?}")))
}

fn ensure_plugins(kind: MediaKind) -> Result<()> {
    for element in ["playbin", "appsink", "fakesink"] {
        require_plugin(element)?;
    }
    if kind == MediaKind::Hls {
        require_plugin("hlsdemux")?;
        let has_http_source = ["souphttpsrc", "curlhttpsrc", "neonhttpsrc"]
            .iter()
            .any(|element| gst::ElementFactory::find(element).is_some());
        if !has_http_source {
            return Err(Error::MissingPlugin("souphttpsrc"));
        }
    }
    Ok(())
}

fn require_plugin(element: &'static str) -> Result<()> {
    if gst::ElementFactory::find(element).is_some() {
        Ok(())
    } else {
        Err(Error::MissingPlugin(element))
    }
}

fn bus_loop(bus: gst::Bus, pipeline: gst::Pipeline, shared: Arc<Shared>, stop: Arc<AtomicBool>) {
    let types = [
        gst::MessageType::Error,
        gst::MessageType::Eos,
        gst::MessageType::StateChanged,
        gst::MessageType::AsyncDone,
        gst::MessageType::DurationChanged,
        gst::MessageType::Buffering,
    ];

    while !stop.load(Ordering::Relaxed) {
        let message =
            bus.timed_pop_filtered(gst::ClockTime::from_mseconds(BUS_POLL_INTERVAL_MS), &types);
        if let Some(message) = message {
            handle_message(&message, &pipeline, &shared);
        }
    }
}

fn handle_message(message: &gst::Message, pipeline: &gst::Pipeline, shared: &Shared) {
    match message.view() {
        gst::MessageView::Eos(_) => {
            shared.set_state_if_not_error(PlayerState::Ended);
        }
        gst::MessageView::Error(error) => {
            let mut text = error.error().to_string();
            if let Some(debug) = error.debug() {
                let debug = debug.trim();
                if !debug.is_empty() {
                    text.push_str(" — ");
                    text.push_str(debug);
                }
            }
            shared.set_state(PlayerState::Error(text));
        }
        gst::MessageView::DurationChanged(_) => {
            if let Some(duration) = query_duration(pipeline) {
                shared.set_duration(Some(duration));
            }
        }
        gst::MessageView::AsyncDone(_) => {
            if let Some(duration) = query_duration(pipeline) {
                shared.set_duration(Some(duration));
            }
            if pipeline.current_state() == gst::State::Playing {
                shared.set_state_if_not_error(PlayerState::Playing);
            }
        }
        gst::MessageView::StateChanged(state) if is_from_pipeline(message, pipeline) => {
            match state.current() {
                gst::State::Playing => {
                    shared.set_state_if_not_error(PlayerState::Playing);
                }
                gst::State::Paused => {
                    shared.set_state_if_not_error(PlayerState::Paused);
                }
                _ => {}
            }
        }
        gst::MessageView::Buffering(buffering) => {
            let percent = buffering.percent();
            let current = shared.state();
            if percent < 100 {
                if current == PlayerState::Playing {
                    shared.set_state_if_not_error(PlayerState::Opening);
                }
            } else if current == PlayerState::Opening {
                shared.set_state_if_not_error(PlayerState::Playing);
            }
        }
        _ => {}
    }
}

fn is_from_pipeline(message: &gst::Message, pipeline: &gst::Pipeline) -> bool {
    message
        .src()
        .is_some_and(|source| source == pipeline.upcast_ref::<gst::Object>())
}

fn query_duration(pipeline: &gst::Pipeline) -> Option<Duration> {
    pipeline
        .query_duration::<gst::ClockTime>()
        .map(Duration::from)
}

/// Copia o sample para um `Frame` RGBA compacto.
///
/// O `appsink` negocia `video/x-raw,format=RGBA`; o `VideoInfo` informa o
/// stride real, que pode ter padding de alinhamento. Este é o único ponto que
/// toca no buffer do GStreamer, e o resultado já é seguro para a UI (sem
/// ponteiro para memória do pipeline).
fn frame_from_sample(sample: &gst::Sample) -> Option<Frame> {
    let caps = sample.caps()?;
    let info = gst_video::VideoInfo::from_caps(caps).ok()?;
    let buffer = sample.buffer()?;
    let map = buffer.map_readable().ok()?;
    let data = map.as_slice();

    let width = info.width();
    let height = info.height();
    if width == 0 || height == 0 {
        return None;
    }

    let source_stride = usize::try_from(*info.stride().first()?).ok()?;
    let row_bytes = (width as usize).checked_mul(4)?;
    if source_stride < row_bytes {
        return None;
    }
    let required = source_stride.checked_mul(height as usize)?;
    if data.len() < required {
        return None;
    }

    let mut bytes = Vec::with_capacity(row_bytes.checked_mul(height as usize)?);
    if source_stride == row_bytes {
        bytes.extend_from_slice(&data[..required]);
    } else {
        for row in 0..height as usize {
            let start = row * source_stride;
            bytes.extend_from_slice(&data[start..start + row_bytes]);
        }
    }

    Some(Frame {
        width,
        height,
        stride: row_bytes,
        bytes,
        pts: buffer.pts().map(Duration::from),
    })
}
