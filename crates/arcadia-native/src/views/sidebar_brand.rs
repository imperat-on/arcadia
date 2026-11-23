//! Wordmark da barra lateral, com um raio atravessando as letras.
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use iced::widget::canvas::{self, Frame, Geometry};
use iced::widget::image::Handle;
use iced::widget::Action;
use iced::{mouse, window, Rectangle};

use crate::app::Message;

const FRAME: Duration = Duration::from_millis(42);
const FRAME_COUNT: usize = 32;
const CYCLE: Duration = Duration::from_millis(2400);
const LOGO_WIDTH: f32 = 125.0;
const LOGO_HEIGHT: f32 = 22.0;

pub(super) struct Brand {
    pub reduced_motion: bool,
}

pub(super) struct State {
    started: Instant,
}

impl Default for State {
    fn default() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

// Raio quebrado, limitado aos pixels existentes da marca.
fn lightning(x: f32, y: f32, position: f32) -> f32 {
    let jagged = if y < 0.34 {
        -0.035
    } else if y < 0.65 {
        0.025
    } else {
        -0.015
    };
    let distance = (x - position - jagged).abs();
    (1.0 - distance / 0.065).clamp(0.0, 1.0).powi(2)
}

fn frames() -> &'static [Handle] {
    static FRAMES: OnceLock<Vec<Handle>> = OnceLock::new();
    FRAMES.get_or_init(|| {
        let source =
            image::load_from_memory(include_bytes!("../../assets/icons/arcadia-wordmark.png"))
                .expect("wordmark PNG empacotado inválido")
                .resize_exact(250, 44, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
        let (width, height) = source.dimensions();
        (0..FRAME_COUNT)
            .map(|frame| {
                let position = -0.1 + 1.2 * frame as f32 / (FRAME_COUNT - 1) as f32;
                let mut pixels = source.clone();
                for (x, y, pixel) in pixels.enumerate_pixels_mut() {
                    if pixel[3] == 0 {
                        continue;
                    }
                    let amount =
                        lightning(x as f32 / width as f32, y as f32 / height as f32, position);
                    for (channel, target) in [190.0, 248.0, 255.0].into_iter().enumerate() {
                        pixel[channel] =
                            (pixel[channel] as f32 * (1.0 - amount) + target * amount) as u8;
                    }
                }
                Handle::from_rgba(width, height, pixels.into_raw())
            })
            .collect()
    })
}

impl canvas::Program<Message> for Brand {
    type State = State;

    fn update(
        &self,
        _state: &mut State,
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        if self.reduced_motion {
            return None;
        }
        let now = match event {
            canvas::Event::Window(window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        Some(Action::request_redraw_at(now + FRAME))
    }

    fn draw(
        &self,
        state: &State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let phase = state.started.elapsed().as_secs_f32() % CYCLE.as_secs_f32();
        let index = if self.reduced_motion {
            0
        } else {
            ((phase / CYCLE.as_secs_f32()) * FRAME_COUNT as f32) as usize % FRAME_COUNT
        };
        frame.draw_image(
            Rectangle {
                x: (bounds.width - LOGO_WIDTH) / 2.0,
                y: (bounds.height - LOGO_HEIGHT) / 2.0,
                width: LOGO_WIDTH,
                height: LOGO_HEIGHT,
            },
            iced::advanced::image::Image::new(frames()[index].clone()),
        );
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lightning_moves_and_stays_bounded() {
        assert!(lightning(0.525, 0.5, 0.5) > 0.99);
        assert_eq!(lightning(0.9, 0.5, 0.5), 0.0);
        assert!(lightning(0.525, 0.5, 0.6) < lightning(0.525, 0.5, 0.5));
    }

    #[test]
    fn embedded_wordmark_builds_all_frames() {
        assert_eq!(frames().len(), FRAME_COUNT);
    }
}
