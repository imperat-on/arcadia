//! Tokens visuais do Arcadia nativo, portados de `ui/src/index.css`.
//!
//! Nesta fatia usamos a fonte sans do sistema (`Font::DEFAULT`). Quando a Inter
//! for empacotada, troque apenas as constantes de fonte e carregue o arquivo em
//! `main.rs` com `Application::font(...)`; nenhum token de cor/espaçamento muda.

use iced::border::Radius;
use iced::widget::scrollable as scrollable_widget;
use iced::widget::{button, container, text_input};
use iced::{Border, Color, Font, Shadow, Theme, Vector};

// ── Cores (mesmos valores de `ui/src/index.css`) ────────────────────────────
pub const BACKGROUND: Color = Color::from_rgb8(0x00, 0x00, 0x00);
pub const SIDEBAR_BG: Color = Color::from_rgb8(0x03, 0x06, 0x0a);
pub const SURFACE_1: Color = Color::from_rgb8(0x0d, 0x0d, 0x0f);
pub const SURFACE_2: Color = Color::from_rgb8(0x14, 0x14, 0x19);
pub const SURFACE_3: Color = Color::from_rgb8(0x1c, 0x1c, 0x22);

pub const TEXT_1: Color = Color::from_rgb8(0xff, 0xff, 0xff);
pub const TEXT_2: Color = Color::from_rgb8(0x8a, 0x93, 0xa6);
pub const TEXT_3: Color = Color::from_rgb8(0x6b, 0x72, 0x80);
pub const TEXT_STRONG: Color = Color::from_rgb8(0xc8, 0xd0, 0xe0);

pub const ACCENT: Color = Color::from_rgb8(0x22, 0xd3, 0xee);
pub const ACCENT_SOFT: Color = Color::from_rgba8(0x22, 0xd3, 0xee, 0.16);
pub const ACCENT_BORDER: Color = Color::from_rgba8(0x22, 0xd3, 0xee, 0.34);
pub const SUCCESS: Color = Color::from_rgb8(0x4a, 0xdf, 0x9a);
pub const WARNING: Color = Color::from_rgb8(0xf5, 0xa6, 0x23);
pub const DANGER: Color = Color::from_rgb8(0xff, 0x6b, 0x81);

pub const BORDER: Color = Color::from_rgba8(0xff, 0xff, 0xff, 0.08);
pub const BORDER_STRONG: Color = Color::from_rgba8(0xff, 0xff, 0xff, 0.16);
pub const TRANSPARENT: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.0);

// ── Tipografia ──────────────────────────────────────────────────────────────
pub const FONT: Font = Font::DEFAULT;
pub const FONT_MEDIUM: Font = Font {
    weight: iced::font::Weight::Medium,
    ..Font::DEFAULT
};
pub const FONT_BOLD: Font = Font {
    weight: iced::font::Weight::Bold,
    ..Font::DEFAULT
};

// ── Espaçamento, raios e medidas ────────────────────────────────────────────
pub const SIDEBAR_WIDTH: f32 = 240.0;
pub const PAGE_PADDING: f32 = 28.0;
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_5: f32 = 24.0;
pub const SPACE_6: f32 = 32.0;

pub const CARD_WIDTH: f32 = 180.0;
pub const CARD_GAP: f32 = 16.0;
pub const CARD_RADIUS: f32 = 12.0;
pub const PANEL_RADIUS: f32 = 16.0;
pub const HERO_HEIGHT: f32 = 320.0;

/// Tema do iced. Os widgets usam os estilos explícitos abaixo, mas o tema
/// mantém defaults coerentes para diálogos e overlays futuros.
pub fn arcadia() -> Theme {
    Theme::custom(
        "Arcadia Dark",
        iced::theme::Palette {
            background: BACKGROUND,
            text: TEXT_1,
            primary: ACCENT,
            success: SUCCESS,
            warning: WARNING,
            danger: DANGER,
        },
    )
}

fn rounded_border(color: Color, width: f32, radius: f32) -> Border {
    Border {
        color,
        width,
        radius: radius.into(),
    }
}

// ── Containers ──────────────────────────────────────────────────────────────
pub fn app_background(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(BACKGROUND.into()),
        ..container::Style::default()
    }
}

pub fn sidebar(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_2),
        background: Some(SIDEBAR_BG.into()),
        border: Border {
            color: ACCENT_SOFT,
            width: 1.0,
            radius: Radius::default(),
        },
        ..container::Style::default()
    }
}

pub fn sidebar_brand(_theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: ACCENT_SOFT,
            width: 1.0,
            radius: Radius::default(),
        },
        ..container::Style::default()
    }
}

pub fn panel(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_2),
        background: Some(SURFACE_1.into()),
        border: rounded_border(BORDER, 1.0, PANEL_RADIUS),
        ..container::Style::default()
    }
}

pub fn stub_panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_2.into()),
        border: rounded_border(BORDER, 1.0, CARD_RADIUS),
        ..container::Style::default()
    }
}

pub fn badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_3.into()),
        border: rounded_border(BORDER, 1.0, 6.0),
        ..container::Style::default()
    }
}

pub fn cover_placeholder(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_3.into()),
        border: rounded_border(BORDER, 1.0, CARD_RADIUS),
        ..container::Style::default()
    }
}

pub fn hero_overlay(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.56).into()),
        ..container::Style::default()
    }
}

pub fn accent_bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(ACCENT.into()),
        border: Border::default().rounded(2),
        ..container::Style::default()
    }
}

pub fn transparent_bar(_theme: &Theme) -> container::Style {
    container::Style::default()
}

pub fn error_panel(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_2),
        background: Some(SURFACE_1.into()),
        border: rounded_border(DANGER, 1.0, PANEL_RADIUS),
        ..container::Style::default()
    }
}

// ── Botões ──────────────────────────────────────────────────────────────────
pub fn card(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color) = match status {
        button::Status::Hovered => (SURFACE_3, ACCENT_BORDER),
        button::Status::Pressed => (SURFACE_2, ACCENT),
        button::Status::Disabled => (SURFACE_1, BORDER),
        button::Status::Active => (SURFACE_2, BORDER),
    };

    button::Style {
        background: Some(background.into()),
        text_color: TEXT_1,
        border: rounded_border(border_color, 1.0, CARD_RADIUS),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        snap: true,
    }
}

pub fn sidebar_item(_theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let background = if active {
        Some(SURFACE_2.into())
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => Some(SURFACE_2.into()),
            _ => None,
        }
    };

    button::Style {
        background,
        text_color: if active { TEXT_1 } else { TEXT_2 },
        border: Border::default().rounded(8),
        ..button::Style::default()
    }
}

pub fn game_item(_theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let background = if active {
        Some(SURFACE_2.into())
    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        Some(SURFACE_1.into())
    } else {
        None
    };

    button::Style {
        background,
        text_color: if active { TEXT_1 } else { TEXT_2 },
        border: Border::default().rounded(7),
        ..button::Style::default()
    }
}

pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Color::from_rgb8(0x67, 0xe8, 0xf9),
        button::Status::Pressed => Color::from_rgb8(0x0e, 0xa5, 0xbd),
        button::Status::Disabled => SURFACE_3,
        button::Status::Active => ACCENT,
    };

    button::Style {
        background: Some(background.into()),
        text_color: BACKGROUND,
        border: Border::default().rounded(10),
        ..button::Style::default()
    }
}

pub fn ghost_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Some(SURFACE_2.into()),
        _ => None,
    };

    button::Style {
        background,
        text_color: TEXT_1,
        border: rounded_border(BORDER_STRONG, 1.0, 10.0),
        ..button::Style::default()
    }
}

pub fn big_picture(_theme: &Theme, status: button::Status) -> button::Style {
    let border_color = match status {
        button::Status::Hovered | button::Status::Pressed => ACCENT_BORDER,
        _ => BORDER,
    };

    button::Style {
        background: None,
        text_color: TEXT_2,
        border: rounded_border(border_color, 1.0, 10.0),
        ..button::Style::default()
    }
}

// ── Inputs e scroll ─────────────────────────────────────────────────────────
pub fn search_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(
        status,
        text_input::Status::Focused { .. } | text_input::Status::Hovered
    );

    text_input::Style {
        background: SURFACE_2.into(),
        border: rounded_border(if focused { ACCENT_BORDER } else { BORDER }, 1.0, 10.0),
        icon: TEXT_2,
        placeholder: TEXT_3,
        value: TEXT_1,
        selection: ACCENT_SOFT,
    }
}

pub fn scrollable(theme: &Theme, status: scrollable_widget::Status) -> scrollable_widget::Style {
    let mut style = scrollable_widget::default(theme, status);
    let rail = scrollable_widget::Rail {
        background: Some(TRANSPARENT.into()),
        border: Border::default(),
        scroller: scrollable_widget::Scroller {
            background: SURFACE_3.into(),
            border: Border::default().rounded(3),
        },
    };

    style.vertical_rail = rail;
    style.horizontal_rail = rail;
    style
}
