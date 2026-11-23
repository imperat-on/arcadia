//! Locally animated, clipped settings disclosure; children stay mounted.
use iced::advanced::{
    layout, mouse, overlay, renderer,
    widget::{
        tree::{self, Tree},
        Operation,
    },
    Clipboard, Layout, Shell, Widget,
};
use iced::{Element, Event, Length, Rectangle, Size, Vector};
use std::time::{Duration, Instant};

const DURATION: Duration = Duration::from_millis(180);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_opens_closes_and_reverses_without_jumping() {
        assert_eq!(interpolate(0.0, true, Duration::ZERO), 0.0);
        assert_eq!(interpolate(0.0, true, DURATION), 1.0);
        assert_eq!(interpolate(1.0, false, DURATION), 0.0);
        let halfway = interpolate(0.0, true, DURATION / 2);
        assert!(halfway > 0.0 && halfway < 1.0);
        assert_eq!(interpolate(halfway, false, Duration::ZERO), halfway);
        assert!(interpolate(halfway, false, DURATION / 2) < halfway);
    }
}

fn interpolate(from: f32, open: bool, elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / DURATION.as_secs_f32()).clamp(0.0, 1.0);
    let end = if open { 1.0 } else { 0.0 };
    from + (end - from) * (1.0 - (1.0 - t).powi(3))
}

struct State {
    value: f32,
    from: f32,
    target: bool,
    started: Option<Instant>,
}

pub(super) fn wrap<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    open: bool,
) -> Element<'a, Message> {
    Element::new(Expand {
        content: content.into(),
        open,
        reduced: std::env::var("ARCADIA_REDUCE_MOTION").as_deref() == Ok("1"),
    })
}

struct Expand<'a, Message> {
    content: Element<'a, Message>,
    open: bool,
    reduced: bool,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Expand<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State {
            value: if self.open { 1.0 } else { 0.0 },
            from: 0.0,
            target: self.open,
            started: None,
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.content]);
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let child = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let size = child.size();
        let value = tree.state.downcast_ref::<State>().value;
        layout::Node::with_children(Size::new(size.width, size.height * value), vec![child])
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        if self.open && tree.state.downcast_ref::<State>().value >= 1.0 {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout.children().next().unwrap(),
                renderer,
                operation,
            );
        }
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
        let now = match event {
            Event::Window(iced::window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        if let Some(started) = state.started {
            let t = (now.saturating_duration_since(started).as_secs_f32() / DURATION.as_secs_f32())
                .clamp(0.0, 1.0);
            let end = if state.target { 1.0 } else { 0.0 };
            state.value = interpolate(
                state.from,
                state.target,
                now.saturating_duration_since(started),
            );
            if t >= 1.0 {
                state.value = end;
                state.started = None;
            }
            shell.invalidate_layout();
        }
        if state.target != self.open {
            state.from = state.value;
            state.target = self.open;
            if self.reduced {
                state.value = if self.open { 1.0 } else { 0.0 };
                state.started = None;
            } else {
                state.started = Some(now);
            }
            shell.invalidate_layout();
            shell.request_redraw();
        }
        if state.started.is_some() {
            shell.request_redraw();
        }
        if self.open && state.value >= 1.0 {
            if let Some(visible) = layout.bounds().intersection(viewport) {
                self.content.as_widget_mut().update(
                    &mut tree.children[0],
                    event,
                    layout.children().next().unwrap(),
                    cursor,
                    renderer,
                    clipboard,
                    shell,
                    &visible,
                );
            }
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.open && tree.state.downcast_ref::<State>().value >= 1.0 {
            if let Some(visible) = layout.bounds().intersection(viewport) {
                return self.content.as_widget().mouse_interaction(
                    &tree.children[0],
                    layout.children().next().unwrap(),
                    cursor,
                    &visible,
                    renderer,
                );
            }
        }
        mouse::Interaction::None
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
        if let Some(visible) = layout.bounds().intersection(viewport) {
            renderer.with_layer(visible, |renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    layout.children().next().unwrap(),
                    cursor,
                    &visible,
                )
            });
        }
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        if self.open && tree.state.downcast_ref::<State>().value >= 1.0 {
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                layout.children().next().unwrap(),
                renderer,
                viewport,
                translation,
            )
        } else {
            None
        }
    }
}
