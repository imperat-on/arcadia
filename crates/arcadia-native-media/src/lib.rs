//! Player de vídeo GStreamer reutilizável pelo frontend nativo do Arcadia.
//!
//! Este crate não depende de iced, webview, hls.js ou MSE. Ele entrega
//! [`Frame`]s RGBA por um [`std::sync::mpsc::Receiver`] e deixa a textura e o
//! loop de eventos por conta da UI.
//!
//! # API
//!
//! ```no_run
//! use std::time::Duration;
//! use arcadia_native_media::Player;
//!
//! # fn exemplo() -> Result<(), Box<dyn std::error::Error>> {
//! let mut player = Player::new()?;
//! player.open("https://cdn.akamai.steamstatic.com/steam/apps/620/trailer.m3u8")?;
//!
//! // Na thread da UI, a cada tick:
//! while let Some(frame) = player.try_frame() {
//!     assert_eq!(frame.stride, frame.width as usize * 4);
//!     let _rgba = frame.bytes; // iced::widget::image::Handle::from_rgba(...)
//! }
//! player.pause()?;
//! player.seek(Duration::from_secs(3))?;
//! player.play()?;
//! player.stop()?;
//! # Ok(())
//! # }
//! ```
//!
//! - [`Player::open`] aceita HTTP(S) HLS/MP4, `file://` e caminho local, e já
//!   inicia a reprodução.
//! - [`Player::play`], [`Player::pause`], [`Player::seek`] e [`Player::stop`]
//!   controlam o pipeline sem bloquear no decode.
//! - [`Player::state`] devolve [`PlayerState`]; erros assíncronos do bus viram
//!   `PlayerState::Error(String)` em vez de sumirem.
//! - [`Player::duration`] e [`Player::position`] respondem quando o container
//!   ou a playlist informa.
//!
//! # Pipeline
//!
//! `playbin` recebe `video-sink` = `appsink` com caps
//! `video/x-raw,format=RGBA` e `audio-sink` = `fakesink`. O `playbin` escolhe
//! `uridecodebin`/`decodebin` e insere `videoconvert` quando o decoder entrega
//! outro formato. O decode roda nas threads do GStreamer; o callback do
//! `appsink` copia o buffer (já sem padding de linha) para [`Frame::bytes`] e
//! envia por um canal limitado.
//!
//! ## HLS (trailers da Steam)
//!
//! Não há MSE: para uma URI `.m3u8`, o typefind entrega
//! `application/x-hls` e o `playbin` usa `hlsdemux`, que baixa os segmentos
//! pela fonte HTTP do sistema (normalmente `souphttpsrc`). Antes de abrir,
//! [`Player::open`] verifica `hlsdemux` e uma fonte HTTP e devolve
//! [`Error::MissingPlugin`] com o nome exato do elemento ausente.
//!
//! Plugins necessários:
//!
//! | Uso | Plugin/elemento | Pacote típico |
//! |---|---|---|
//! | pipeline base | `playbin` | `gstreamer1.0-plugins-base` / `gst-plugins-base` |
//! | janela de vídeo | `appsink` | `gstreamer1.0-plugins-base` / `gst-plugins-base` |
//! | HTTP | `souphttpsrc` | `gstreamer1.0-plugins-good` / `gst-plugins-good` |
//! | HLS | `hlsdemux` | `gstreamer1.0-plugins-bad` / `gst-plugins-bad` |
//! | H.264 | `openh264dec` **ou** `avdec_h264` | `gstreamer1.0-plugins-bad` ou `gstreamer1.0-libav` |
//! | MP4 | `qtdemux` | `gstreamer1.0-plugins-good` / `gst-plugins-good` |
//!
//! O AppImage do Arcadia já injeta `libgsthls.so`, `libgstopenh264.so` e
//! `libopenh264.so.8`. O processo precisa enxergá-los: o `AppRun`/empacotamento
//! deve manter `GST_PLUGIN_PATH` (e `GST_PLUGIN_SYSTEM_PATH` quando for o caso)
//! apontando para os plugins embarcados. Sem `hlsdemux`, o erro é explícito em
//! [`Error::MissingPlugin`]; sem decoder H.264, o bus reporta o erro em
//! [`PlayerState::Error`].
//!
//! # Integração com iced
//!
//! O crate não importa iced. Configure uma `Subscription` de tick (por exemplo
//! `iced::time::every(Duration::from_millis(16))`), drene os frames no `update`
//! e guarde o `iced::widget::image::Handle` mais recente:
//!
//! ```ignore
//! use std::time::Duration;
//! use arcadia_native_media::{Frame, Player};
//!
//! struct Trailer {
//!     player: Player,
//!     textura: Option<iced::widget::image::Handle>,
//! }
//!
//! impl Trailer {
//!     fn drenar_frames(&mut self) {
//!         // Não use recv()/recv_timeout() aqui: bloquearia o loop da UI.
//!         while let Ok(Frame { width, height, bytes, .. }) = self.player.frames().try_recv() {
//!             self.textura = Some(iced::widget::image::Handle::from_rgba(width, height, bytes));
//!         }
//!     }
//! }
//! ```
//!
//! A view usa `iced::widget::image(handle).content_fit(iced::ContentFit::Contain)`.
//! Como `Frame::stride == width * 4` e as linhas já vêm compactadas, o
//! `Vec<u8>` de [`Frame::bytes`] é aceito direto por `Handle::from_rgba`.
//! Para saber quando mostrar spinner/erro, consulte [`Player::state`] no mesmo
//! tick: `Opening` cobre buffering de HLS e `Error(msg)` traz o texto do bus.
//!
//! # Limites
//!
//! - Áudio é consumido por `fakesink`; este crate não escolhe dispositivo de
//!   som.
//! - O canal de frames tem capacidade 2. Se a UI não drenar, os frames mais
//!   novos são descartados para não crescer memória sem limite (um frame 4K
//!   RGBA ocupa ~33 MB).
//! - `open`/`stop`/`Drop` esperam o pipeline voltar para `NULL` e a thread do
//!   bus encerrar; isso pode custar alguns milissegundos, principalmente em
//!   rede lenta.
//! - Seek é absoluto e usa `FLUSH | ACCURATE`.

pub mod error;
pub mod frame;
pub mod player;

mod uri;

pub use error::{Error, Result};
pub use frame::Frame;
pub use player::{init, Player, PlayerState};
pub use uri::MediaKind;
