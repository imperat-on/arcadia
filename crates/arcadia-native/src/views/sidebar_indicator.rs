//! Indicador local da navegação, independente do relógio do aplicativo.
use crate::app::Message;
use crate::theme;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::widget::Action;
use iced::{mouse, window, Color, Point, Rectangle, Size};
use std::time::{Duration, Instant};

const DURATION: Duration = Duration::from_millis(180);
const FRAME: Duration = Duration::from_millis(16);

pub struct Indicator(pub Option<usize>);
#[derive(Default)]
pub struct State {
    current: Option<f32>,
    from: f32,
    target: Option<f32>,
    started: Option<Instant>,
}
fn interpolate(from: f32, to: f32, elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / DURATION.as_secs_f32()).clamp(0.0, 1.0);
    from + (to - from) * (1.0 - (1.0 - t).powi(3))
}
impl canvas::Program<Message> for Indicator {
    type State = State;
    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let now = match event {
            canvas::Event::Window(window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        if let (Some(start), Some(target)) = (state.started, state.target) {
            state.current = Some(interpolate(
                state.from,
                target,
                now.saturating_duration_since(start),
            ));
            if now.saturating_duration_since(start) >= DURATION {
                state.started = None;
            }
        }
        let target = self.0.map(|index| index as f32 * 40.0 + 9.0);
        if state.target != target {
            let reduced = std::env::var("ARCADIA_REDUCE_MOTION").as_deref() == Ok("1");
            state.started = if !reduced && state.current.is_some() && target.is_some() {
                Some(now)
            } else {
                None
            };
            state.from = state.current.unwrap_or(target.unwrap_or(0.0));
            state.target = target;
            if state.started.is_none() {
                state.current = target;
            }
        }
        state
            .started
            .map(|_| Action::request_redraw_at(now + FRAME))
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
        if self.0.is_some() {
            let y = state
                .current
                .unwrap_or(self.0.unwrap_or_default() as f32 * 40.0 + 9.0);
            frame.fill(
                &Path::rounded_rectangle(Point::new(12.0, y), Size::new(2.0, 20.0), 1.0.into()),
                Color { ..theme::ACCENT },
            );
        }
        vec![frame.into_geometry()]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indicator_moves_and_reverses() {
        assert_eq!(interpolate(9.0, 49.0, Duration::ZERO), 9.0);
        assert_eq!(interpolate(9.0, 49.0, DURATION), 49.0);
        assert!(interpolate(49.0, 9.0, DURATION / 2) < 49.0);
    }
}
