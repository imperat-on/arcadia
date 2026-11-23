//! Animação local ao widget: não exige mensagens nem subscription no App.
use std::time::{Duration, Instant};

use iced::advanced::{
    layout, mouse, overlay, renderer,
    widget::{
        tree::{self, Tree},
        Operation,
    },
    Clipboard, Layout, Shell, Widget,
};
use iced::{Color, Element, Event, Length, Rectangle, Size, Transformation, Vector};

const DURATION: Duration = Duration::from_millis(180);

#[derive(Default)]
struct State {
    value: f32,
    from: f32,
    target: f32,
    started: Option<Instant>,
    pressed: bool,
}

fn eased(from: f32, target: f32, elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / DURATION.as_secs_f32()).clamp(0.0, 1.0);
    from + (target - from) * (1.0 - (1.0 - t).powi(3))
}

pub(in crate::views) enum Kind {
    Card,
    Navigation,
}

pub(in crate::views) fn wrap<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    kind: Kind,
) -> Element<'a, Message> {
    Element::new(Hover {
        content: content.into(),
        kind,
        reduced: std::env::var("ARCADIA_REDUCE_MOTION").as_deref() == Ok("1"),
    })
}

struct Hover<'a, Message> {
    content: Element<'a, Message>,
    kind: Kind,
    reduced: bool,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Hover<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.content]);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.pressed = cursor.is_over(layout.bounds()) && cursor.is_over(*viewport);
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.pressed = false;
                shell.request_redraw();
            }
            _ => {}
        }
        let target = if cursor.is_over(layout.bounds())
            && layout
                .bounds()
                .intersection(viewport)
                .is_some_and(|r| cursor.is_over(r))
        {
            1.0
        } else {
            0.0
        };
        let now = match event {
            Event::Window(iced::window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        if let Some(started) = state.started {
            state.value = eased(
                state.from,
                state.target,
                now.saturating_duration_since(started),
            );
            if now.saturating_duration_since(started) >= DURATION {
                state.value = state.target;
                state.started = None;
            }
        }
        if target != state.target {
            state.from = state.value;
            state.target = target;
            state.started = if self.reduced {
                state.value = target;
                None
            } else {
                Some(now)
            };
            shell.request_redraw();
        }
        if state.started.is_some() {
            shell.request_redraw();
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let state = tree.state.downcast_ref::<State>();
        let amount = state.value;
        let bounds = layout.bounds();
        let transform = match self.kind {
            Kind::Card => {
                let scale = 1.0 + 0.025 * amount;
                Transformation::translate(bounds.center_x(), bounds.center_y() - 3.0 * amount)
                    * Transformation::scale(scale)
                    * Transformation::translate(-bounds.center_x(), -bounds.center_y())
            }
            Kind::Navigation => {
                let press = if state.pressed && !self.reduced {
                    0.98
                } else {
                    1.0
                };
                Transformation::translate(bounds.center_x() + 2.0 * amount, bounds.center_y())
                    * Transformation::scale(press)
                    * Transformation::translate(-bounds.center_x(), -bounds.center_y())
            }
        };
        renderer.with_transformation(transform, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
            if matches!(self.kind, Kind::Card) && amount > 0.0 {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: iced::Border {
                            radius: 3.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    Color::from_rgba(1.0, 1.0, 1.0, 0.035 * amount),
                );
            }
        });
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interpolation_has_exact_endpoints_and_reverses() {
        assert_eq!(eased(0.0, 1.0, Duration::ZERO), 0.0);
        assert_eq!(eased(0.0, 1.0, DURATION), 1.0);
        assert_eq!(eased(1.0, 0.0, DURATION), 0.0);
        assert!(eased(0.0, 1.0, DURATION / 2) > 0.5);
    }
}
