//! Escala de entrada local, preservando eventos e operações do widget filho.
use std::time::{Duration, Instant};

use iced::advanced::{
    layout, mouse, overlay, renderer,
    widget::{
        tree::{self, Tree},
        Operation,
    },
    Clipboard, Layout, Shell, Widget,
};
use iced::{Element, Event, Length, Rectangle, Size, Transformation, Vector};

const DURATION: Duration = Duration::from_millis(180);

struct State {
    started: Instant,
}
impl Default for State {
    fn default() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}
fn eased(elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / DURATION.as_secs_f32()).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}
pub(super) fn panel<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    reduced: bool,
) -> Element<'a, Message> {
    Element::new(Entrance {
        content: content.into(),
        reduced,
    })
}
struct Entrance<'a, Message> {
    content: Element<'a, Message>,
    reduced: bool,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Entrance<'_, Message> {
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
        if !self.reduced && state.started.elapsed() < DURATION {
            shell.request_redraw_at(Instant::now() + Duration::from_millis(16));
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
        let progress = if self.reduced {
            1.0
        } else {
            eased(state.started.elapsed())
        };
        let bounds = layout.bounds();
        let scale = 0.96 + 0.04 * progress;
        let transform = Transformation::translate(bounds.center_x(), bounds.center_y())
            * Transformation::scale(scale)
            * Transformation::translate(-bounds.center_x(), -bounds.center_y());
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
    fn entrance_interpolation_is_bounded() {
        assert_eq!(eased(Duration::ZERO), 0.0);
        assert!(eased(DURATION / 2) > 0.0);
        assert_eq!(eased(DURATION), 1.0);
        assert_eq!(eased(DURATION * 2), 1.0);
    }
}
