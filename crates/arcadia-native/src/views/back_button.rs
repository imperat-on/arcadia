//! Botão "voltar" desenhado em canvas.
//!
//! O `‹` de fonte tem *bearing* assimétrico: mesmo centralizado no layout o
//! traço parece colado à esquerda. Aqui o chevron é geometria pura — dois
//! segmentos com ponta arredondada, calculados dentro da caixa — então ele
//! fica perfeitamente centrado em qualquer tamanho e nítido em qualquer DPI.
//!
//! Dois formatos:
//! * [`circle`] — o botão flutuante de vidro (ficha do jogo/loja).
//! * [`chevron`] / [`labeled`] — o desenho e a pill "‹ Voltar".

use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::widget::{button, canvas as canvas_widget, row, text};
use iced::{
    mouse, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme,
};

use crate::theme;

/// Diâmetro do círculo flutuante.
const DIAMETER: f32 = 40.0;
/// Espessura do traço do chevron.
const STROKE: f32 = 2.2;

/// Só o desenho do chevron, para compor dentro de uma pill com rótulo.
pub fn chevron<'a, Message: 'a>(size: f32) -> Element<'a, Message> {
    let size = size.max(12.0);
    canvas_widget(Chevron { size })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

/// Botão circular de vidro, com chevron centralizado e estados de hover.
pub fn circle<'a, Message: Clone + 'a>(on_press: Option<Message>) -> Element<'a, Message> {
    let mut btn = button(
        canvas_widget(Chevron { size: 18.0 })
            .width(Length::Fixed(DIAMETER))
            .height(Length::Fixed(DIAMETER)),
    )
    .width(Length::Fixed(DIAMETER))
    .height(Length::Fixed(DIAMETER))
    .padding(0)
    .style(theme::back_circle);

    if let Some(message) = on_press {
        btn = btn.on_press(message);
    }

    btn.into()
}

/// Pill "‹ Voltar" com o chevron desenhado e um rótulo ao lado.
pub fn labeled<'a, Message: Clone + 'a>(
    label: impl Into<String>,
    on_press: Message,
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, Message> {
    button(
        row![
            chevron::<Message>(theme::TEXT_HEADING),
            text(label.into())
                .size(theme::TEXT_READ)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .on_press(on_press)
    .padding([8.0, 14.0])
    .style(style)
    .into()
}

// ── Canvas ────────────────────────────────────────────────────────────────

struct Chevron {
    /// Tamanho da caixa do chevron (o traço escala junto).
    size: f32,
}

impl<Message> canvas::Program<Message, Theme, Renderer> for Chevron {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let color = if cursor.is_over(bounds) {
            theme::TEXT_1
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.92)
        };
        draw_chevron(&mut frame, bounds.size(), self.size, color);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Desenha o chevron `‹` centralizado na caixa.
///
/// O traço é deslocado para a direita por meia espessura, para que a *tinta* —
/// não a caixa — fique no centro óptico.
fn draw_chevron(frame: &mut Frame, size: Size, box_size: f32, color: Color) {
    let cx = size.width / 2.0;
    let cy = size.height / 2.0;

    // Proporção do chevron: ~42% da caixa, aberto na vertical.
    let half_width = box_size * 0.21;
    let half_height = box_size * 0.26;
    // Compensação óptica: empurra ~meia espessura para a direita do centro.
    let apex_x = cx - half_width * 0.5 + STROKE * 0.5;

    let top = Point::new(apex_x + half_width, cy - half_height);
    let apex = Point::new(apex_x - half_width * 0.5, cy);
    let bottom = Point::new(apex_x + half_width, cy + half_height);

    let path = Path::new(|builder| {
        builder.move_to(top);
        builder.line_to(apex);
        builder.line_to(bottom);
    });

    frame.stroke(
        &path,
        Stroke::default()
            .with_width(STROKE)
            .with_color(color)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round),
    );
}
