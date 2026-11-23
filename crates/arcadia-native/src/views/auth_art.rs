//! Brilho de fundo e ícones de linha da autenticação.
use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::{mouse, Color, Point, Rectangle, Size, Vector};

use crate::app::Message;
use crate::theme;

pub(super) struct Backdrop;

impl canvas::Program<Message> for Backdrop {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        for (x, y, width, height) in [(0.47, 0.21, 0.37, 0.48), (0.60, 0.88, 0.25, 0.31)] {
            frame.with_save(|frame| {
                frame.translate(Vector::new(bounds.width * x, bounds.height * y));
                frame.scale_nonuniform(Vector::new(bounds.width * width, bounds.height * height));
                for step in (1..=40).rev() {
                    frame.fill(
                        &Path::circle(Point::ORIGIN, step as f32 / 40.0),
                        theme::AUTH_GLOW,
                    );
                }
            });
        }
        vec![frame.into_geometry()]
    }
}

#[derive(Clone, Copy)]
pub(super) enum Glyph {
    Mail,
    User,
    Lock,
    Arrow,
    Close,
}

pub(super) struct Icon {
    pub glyph: Glyph,
    pub color: Color,
}

impl canvas::Program<Message> for Icon {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let unit = bounds.width.min(bounds.height) / 24.0;
        let at = |x: f32, y: f32| Point::new(x * unit, y * unit);
        let stroke = Stroke::default()
            .with_width(1.8 * unit)
            .with_color(self.color)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round);
        let path = Path::new(|p| match self.glyph {
            Glyph::Mail => {
                p.rounded_rectangle(
                    at(3.0, 5.0),
                    Size::new(18.0 * unit, 14.0 * unit),
                    (2.0 * unit).into(),
                );
                p.move_to(at(3.5, 7.0));
                p.line_to(at(12.0, 13.0));
                p.line_to(at(20.5, 7.0));
            }
            Glyph::User => {
                p.circle(at(12.0, 8.0), 4.0 * unit);
                p.move_to(at(4.0, 21.0));
                p.quadratic_curve_to(at(12.0, 11.5), at(20.0, 21.0));
            }
            Glyph::Lock => {
                p.rounded_rectangle(
                    at(5.0, 11.0),
                    Size::new(14.0 * unit, 9.0 * unit),
                    (2.0 * unit).into(),
                );
                p.move_to(at(8.0, 11.0));
                p.line_to(at(8.0, 8.0));
                p.quadratic_curve_to(at(8.0, 3.0), at(12.0, 3.0));
                p.quadratic_curve_to(at(16.0, 3.0), at(16.0, 8.0));
                p.line_to(at(16.0, 11.0));
            }
            Glyph::Arrow => {
                p.move_to(at(5.0, 12.0));
                p.line_to(at(19.0, 12.0));
                p.move_to(at(13.0, 6.0));
                p.line_to(at(19.0, 12.0));
                p.line_to(at(13.0, 18.0));
            }
            Glyph::Close => {
                p.move_to(at(5.0, 5.0));
                p.line_to(at(19.0, 19.0));
                p.move_to(at(19.0, 5.0));
                p.line_to(at(5.0, 19.0));
            }
        });
        frame.stroke(&path, stroke);
        vec![frame.into_geometry()]
    }
}
