//! Chrome da janela frameless: faixa superior arrastável e os três controles
//! no estilo macOS usados pelo shell web (`WindowControls.tsx`).

use iced::widget::{button, container, mouse_area, row, Space};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::theme;

pub fn top_bar() -> Element<'static, Message> {
    let controls = row![
        dot(
            theme::CONTROL_CLOSE,
            theme::CONTROL_CLOSE_RING,
            Message::WindowClose,
        ),
        dot(
            theme::CONTROL_MINIMIZE,
            theme::CONTROL_MINIMIZE_RING,
            Message::WindowMinimize,
        ),
        dot(
            theme::CONTROL_MAXIMIZE,
            theme::CONTROL_MAXIMIZE_RING,
            Message::WindowToggleMaximize,
        ),
    ]
    .spacing(8.0)
    .align_y(iced::Center);

    let drag_left = mouse_area(
        container(controls)
            .width(Fill)
            .height(Fill)
            .padding([0.0, 14.0])
            .align_y(iced::Center)
            .style(theme::sidebar_top),
    )
    .on_press(Message::WindowDrag);

    let drag_right = mouse_area(
        container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(theme::top_bar),
    )
    .on_press(Message::WindowDrag)
    .on_double_click(Message::WindowToggleMaximize);

    row![
        container(drag_left)
            .width(theme::SIDEBAR_WIDTH)
            .height(theme::TOPBAR_HEIGHT),
        container(drag_right)
            .width(Fill)
            .height(theme::TOPBAR_HEIGHT),
    ]
    .width(Fill)
    .height(theme::TOPBAR_HEIGHT)
    .into()
}

fn dot(color: iced::Color, ring: iced::Color, message: Message) -> Element<'static, Message> {
    button(Space::new().width(12).height(12))
        .on_press(message)
        .width(Length::Fixed(12.0))
        .height(Length::Fixed(12.0))
        .padding(0)
        .style(move |_theme, status| theme::window_dot(color, ring, status))
        .into()
}
