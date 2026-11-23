//! Animações locais aos overlays e downloads, sem subscription global.
use crate::app::Message;
use crate::theme;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path};
use iced::widget::Action;
use iced::{mouse, window, Color, Element, Length, Point, Rectangle, Size};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const FRAME: Duration = Duration::from_millis(33);
const TRANSITION: Duration = Duration::from_millis(250);

pub(super) fn motion_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("ARCADIA_REDUCE_MOTION").as_deref() != Ok("1"))
}

#[derive(Clone, Copy)]
pub(in crate::views) enum Motion {
    Progress(f32),
    Skeleton,
    Veil(bool),
}

pub(in crate::views) fn canvas(
    motion: Motion,
    width: impl Into<Length>,
    height: impl Into<Length>,
) -> Element<'static, Message> {
    Canvas::new(motion).width(width).height(height).into()
}

pub(in crate::views) struct State {
    started: Instant,
    from: f32,
    target: Option<f32>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            from: 0.0,
            target: None,
        }
    }
}
impl State {
    fn value(&self, now: Instant) -> f32 {
        let target = self.target.unwrap_or(0.0);
        let t = (now.saturating_duration_since(self.started).as_secs_f32()
            / TRANSITION.as_secs_f32())
        .clamp(0.0, 1.0);
        self.from + (target - self.from) * (1.0 - (1.0 - t).powi(2))
    }
}
impl canvas::Program<Message> for Motion {
    type State = State;
    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        if !motion_enabled() || matches!(self, Motion::Veil(true)) {
            return None;
        }
        let now = match event {
            canvas::Event::Window(window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        match *self {
            Motion::Progress(percent) => {
                let target = if percent.is_finite() {
                    percent.clamp(0.0, 100.0)
                } else {
                    0.0
                };
                if state.target.is_none() {
                    state.from = target;
                    state.target = Some(target);
                } else if state.target != Some(target) {
                    state.from = state.value(now);
                    state.target = Some(target);
                    state.started = now;
                }
                if now.saturating_duration_since(state.started) < TRANSITION && state.from != target
                {
                    return Some(Action::request_redraw_at(now + FRAME));
                }
            }
            Motion::Skeleton => return Some(Action::request_redraw_at(now + FRAME)),
            Motion::Veil(_) if now.saturating_duration_since(state.started) < TRANSITION => {
                return Some(Action::request_redraw_at(now + FRAME))
            }
            Motion::Veil(_) => {}
        }
        None
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
        let enabled = motion_enabled();
        let elapsed = state.started.elapsed().as_secs_f32();
        match *self {
            Motion::Progress(percent) => {
                let percent = if percent.is_finite() {
                    percent.clamp(0.0, 100.0)
                } else {
                    0.0
                };
                let value = if enabled && state.target.is_some() {
                    state.value(Instant::now())
                } else {
                    percent
                };
                frame.fill(
                    &Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 4.0.into()),
                    Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                );
                let width = bounds.width * value / 100.0;
                if width > 0.0 {
                    frame.fill(
                        &Path::rounded_rectangle(
                            Point::ORIGIN,
                            Size::new(width, bounds.height),
                            (width.min(bounds.height) / 2.0).into(),
                        ),
                        theme::ACCENT,
                    );
                }
            }
            Motion::Skeleton => {
                let alpha = if enabled {
                    0.08 + 0.06 * (0.5 + 0.5 * (elapsed * 3.0).sin())
                } else {
                    0.11
                };
                frame.fill(
                    &Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 4.0.into()),
                    Color::from_rgba(1.0, 1.0, 1.0, alpha),
                );
            }
            Motion::Veil(reduced) => {
                let t = if enabled && !reduced {
                    (elapsed / TRANSITION.as_secs_f32()).min(1.0)
                } else {
                    1.0
                };
                frame.fill_rectangle(
                    Point::ORIGIN,
                    bounds.size(),
                    Color::from_rgba(0.0, 0.0, 0.0, 0.72 * (1.0 - (1.0 - t).powi(2))),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_interpolates_and_finishes_at_target() {
        let now = Instant::now();
        let state = State {
            started: now,
            from: 20.0,
            target: Some(80.0),
        };
        assert_eq!(state.value(now), 20.0);
        assert!(state.value(now + TRANSITION / 2) > 20.0);
        assert_eq!(state.value(now + TRANSITION), 80.0);
    }
}
