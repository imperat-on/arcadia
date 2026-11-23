//! Player de trailer da página do jogo.
//!
//! Fachada fina sobre [`arcadia_native_media::Player`]. O pipeline GStreamer é
//! criado sob demanda (primeiro trailer aberto) e os frames RGBA são drenados
//! pela `Subscription` de tick do iced, nunca por bloqueio na thread da UI.
//!
//! Prioridade de fonte: `hls` (trailers atuais da Steam), depois `mp4` e por
//! fim `webm`. Áudio fica de fora nesta fase: o crate usa `fakesink`.

use std::time::{Duration, Instant};

use arcadia_native_media::{Frame, Player, PlayerState};
use iced::widget::image::Handle;

/// Aviso exibido na UI: o player desta onda é mudo de propósito.
#[allow(dead_code)]
pub const AUDIO_NOTE: &str =
    "Sem áudio nesta fase: o GStreamer usa fakesink e o trailer toca mudo.";

/// Fontes de um trailer, na ordem de preferência do player.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrailerSources {
    pub hls: Option<String>,
    pub mp4: Option<String>,
    pub webm: Option<String>,
}

impl TrailerSources {
    /// Todas as URIs na ordem de tentativa (`hls` → `mp4` → `webm`), sem
    /// vazias.
    fn candidates(&self) -> Vec<String> {
        [
            self.hls.as_deref(),
            self.mp4.as_deref(),
            self.webm.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|uri| !uri.is_empty())
        .map(str::to_string)
        .collect()
    }
}

/// Estado observável do trailer na UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum MediaStatus {
    /// Nenhum trailer aberto.
    #[default]
    Idle,
    /// Abrindo o pipeline, baixando playlist ou fazendo pré-roll.
    Loading,
    /// Reproduzindo.
    Playing,
    /// Pausado (ou parado no fim).
    Paused,
    /// Fim do stream; um novo play recomeça do início.
    Ended,
    /// Erro local (plugin ausente, URI inválida) ou reportado pelo bus.
    Error(String),
}

impl MediaStatus {
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    /// Texto curto para a UI. Erros carregam a mensagem do GStreamer.
    #[allow(dead_code)]
    pub fn label(&self) -> &str {
        match self {
            Self::Idle => "Nenhum trailer carregado.",
            Self::Loading => "Carregando trailer…",
            Self::Playing => "Tocando (mudo)",
            Self::Paused => "Pausado",
            Self::Ended => "Fim do trailer",
            Self::Error(message) => message,
        }
    }
}

/// Estado de mídia da página do jogo, dono do [`Player`] quando existe.
#[derive(Default)]
pub struct MediaPlayer {
    player: Option<Player>,
    texture: Option<Handle>,
    status: MediaStatus,
    duration: Option<Duration>,
    position: Option<Duration>,
    uri: Option<String>,
    opened_at: Option<Instant>,
    first_frame_ms: Option<u128>,
}

impl MediaPlayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` quando já existe pipeline para esta página (tick deve rodar).
    pub fn is_active(&self) -> bool {
        self.player.is_some()
    }

    /// Abre a melhor fonte do trailer (`hls` → `mp4` → `webm`).
    ///
    /// A chamada não espera o pré-roll: o estado vira `Loading` e a UI descobre
    /// o primeiro frame no tick seguinte. Erros síncronos (plugin ausente,
    /// URI inválida) tentam a próxima fonte da lista.
    pub fn open(&mut self, sources: &TrailerSources) -> Result<(), String> {
        let candidates = sources.candidates();
        if candidates.is_empty() {
            let message = "O trailer veio sem URL tocável (hls/mp4/webm).".to_string();
            self.status = MediaStatus::Error(message.clone());
            return Err(message);
        }

        let mut last_error = String::new();
        for uri in &candidates {
            match self.open_uri(uri) {
                Ok(()) => return Ok(()),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    /// Abre um trailer já baixado (`file://…` ou caminho local).
    ///
    /// O caminho vem de `trailer:downloadUrl`/`trailer:path`; o `playbin` do
    /// GStreamer aceita os dois formatos. Caminho vazio falha antes de criar
    /// pipeline, para a UI não ficar presa num "carregando" sem mídia.
    pub fn open_local(&mut self, path: impl Into<String>) -> Result<(), String> {
        let path = path.into();
        let uri = path.trim();
        if uri.is_empty() {
            let message = "O trailer baixado veio sem caminho local.".to_string();
            self.status = MediaStatus::Error(message.clone());
            return Err(message);
        }

        self.open(&TrailerSources {
            hls: None,
            mp4: Some(uri.to_string()),
            webm: None,
        })
    }

    fn open_uri(&mut self, uri: &str) -> Result<(), String> {
        if self.player.is_none() {
            match Player::new() {
                Ok(player) => self.player = Some(player),
                Err(error) => {
                    let message = format!("GStreamer indisponível: {error}");
                    self.status = MediaStatus::Error(message.clone());
                    return Err(message);
                }
            }
        }

        self.texture = None;
        self.duration = None;
        self.position = Some(Duration::ZERO);
        self.first_frame_ms = None;
        self.opened_at = Some(Instant::now());
        self.status = MediaStatus::Loading;

        let player = self.player.as_mut().expect("player criado acima");
        match player.open(uri) {
            Ok(()) => {
                self.uri = Some(uri.to_string());
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                self.status = MediaStatus::Error(message.clone());
                Err(message)
            }
        }
    }

    /// Drena os frames disponíveis e sincroniza estado/duração/posição.
    ///
    /// Chamada pela `Subscription` (~60 fps). Nunca bloqueia: usa
    /// `try_frame` e mantém apenas o frame mais novo do lote.
    pub fn tick(&mut self) {
        let Some(player) = self.player.as_ref() else {
            return;
        };

        let mut latest: Option<Frame> = None;
        while let Some(frame) = player.try_frame() {
            latest = Some(frame);
        }

        if let Some(frame) = latest {
            if self.first_frame_ms.is_none() {
                if let Some(opened_at) = self.opened_at {
                    let elapsed = opened_at.elapsed().as_millis();
                    self.first_frame_ms = Some(elapsed);
                    eprintln!(
                        "[arcadia-native] primeiro frame do trailer em {elapsed} ms ({}x{})",
                        frame.width, frame.height
                    );
                }
            }
            // O renderer do iced só desenha no mesmo quadro o que cabe no
            // upload síncrono de 2 MiB. Aceitar um frame maior aqui produz um
            // handle que some por um quadro — exatamente a piscada que o
            // `arcadia-native-media` evita ao reduzir o vídeo. Se um dia
            // chegar um frame acima do teto, ele é descartado em vez de
            // virar buraco preto.
            if arcadia_native_media::frame_fits_sync_upload(frame.width, frame.height) {
                self.texture = Some(Handle::from_rgba(frame.width, frame.height, frame.bytes));
            }
        }

        self.duration = player.duration().or(self.duration);
        self.position = player.position().or(self.position);

        let state = player.state();
        if let PlayerState::Error(message) = state {
            self.status = MediaStatus::Error(message);
            return;
        }
        // Erros locais de `open` (plugin ausente/URI inválida) não podem ser
        // apagados pelo `Idle` do pipeline, que nunca chegou a abrir mídia.
        if self.status.is_error() {
            return;
        }

        self.status = match state {
            PlayerState::Idle => return,
            PlayerState::Opening => MediaStatus::Loading,
            PlayerState::Playing => MediaStatus::Playing,
            // O `playbin` passa por PAUSED no pré-roll do HLS; não pisca
            // "Pausado" antes do primeiro frame.
            PlayerState::Paused | PlayerState::Stopped
                if self.texture.is_none() && self.status == MediaStatus::Loading =>
            {
                MediaStatus::Loading
            }
            PlayerState::Paused | PlayerState::Stopped => MediaStatus::Paused,
            PlayerState::Ended => MediaStatus::Ended,
            PlayerState::Error(message) => MediaStatus::Error(message),
        };
    }

    /// Alterna play/pause. No fim do stream, `play` recomeça do início.
    pub fn toggle(&mut self) -> Result<(), String> {
        if self.status == MediaStatus::Playing {
            self.pause()
        } else {
            self.play()
        }
    }

    pub fn play(&mut self) -> Result<(), String> {
        let result = match self.player.as_ref() {
            Some(player) => player.play().map_err(|error| error.to_string()),
            None => Err("Nenhum trailer carregado.".to_string()),
        };
        if let Err(message) = &result {
            self.status = MediaStatus::Error(message.clone());
        }
        result
    }

    pub fn pause(&mut self) -> Result<(), String> {
        let result = match self.player.as_ref() {
            Some(player) => player.pause().map_err(|error| error.to_string()),
            None => Err("Nenhum trailer carregado.".to_string()),
        };
        if let Err(message) = &result {
            self.status = MediaStatus::Error(message.clone());
        }
        result
    }

    /// Seek absoluto em segundos, limitado à duração quando conhecida.
    pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
        let seconds = seconds.max(0.0);
        let seconds = match self.duration {
            Some(duration) if seconds > duration.as_secs_f64() => duration.as_secs_f64(),
            _ => seconds,
        };
        let target = Duration::from_secs_f64(seconds);

        let result = match self.player.as_ref() {
            Some(player) => player.seek(target).map_err(|error| error.to_string()),
            None => Err("Nenhum trailer carregado.".to_string()),
        };
        match &result {
            Ok(()) => self.position = Some(target),
            Err(message) => self.status = MediaStatus::Error(message.clone()),
        }
        result
    }

    /// Para e descarta o pipeline. Chamado ao trocar de jogo ou voltar para a
    /// biblioteca para não deixar decode/threads do GStreamer em segundo plano.
    pub fn clear(&mut self) {
        if let Some(mut player) = self.player.take() {
            let _ = player.stop();
        }
        self.texture = None;
        self.status = MediaStatus::Idle;
        self.duration = None;
        self.position = None;
        self.uri = None;
        self.opened_at = None;
        self.first_frame_ms = None;
    }

    /// Registra um erro vindo de uma mensagem da UI (`MediaError`).
    pub fn report_error(&mut self, message: impl Into<String>) {
        self.status = MediaStatus::Error(message.into());
    }

    pub fn status(&self) -> &MediaStatus {
        &self.status
    }

    pub fn texture(&self) -> Option<&Handle> {
        self.texture.as_ref()
    }

    pub fn is_playing(&self) -> bool {
        self.status == MediaStatus::Playing
    }

    /// `true` quando o play/pause tem um pipeline por trás.
    pub fn can_control(&self) -> bool {
        self.player.is_some() && !self.status.is_error()
    }

    pub fn uri(&self) -> Option<&str> {
        self.uri.as_deref()
    }

    /// Latência entre `open` e o primeiro frame decodificado.
    pub fn first_frame_ms(&self) -> Option<u128> {
        self.first_frame_ms
    }

    pub fn duration_seconds(&self) -> f64 {
        self.duration
            .map(|value| value.as_secs_f64())
            .unwrap_or(0.0)
    }

    pub fn position_seconds(&self) -> f64 {
        let position = self
            .position
            .map(|value| value.as_secs_f64())
            .unwrap_or(0.0);
        let duration = self.duration_seconds();
        if duration > 0.0 {
            position.clamp(0.0, duration)
        } else {
            position.max(0.0)
        }
    }
}

/// `mm:ss` para o rótulo de tempo do trailer.
pub fn format_seconds(value: f64) -> String {
    let total = value.max(0.0).round() as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_respect_hls_mp4_webm_order() {
        let all = TrailerSources {
            hls: Some("https://cdn.test/trailer.m3u8".to_string()),
            mp4: Some("https://cdn.test/trailer.mp4".to_string()),
            webm: Some("https://cdn.test/trailer.webm".to_string()),
        };
        assert_eq!(
            all.candidates(),
            vec![
                "https://cdn.test/trailer.m3u8".to_string(),
                "https://cdn.test/trailer.mp4".to_string(),
                "https://cdn.test/trailer.webm".to_string()
            ]
        );

        let sem_hls = TrailerSources {
            hls: Some("   ".to_string()),
            mp4: Some("https://cdn.test/trailer.mp4".to_string()),
            webm: Some("https://cdn.test/trailer.webm".to_string()),
        };
        assert_eq!(
            sem_hls.candidates(),
            vec![
                "https://cdn.test/trailer.mp4".to_string(),
                "https://cdn.test/trailer.webm".to_string()
            ]
        );

        assert!(TrailerSources::default().candidates().is_empty());
    }

    #[test]
    fn candidates_keep_try_order_and_drop_blank_entries() {
        let sources = TrailerSources {
            hls: Some(" https://cdn.test/a.m3u8 ".to_string()),
            mp4: Some(String::new()),
            webm: Some("https://cdn.test/c.webm".to_string()),
        };
        assert_eq!(
            sources.candidates(),
            vec![
                "https://cdn.test/a.m3u8".to_string(),
                "https://cdn.test/c.webm".to_string()
            ]
        );
    }

    #[test]
    fn formats_trailer_time_as_mm_ss() {
        assert_eq!(format_seconds(-1.0), "00:00");
        assert_eq!(format_seconds(0.0), "00:00");
        assert_eq!(format_seconds(9.4), "00:09");
        assert_eq!(format_seconds(65.0), "01:05");
        assert_eq!(format_seconds(3600.0), "60:00");
    }

    #[test]
    fn status_labels_are_explicit_for_loading_and_errors() {
        assert_eq!(MediaStatus::Loading.label(), "Carregando trailer…");
        assert_eq!(MediaStatus::Playing.label(), "Tocando (mudo)");
        assert_eq!(
            MediaStatus::Error("openh264dec ausente".to_string()).label(),
            "openh264dec ausente"
        );
        assert!(MediaStatus::Error("x".to_string()).is_error());
        assert!(!MediaStatus::Paused.is_error());
    }

    #[test]
    fn empty_sources_report_an_error_without_creating_a_pipeline() {
        let mut media = MediaPlayer::new();
        let error = media.open(&TrailerSources::default()).unwrap_err();
        assert!(error.contains("hls/mp4/webm"), "veio: {error}");
        assert!(media.status().is_error());
        assert!(!media.is_active());
    }

    #[test]
    fn empty_local_trailer_path_fails_before_opening_a_pipeline() {
        let mut media = MediaPlayer::new();
        let error = media.open_local("   ").unwrap_err();
        assert!(error.contains("sem caminho"), "veio: {error}");
        assert!(media.status().is_error());
        assert!(!media.is_active());
    }
}
