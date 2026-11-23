//! Ícones de linha do shell, extraídos do set autoral da `Sidebar.tsx`.
//!
//! São PNGs brancos (24 px de grade, traço 1.6) rasterizados em 4×. O iced
//! aplica a opacidade do estado por cima, então um arquivo só serve para os
//! estados normal/hover/ativo sem precisar de SVG ou de uma dependência nova.

use std::sync::OnceLock;

use iced::widget::image::{FilterMethod, Handle};
use iced::widget::Image;
use iced::Element;

use crate::app::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Home,
    Grid,
    Store,
    Plugin,
    Download,
    Link,
    Users,
    Gear,
    Monitor,
    Search,
    Refresh,
    ChevronDown,
    Heart,
    Gamepad,
}

impl Icon {
    fn handle(self) -> &'static Handle {
        macro_rules! load {
            ($bytes:expr) => {{
                static HANDLE: OnceLock<Handle> = OnceLock::new();
                HANDLE.get_or_init(|| Handle::from_bytes($bytes.as_slice()))
            }};
        }

        match self {
            Icon::Home => load!(include_bytes!("../../assets/icons/home.png")),
            Icon::Grid => load!(include_bytes!("../../assets/icons/grid.png")),
            Icon::Store => load!(include_bytes!("../../assets/icons/store.png")),
            Icon::Plugin => load!(include_bytes!("../../assets/icons/plugin.png")),
            Icon::Download => load!(include_bytes!("../../assets/icons/download.png")),
            Icon::Link => load!(include_bytes!("../../assets/icons/link.png")),
            Icon::Users => load!(include_bytes!("../../assets/icons/users.png")),
            Icon::Gear => load!(include_bytes!("../../assets/icons/gear.png")),
            Icon::Monitor => load!(include_bytes!("../../assets/icons/monitor.png")),
            Icon::Search => load!(include_bytes!("../../assets/icons/search.png")),
            Icon::Refresh => load!(include_bytes!("../../assets/icons/refresh.png")),
            Icon::ChevronDown => load!(include_bytes!("../../assets/icons/chevron-down.png")),
            Icon::Heart => load!(include_bytes!("../../assets/icons/heart.png")),
            Icon::Gamepad => load!(include_bytes!("../../assets/icons/gamepad.png")),
        }
    }
}

/// Ícone quadrado pronto para entrar numa linha. `opacity` mapeia o
/// `currentColor`/opacidade do CSS (0.0–1.0).
pub fn icon(icon: Icon, size: f32, opacity: f32) -> Element<'static, Message> {
    Image::new(icon.handle())
        .width(size)
        .height(size)
        .filter_method(FilterMethod::Linear)
        .opacity(opacity)
        .into()
}

/// Mesmo ícone, mas dentro de um botão quadrado com hover de vidro.
pub fn icon_button(
    icon_value: Icon,
    size: f32,
    padding: f32,
    message: Message,
) -> Element<'static, Message> {
    use iced::widget::button;

    button(icon(icon_value, size, 0.55))
        .on_press(message)
        .padding(padding)
        .style(crate::theme::icon_button)
        .into()
}
