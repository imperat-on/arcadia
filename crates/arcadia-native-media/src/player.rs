//! Player GStreamer independente de UI.
//!
//! O `playbin` decodifica em threads próprias do GStreamer; o callback do
//! `appsink` copia cada frame para RGBA e o entrega por um canal limitado. A
//! thread da UI nunca é bloqueada por I/O, decode ou espera de clock: ela só
//! chama [`Player::frames`] e drena o que estiver pronto.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;

use crate::error::{Error, Result};
use crate::frame::Frame;
use crate::uri::{self, MediaKind, ResolvedSource};

/// Quantos buffers o `appsink` mantém antes de descartar os antigos.
///
/// Com `sync(false)` o `playbin` entrega o stream o mais rápido que o decoder
/// produz; a UI drena a 60 Hz. Um buffer de 2 fazia o sink descartar quase
/// tudo e o card mostrar ~2 frames por loop (medido: 2 frames a cada 4 s).
/// 8 dá folga para a UI atrasar um tick sem perder o frame seguinte, e ainda
/// limita a memória a 8 × 921 kB ≈ 7 MB por card.
const APP_SINK_MAX_BUFFERS: u32 = 8;
/// Capacidade do canal de frames. Mesma lógica do `APP_SINK_MAX_BUFFERS`: 2
/// era apertado demais para o ritmo real da UI. Frames velhos são descartados
/// em vez de crescer memória sem limite.
const FRAME_CHANNEL_CAPACITY: usize = 8;
/// Largura de entrega do vídeo para a UI.
///
/// Só a largura é fixada: o `videoscale` calcula a altura pela proporção do
/// stream. Fixar as duas dimensões (como era 640×360) obrigava o escala a
/// **esticar** todo vídeo que não fosse 16:9 — os cards de seção da Steam são
/// 1170×438 (2,67:1) e saíam achatados.
///
/// O teto de altura existe só para o caso de um vídeo vertical, que a essa
/// largura passaria dos 2 MiB do upload síncrono do `iced_wgpu`.
const VIDEO_SINK_WIDTH: i32 = 640;
/// Teto de altura: 640×768 RGBA = 1,9 MiB, abaixo do limite de 2 MiB.
const VIDEO_SINK_HEIGHT: i32 = 360;
const VIDEO_SINK_MAX_HEIGHT: i32 = 768;
/// Teto do upload síncrono do `iced_wgpu`. Um frame RGBA maior que isto é
/// entregue fora do quadro e a imagem não aparece naquele render.
const ICE_SYNC_UPLOAD_LIMIT: usize = 2 * 1024 * 1024;

/// Um frame RGBA deste tamanho cabe no upload síncrono do renderer e é
/// desenhado no mesmo quadro em que foi entregue.
///
/// Exposto porque o frontend precisa poder afirmar o contrato: um frame acima
/// do teto do `iced_wgpu` é entregue fora do quadro e a imagem simplesmente
/// não aparece naquele render — a piscada.
pub fn frame_fits_sync_upload(width: u32, height: u32) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4)
        < ICE_SYNC_UPLOAD_LIMIT
}
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
            shared: Arc::new(Shared::new()),
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
        self.reset_pacing();

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
            // Recomeço do loop: o PTS volta a zero, então a âncora do ritmo
            // precisa ser refeita — senão o relógio antigo acha que o frame
            // está "muito atrás" e libera tudo em rajada (2x).
            self.reset_pacing();
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
        // Seek muda a base do PTS; a âncora é refeita no próximo frame.
        self.reset_pacing();
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

        // A UI desenha o trailer num painel de ~640×340, e o renderer só
        // desenha no mesmo quadro imagens de até 2 MiB (ver
        // [`ICE_SYNC_UPLOAD_LIMIT`]). O bin abaixo reduz para 640×360 antes do
        // appsink, preservando a proporção com barras.
        debug_assert!(
            frame_fits_sync_upload(VIDEO_SINK_WIDTH as u32, VIDEO_SINK_MAX_HEIGHT as u32),
            "frame {VIDEO_SINK_WIDTH}×{VIDEO_SINK_MAX_HEIGHT} passa do upload síncrono e pisca"
        );
        // O `videoscale` do GStreamer NÃO preserva proporção sozinho:
        // - caps só de largura: mantém a altura da fonte (640×438, medido);
        // - caps com as duas dimensões fixas em 16:9: estica (640×360).
        // - `add-borders=true`: não adiciona barras nesta build (800×360
        //   medido com conteúdo preenchendo tudo).
        //
        // O que funciona é declarar `pixel-aspect-ratio=1/1` nos DOIS lados
        // do escala. Aí o elemento mantém a razão largura/altura da fonte e
        // calcula a altura sozinho. Verificado com o card real do Black Myth:
        // Wukong (1170×438, 2,67:1): sai 640×240, ratio 2,667 — proporção
        // intacta, sem barras.
        //
        // O teto de 2 MiB do upload síncrono continua respeitado: o pior caso
        // é um vídeo vertical em 640 de largura; a altura fica abaixo de 768.
        let sink_caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .field("width", VIDEO_SINK_WIDTH)
            .build();
        let source_caps = gst::Caps::builder("video/x-raw")
            .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
            .build();
        let convert = make_element("videoconvert")?;
        let scale = make_element("videoscale")?;
        let src_filter = gst::ElementFactory::make("capsfilter")
            .property("caps", &source_caps)
            .build()
            .map_err(|_| Error::MissingElement("capsfilter"))?;
        let filter = gst::ElementFactory::make("capsfilter")
            .property("caps", &sink_caps)
            .build()
            .map_err(|_| Error::MissingElement("capsfilter"))?;
        let _ = VIDEO_SINK_MAX_HEIGHT;
        let appsink = gst_app::AppSink::builder()
            .max_buffers(APP_SINK_MAX_BUFFERS)
            .drop(true)
            // `sync(true)` amarra o appsink ao relógio do pipeline: em
            // PLAYING cada buffer espera o seu PTS, e como a UI drena a
            // 60 Hz, o sink passa a devolver no máximo os 2 primeiros
            // buffers (o `max_buffers`) e trava — medido: 2 frames e depois
            // silêncio. O `poster.rs` já usava `sync(false)` e por isso
            // extraía frame do mesmo vídeo sem problema.
            //
            // Sem sync, o `playbin` continua com o ritmo correto (o
            // `video-sink` é só consumidor) e o appsink entrega o que chega.
            .sync(false)
            .caps(&sink_caps)
            .build();

        let video_bin = gst::Bin::new();
        video_bin
            .add_many([&convert, &src_filter, &scale, &filter, appsink.upcast_ref()])
            .map_err(|_| Error::Pipeline("não foi possível montar o sink de vídeo".to_string()))?;
        gst::Element::link_many([&convert, &src_filter, &scale, &filter, appsink.upcast_ref()])
            .map_err(|_| Error::Pipeline("não foi possível ligar o sink de vídeo".to_string()))?;
        let sink_pad = convert
            .static_pad("sink")
            .ok_or_else(|| Error::Pipeline("sink de vídeo sem pad de entrada".to_string()))?;
        let ghost = gst::GhostPad::with_target(&sink_pad)
            .map_err(|_| Error::Pipeline("falha ao criar ghost pad do vídeo".to_string()))?;
        video_bin
            .add_pad(&ghost)
            .map_err(|_| Error::Pipeline("falha ao adicionar ghost pad do vídeo".to_string()))?;

        let audio_sink = make_element("fakesink")?;
        audio_sink.set_property("sync", true);
        audio_sink.set_property("enable-last-sample", false);

        // O Arcadia é mudo por enquanto: decodificar áudio e legendas só
        // consome CPU. `video` mantém o pipeline de imagem e desliga o resto.
        playbin.set_property_from_str("flags", "video");
        playbin.set_property("video-sink", &video_bin);
        playbin.set_property("audio-sink", &audio_sink);
        playbin.set_property("uri", source.uri.as_str());

        pipeline.add(&playbin).map_err(|_| {
            Error::Pipeline("não foi possível adicionar o playbin ao pipeline".to_string())
        })?;

        let frame_tx = self.frame_tx.clone();
        let pacing = Arc::clone(&self.shared);
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let Ok(sample) = sink.pull_sample() else {
                        return Err(gst::FlowError::Eos);
                    };
                    let Some(frame) = frame_from_sample(&sample) else {
                        return Ok(gst::FlowSuccess::Ok);
                    };

                    // Ritmo de tempo real. O appsink roda com `sync(false)`,
                    // então os buffers chegam na velocidade do decoder (~5x
                    // em HLS local). Aqui cada frame espera o seu instante,
                    // ancorado no primeiro PTS. O `sleep` acontece na thread
                    // de streaming — que é exatamente onde o GStreamer
                    // esperaria com `sync(true)`, sem o travamento do appsink.
                    if let Some(pts) = frame.pts {
                        let delay = {
                            let now = Instant::now();
                            let mut guard = lock(&pacing.pacing);
                            let (wait, resync) = match guard.as_ref() {
                                Some(clock) => clock.decide(pts, now),
                                None => (None, true),
                            };
                            if resync {
                                *guard = Some(PacingClock {
                                    base_pts: pts,
                                    base_instant: now,
                                });
                            }
                            wait
                        };
                        // Espera interruptível: `set_state(Null)` precisa do
                        // stream lock que este callback segura. Um `sleep`
                        // comum deixava o fechamento do player (e o clique
                        // que troca de jogo) preso até o PTS do frame.
                        if let Some(delay) = delay {
                            pacing.wait_pacing(delay);
                        }
                    }

                    // Canal cheio: a UI está atrasada; descartar é melhor
                    // que bloquear a thread de streaming do GStreamer.
                    let _ = frame_tx.try_send(frame);
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
        // Acorda o callback antes de derrubar o pipeline. Sem isso o
        // streaming thread continua dormindo com o stream lock na mão.
        self.shared.cancel_pacing();
        let old = lock(&self.session).take();
        drop(old);
        self.uri = None;
        self.shared.reset();
    }

    fn reset_pacing(&self) {
        *lock(&self.shared.pacing) = None;
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
        // O teardown do GStreamer roda fora da thread que pediu o
        // fechamento. `set_state(Null)` espera o streaming thread soltar o
        // stream lock; se o callback do appsink estiver no meio do ritmo de
        // um frame, essa espera chega a centenas de milissegundos e trava a
        // UI que trocou de jogo. O `wait_pacing` acorda na hora, e o `join`
        // da thread do bus fica neste worker.
        let pipeline = self.pipeline.clone();
        let bus_thread = self.bus_thread.take();
        thread::Builder::new()
            .name("arcadia-native-media-stop".to_string())
            .spawn(move || {
                let _ = pipeline.set_state(gst::State::Null);
                if let Some(handle) = bus_thread {
                    let _ = handle.join();
                }
            })
            .ok();
    }
}

struct Shared {
    state: Mutex<PlayerState>,
    duration: Mutex<Option<Duration>>,
    /// Âncora do relógio de reprodução: `Instant` local em que o PTS 0
    /// deveria ter sido exibido, e o PTS do primeiro frame entregue.
    ///
    /// O appsink roda com `sync(false)` (ver `start_pipeline`), então os
    /// buffers chegam na velocidade do decoder — em HLS local isso é ~5x
    /// mais rápido que tempo real. Em vez de religar o `sync` do appsink
    /// (que trava com `max_buffers`), o callback usa esta âncora para
    /// segurar cada frame até o seu instante.
    pacing: Mutex<Option<PacingClock>>,
    /// Acorda o callback do appsink quando a sessão está sendo fechada.
    pacing_stop: AtomicBool,
    pacing_wake: Condvar,
}

/// Relógio de tempo real ancorado no primeiro PTS entregue.
#[derive(Debug, Clone, Copy)]
struct PacingClock {
    /// PTS do primeiro frame (base de comparação).
    base_pts: Duration,
    /// `Instant` em que o `base_pts` foi (ou deveria ter sido) exibido.
    base_instant: Instant,
}

impl PacingClock {
    /// Decisão de ritmo para um frame: quanto dormir e se o clock deve ser
    /// re-ancorado.
    ///
    /// Re-ancorar cobre seek e retomada de pausa: quando o PTS salta para
    /// trás (seek regressivo) ou para muito à frente (o pipeline continuou
    /// correndo), a âncora antiga produziria uma espera enorme ou negativa.
    /// Um salto acima de [`RESYNC_TOLERANCE`] reinicia o relógio.
    fn decide(&self, pts: Duration, now: Instant) -> (Option<Duration>, bool) {
        let Some(elapsed) = pts.checked_sub(self.base_pts) else {
            // PTS antes do primeiro frame: seek regressivo.
            return (None, true);
        };
        let due = self.base_instant + elapsed;
        let ahead = due.checked_duration_since(now);
        match ahead {
            Some(wait) if wait > RESYNC_TOLERANCE => (None, true),
            Some(wait) => (Some(wait), false),
            // Atrasado (UI lenta ou primeira rajada): segue sem esperar.
            None => (None, false),
        }
    }
}

/// Salto de PTS acima disto (adiante do previsto) reinicia a âncora, em vez
/// de gerar uma espera absurda.
const RESYNC_TOLERANCE: Duration = Duration::from_millis(600);

impl Shared {
    fn new() -> Self {
        Self {
            state: Mutex::new(PlayerState::Idle),
            duration: Mutex::new(None),
            pacing: Mutex::new(None),
            pacing_stop: AtomicBool::new(false),
            pacing_wake: Condvar::new(),
        }
    }

    /// Espera o instante do frame, ou acorda na hora se a sessão fechar.
    ///
    /// O callback segura o stream lock do GStreamer durante esta espera. Um
    /// sleep comum fazia `set_state(Null)` — chamado ao trocar de jogo —
    /// ficar bloqueado até o PTS do próximo frame.
    fn wait_pacing(&self, delay: Duration) {
        if self.pacing_stop.load(Ordering::Relaxed) || delay.is_zero() {
            return;
        }
        let guard = lock(&self.pacing);
        let (guard, _) = self
            .pacing_wake
            .wait_timeout_while(guard, delay, |_| !self.pacing_stop.load(Ordering::Relaxed))
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        drop(guard);
    }

    fn cancel_pacing(&self) {
        self.pacing_stop.store(true, Ordering::Relaxed);
        self.pacing_wake.notify_all();
    }

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
        self.pacing_stop.store(false, Ordering::Relaxed);
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

    // O sink usa `videoscale add-borders=true` com molde 16:9 para preservar
    // a proporção de qualquer fonte. Um card da Steam 1170×438 (2,67:1) chega
    // como 640×360 com tarja preta em cima e embaixo; entregar assim deixa o
    // card mais alto que o conteúdo. As tarjas são removidas aqui.
    let (top, bottom) = letterbox_bounds(&bytes, width, height);
    if top > 0 || bottom < height {
        let kept_height = bottom.saturating_sub(top);
        if kept_height > 0 {
            let start = top as usize * row_bytes;
            let end = start + kept_height as usize * row_bytes;
            bytes = bytes[start..end].to_vec();
        }
        return Some(Frame {
            width,
            height: kept_height,
            stride: row_bytes,
            bytes,
            pts: buffer.pts().map(Duration::from),
        });
    }

    Some(Frame {
        width,
        height,
        stride: row_bytes,
        bytes,
        pts: buffer.pts().map(Duration::from),
    })
}

/// Primeira e última linha que **não** são tarja preta.
///
/// Uma linha é tarja quando 99% dos seus pixels têm os três canais abaixo de
/// 8. O limiar alto evita cortar cena escura: a tarja real do `add-borders` é
/// preto uniforme, enquanto uma cena noturna tem variação.
///
/// A função nunca corta mais de um terço de cada lado — se o vídeo for
/// legitimamente escuro, nada é removido.
fn letterbox_bounds(rgba: &[u8], width: u32, height: u32) -> (u32, u32) {
    let row_bytes = width as usize * 4;
    if row_bytes == 0 || height == 0 {
        return (0, height);
    }

    let is_bar = |row: u32| -> bool {
        let start = row as usize * row_bytes;
        let end = start + row_bytes;
        if end > rgba.len() {
            return false;
        }
        let pixels = &rgba[start..end];
        let total = pixels.len() / 4;
        if total == 0 {
            return false;
        }
        let dark = pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[0] < 8 && pixel[1] < 8 && pixel[2] < 8)
            .count();
        dark * 100 >= total * 99
    };

    let mut top = 0u32;
    while top < height && is_bar(top) {
        top += 1;
    }
    let mut bottom = height;
    while bottom > top && is_bar(bottom - 1) {
        bottom -= 1;
    }

    // Corte de segurança: nada além de um terço por lado.
    if top > height / 3 || (height - bottom) > height / 3 {
        return (0, height);
    }

    (top, bottom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sink_resolution_fits_the_renderer_sync_upload() {
        // Se este teste quebrar, o vídeo volta a piscar: um frame acima de
        // 2 MiB é entregue fora do quadro pelo `iced_wgpu` e a imagem some
        // por um render.
        assert!(
            frame_fits_sync_upload(VIDEO_SINK_WIDTH as u32, VIDEO_SINK_MAX_HEIGHT as u32),
            "{VIDEO_SINK_WIDTH}×{VIDEO_SINK_MAX_HEIGHT} RGBA passa do upload síncrono"
        );
    }

    #[test]
    fn sync_upload_boundary_matches_the_renderer_limit() {
        // 1024×512 = 512 KiB por plano de 4 B = 2 MiB exatos: o renderer usa
        // `< MAX_SYNC_SIZE`, então o limite é exclusivo.
        assert!(frame_fits_sync_upload(1024, 511));
        assert!(!frame_fits_sync_upload(1024, 512));
        // O 720p anterior, que causava a piscada, precisa ser rejeitado.
        assert!(!frame_fits_sync_upload(1280, 720));
    }

    #[test]
    fn sync_upload_rejects_absurd_dimensions_without_overflow() {
        assert!(!frame_fits_sync_upload(u32::MAX, u32::MAX));
        assert!(!frame_fits_sync_upload(0, 720));
    }

    /// O callback do appsink dorme até o PTS do frame segurando o stream
    /// lock. Fechar a sessão tem de acordar essa espera na hora; se voltar a
    /// ser um `sleep`, trocar de jogo trava a UI pelo tempo do frame.
    #[test]
    fn closing_wakes_a_frame_wait() {
        let shared = Shared::new();
        let waiting = Arc::new(shared);
        let observed = Arc::clone(&waiting);
        let started = Instant::now();
        let handle = thread::spawn(move || {
            observed.wait_pacing(Duration::from_secs(5));
        });

        // A espera só entra no condvar depois de observar o flag. Sem este
        // intervalo o cancelamento pode ganhar a corrida e o teste não mede
        // o despertar.
        thread::sleep(Duration::from_millis(30));
        waiting.cancel_pacing();
        handle.join().expect("a espera do frame terminou");
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "fechar a sessão não acordou a espera do frame"
        );
    }
}
