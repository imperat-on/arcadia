//! Transição local da imagem principal; eventos continuam no painel original.
use iced::advanced::{
    layout, mouse, overlay, renderer,
    widget::{
        tree::{self, Tree},
        Operation,
    },
    Clipboard, Layout, Shell, Widget,
};
use iced::widget::image::Handle;
use iced::{Element, Event, Length, Rectangle, Size, Vector};
use std::time::{Duration, Instant};

const DURATION: Duration = Duration::from_millis(220);
const FRAME: Duration = Duration::from_millis(16);

struct State {
    game: String,
    current: Option<Handle>,
    previous: Option<Handle>,
    started: Option<Instant>,
}

pub(in crate::views) fn wrap<'a, Message: 'a>(
    content: Element<'a, Message>,
    game: String,
    current: Option<Handle>,
) -> Element<'a, Message> {
    Element::new(Gallery {
        content,
        game,
        current,
        reduced: std::env::var("ARCADIA_REDUCE_MOTION").as_deref() == Ok("1"),
    })
}

struct Gallery<'a, Message> {
    content: Element<'a, Message>,
    game: String,
    current: Option<Handle>,
    reduced: bool,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Gallery<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State {
            game: self.game.clone(),
            current: self.current.clone(),
            previous: None,
            started: None,
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.game != self.game {
            state.game = self.game.clone();
            state.current = self.current.clone();
            state.previous = None;
            state.started = None;
        } else if state.current != self.current {
            // Não esconda o player nem a mensagem de carregamento.
            state.previous = if !self.reduced && self.current.is_some() {
                state.current.clone()
            } else {
                None
            };
            state.current = self.current.clone();
            state.started = state.previous.as_ref().map(|_| Instant::now());
        }
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
        let now = match event {
            Event::Window(iced::window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        if let Some(started) = state.started {
            if now.saturating_duration_since(started) < DURATION {
                shell.request_redraw_at(now + FRAME);
            } else {
                state.started = None;
                state.previous = None;
            }
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        let state = tree.state.downcast_ref::<State>();
        if let (Some(previous), Some(started)) = (&state.previous, state.started) {
            use iced::advanced::image::Renderer as _;
            let elapsed = Instant::now()
                .saturating_duration_since(started)
                .as_secs_f32();
            let alpha = (1.0 - elapsed / DURATION.as_secs_f32()).clamp(0.0, 1.0);
            let bounds = layout.bounds();
            let pad = crate::theme::SPACE_4;
            let clip = Rectangle {
                x: bounds.x + pad,
                y: bounds.y + pad,
                width: (bounds.width - pad * 2.0).max(0.0),
                height: 340.0,
            };
            if let Some(dimensions) = renderer
                .measure_image(previous)
                .filter(|size| size.width > 0 && size.height > 0 && clip.width > 0.0)
            {
                let scale = (clip.width / dimensions.width as f32)
                    .min(clip.height / dimensions.height as f32);
                let width = dimensions.width as f32 * scale;
                let height = dimensions.height as f32 * scale;
                let rect = Rectangle {
                    x: clip.x + (clip.width - width) / 2.0,
                    y: clip.y + (clip.height - height) / 2.0,
                    width,
                    height,
                };
                renderer.draw_image(
                    iced::advanced::image::Image::new(previous.clone()).opacity(alpha),
                    rect,
                    clip,
                );
            }
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
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
