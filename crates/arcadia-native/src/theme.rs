//! Tokens visuais do Arcadia nativo, portados de `ui/src/index.css`.
//!
//! Os valores batem com os tokens do shell desktop: preto OLED, superfícies
//! #0d0d0f/#141419/#1c1c22, acento ciano #22d3ee e a geometria curta do
//! `.arcadia-desktop-retro` (raios de 3 px, sidebar de 212 px). A fonte é a
//! Inter empacotada em `assets/fonts` e carregada no `main.rs`.

use iced::border::Radius;
use iced::font::{Family, Stretch, Style as FontStyle, Weight};
use iced::widget::overlay::menu as menu_widget;
use iced::widget::scrollable as scrollable_widget;
use iced::widget::{button, container, pick_list as pick_list_widget, text_input};
use iced::{Border, Color, Font, Shadow, Theme, Vector};

// ── Cores (mesmos valores de `ui/src/index.css`) ────────────────────────────
pub const BACKGROUND: Color = Color::from_rgb8(0x00, 0x00, 0x00);
pub const SIDEBAR_BG: Color = Color::from_rgb8(0x03, 0x06, 0x0a);
pub const SURFACE_1: Color = Color::from_rgb8(0x0d, 0x0d, 0x0f);
pub const SURFACE_2: Color = Color::from_rgb8(0x14, 0x14, 0x19);
pub const SURFACE_3: Color = Color::from_rgb8(0x1c, 0x1c, 0x22);
pub const CARD_BG: Color = Color::from_rgb8(0x08, 0x0b, 0x0d);

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
pub const CARD_BORDER: Color = Color::from_rgba8(0xff, 0xff, 0xff, 0.07);
pub const CARD_BORDER_HOVER: Color = Color::from_rgba8(0xff, 0xff, 0xff, 0.25);
pub const TRANSPARENT: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.0);

/// `color-mix(in srgb, var(--accent) 17%, transparent)` da sidebar.
pub const SIDEBAR_LINE: Color = Color::from_rgba8(0x22, 0xd3, 0xee, 0.17);
/// `color-mix(in srgb, var(--accent) 15%, transparent)` da top bar.
pub const TOPBAR_LINE: Color = Color::from_rgba8(0x22, 0xd3, 0xee, 0.15);
pub const TOPBAR_BG: Color = Color::from_rgb8(0x03, 0x06, 0x09);

pub const CONTROL_CLOSE: Color = Color::from_rgb8(0xff, 0x5f, 0x57);
pub const CONTROL_CLOSE_RING: Color = Color::from_rgb8(0xe0, 0x44, 0x3e);
pub const CONTROL_MINIMIZE: Color = Color::from_rgb8(0xfe, 0xbc, 0x2e);
pub const CONTROL_MINIMIZE_RING: Color = Color::from_rgb8(0xde, 0xa1, 0x23);
pub const CONTROL_MAXIMIZE: Color = Color::from_rgb8(0x28, 0xc8, 0x40);
pub const CONTROL_MAXIMIZE_RING: Color = Color::from_rgb8(0x1a, 0xab, 0x29);

// ── Tipografia ──────────────────────────────────────────────────────────────
const INTER: Family = Family::Name("Inter");

pub const FONT: Font = Font {
    family: INTER,
    weight: Weight::Normal,
    stretch: Stretch::Normal,
    style: FontStyle::Normal,
};
pub const FONT_MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..FONT
};
pub const FONT_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..FONT
};
pub const FONT_BOLD: Font = Font {
    weight: Weight::Bold,
    ..FONT
};

// ── Espaçamento, raios e medidas ────────────────────────────────────────────
pub const SIDEBAR_WIDTH: f32 = 212.0;
pub const TOPBAR_HEIGHT: f32 = 32.0;
pub const PAGE_PADDING: f32 = 32.0;
#[allow(dead_code)] // token público do design system; nem toda tela usa todos.
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_5: f32 = 24.0;
pub const SPACE_6: f32 = 32.0;

/// A web usa `minmax(170px, 1fr)`. O `Grid::fluid` do iced arredonda o número
/// de colunas para cima; 190 px de limite produz as mesmas 7 colunas (~175 px)
/// da referência em 1600×900, sem esmagar os cards abaixo de 170 px.
pub const CARD_WIDTH: f32 = 190.0;
pub const CARD_GAP: f32 = 16.0;
/// `.arcadia-desktop-retro .rounded-xl` força 3 px nos cards do shell.
pub const CARD_RADIUS: f32 = 3.0;
pub const PANEL_RADIUS: f32 = 3.0;
pub const HERO_HEIGHT: f32 = 396.0;

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
        text_color: Some(TEXT_2),
        background: Some(BACKGROUND.into()),
        ..container::Style::default()
    }
}

pub fn sidebar(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_2),
        background: Some(SIDEBAR_BG.into()),
        border: Border {
            color: SIDEBAR_LINE,
            width: 1.0,
            radius: Radius::default(),
        },
        ..container::Style::default()
    }
}

pub fn sidebar_top(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SIDEBAR_BG.into()),
        border: Border {
            color: SIDEBAR_LINE,
            width: 1.0,
            radius: Radius::default(),
        },
        ..container::Style::default()
    }
}

pub fn sidebar_brand(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SIDEBAR_BG.into()),
        ..container::Style::default()
    }
}

pub fn top_bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(TOPBAR_BG.into()),
        border: Border {
            color: TOPBAR_LINE,
            width: 1.0,
            radius: Radius::default(),
        },
        ..container::Style::default()
    }
}

pub fn divider_accent(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SIDEBAR_LINE.into()),
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
        text_color: Some(TEXT_2),
        background: Some(CARD_BG.into()),
        border: rounded_border(CARD_BORDER, 1.0, CARD_RADIUS),
        ..container::Style::default()
    }
}

pub fn empty_panel(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_3),
        background: Some(SURFACE_1.into()),
        border: rounded_border(BORDER, 1.0, CARD_RADIUS),
        ..container::Style::default()
    }
}

pub fn badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.10).into()),
        border: Border::default().rounded(3),
        ..container::Style::default()
    }
}

pub fn cover_placeholder(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_3),
        background: Some(SURFACE_2.into()),
        border: rounded_border(CARD_BORDER, 1.0, CARD_RADIUS),
        ..container::Style::default()
    }
}

pub fn game_icon_placeholder(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_2),
        background: Some(Color::from_rgba8(0x22, 0xd3, 0xee, 0.22).into()),
        border: Border::default().rounded(2),
        ..container::Style::default()
    }
}

pub fn avatar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.10), 1.0, 18.0),
        ..container::Style::default()
    }
}

pub fn heart_badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.60).into()),
        border: Border::default().rounded(12),
        ..container::Style::default()
    }
}

pub fn hero_overlay(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.52).into()),
        ..container::Style::default()
    }
}

pub fn accent_bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(ACCENT.into()),
        border: Border::default().rounded(0),
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
        button::Status::Hovered => (Color::from_rgb8(0x16, 0x16, 0x1b), CARD_BORDER_HOVER),
        button::Status::Pressed => (SURFACE_2, ACCENT_BORDER),
        button::Status::Disabled => (SURFACE_1, BORDER),
        button::Status::Active => (SURFACE_2, CARD_BORDER),
    };

    button::Style {
        background: Some(background.into()),
        text_color: TEXT_1,
        border: rounded_border(border_color, 1.0, CARD_RADIUS),
        ..button::Style::default()
    }
}

pub fn home_card(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color) = match status {
        button::Status::Hovered => (CARD_BG, Color::from_rgba8(0x22, 0xd3, 0xee, 0.50)),
        button::Status::Pressed => (CARD_BG, ACCENT),
        button::Status::Disabled => (SURFACE_1, BORDER),
        button::Status::Active => (CARD_BG, SIDEBAR_LINE),
    };

    button::Style {
        background: Some(background.into()),
        text_color: TEXT_1,
        border: rounded_border(border_color, 1.0, CARD_RADIUS),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 20.0,
        },
        ..button::Style::default()
    }
}

pub fn plain_button(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: TEXT_1,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn sidebar_item(_theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let (background, border_color) = if active {
        (
            Some(Color::from_rgb8(0x00, 0x00, 0x00).into()),
            Color::from_rgba(1.0, 1.0, 1.0, 0.12),
        )
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => (
                Some(Color::from_rgba(1.0, 1.0, 1.0, 0.05).into()),
                Color::from_rgba(1.0, 1.0, 1.0, 0.14),
            ),
            _ => (None, TRANSPARENT),
        }
    };

    button::Style {
        background,
        text_color: if active {
            TEXT_1
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Color::from_rgba(1.0, 1.0, 1.0, 0.85)
        } else {
            Color::from_rgb8(0x95, 0xa0, 0xa6)
        },
        border: rounded_border(border_color, 1.0, 3.0),
        ..button::Style::default()
    }
}

pub fn game_item(_theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let background = if active {
        Some(Color::from_rgba(1.0, 1.0, 1.0, 0.07).into())
    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into())
    } else {
        None
    };

    button::Style {
        background,
        text_color: if active { TEXT_1 } else { TEXT_STRONG },
        border: Border::default().rounded(3),
        ..button::Style::default()
    }
}

pub fn icon_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into())
        }
        _ => None,
    };

    button::Style {
        background,
        text_color: TEXT_2,
        border: Border::default().rounded(3),
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
        border: Border::default().rounded(3),
        ..button::Style::default()
    }
}

pub fn ghost_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.07).into()),
            Color::from_rgba(1.0, 1.0, 1.0, 0.26),
            TEXT_1,
        ),
        button::Status::Disabled => (None, BORDER, TEXT_3),
        button::Status::Active => (
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.03).into()),
            BORDER_STRONG,
            Color::from_rgba(1.0, 1.0, 1.0, 0.78),
        ),
    };

    button::Style {
        background,
        text_color,
        border: rounded_border(border_color, 1.0, 3.0),
        ..button::Style::default()
    }
}

pub fn big_picture(_theme: &Theme, status: button::Status) -> button::Style {
    let (border_color, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (ACCENT, TEXT_1),
        button::Status::Disabled => (BORDER, TEXT_3),
        button::Status::Active => (
            Color::from_rgba8(0x22, 0xd3, 0xee, 0.22),
            Color::from_rgba(1.0, 1.0, 1.0, 0.70),
        ),
    };

    button::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.012).into()),
        text_color,
        border: rounded_border(border_color, 1.0, 3.0),
        ..button::Style::default()
    }
}

/// Fundo do segmented control Loja/Retro (`StoreTabs` da web).
pub fn segmented(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.025).into()),
        border: rounded_border(BORDER, 1.0, 3.0),
        ..container::Style::default()
    }
}

/// Aba do segmented control; a ativa usa o mesmo realce branco de 12% da web.
pub fn segment_button(_theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let background = if active {
        Color::from_rgba(1.0, 1.0, 1.0, 0.12)
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => {
                Color::from_rgba(1.0, 1.0, 1.0, 0.06)
            }
            _ => TRANSPARENT,
        }
    };

    button::Style {
        background: Some(background.into()),
        text_color: if active {
            TEXT_1
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.50)
        },
        border: Border::default().rounded(3),
        ..button::Style::default()
    }
}

/// Seletor de sistema do catálogo Retro, no mesmo vidro dos inputs.
pub fn pick_list(_theme: &Theme, status: pick_list_widget::Status) -> pick_list_widget::Style {
    let focused = matches!(
        status,
        pick_list_widget::Status::Hovered | pick_list_widget::Status::Opened { .. }
    );

    pick_list_widget::Style {
        text_color: TEXT_1,
        placeholder_color: TEXT_2,
        handle_color: TEXT_2,
        background: if focused {
            Color::from_rgba(1.0, 1.0, 1.0, 0.055).into()
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.04).into()
        },
        border: rounded_border(
            if focused {
                Color::from_rgba8(0x22, 0xd3, 0xee, 0.70)
            } else {
                Color::from_rgba8(0xff, 0xff, 0xff, 0.10)
            },
            1.0,
            3.0,
        ),
    }
}

pub fn pick_list_menu(_theme: &Theme) -> menu_widget::Style {
    menu_widget::Style {
        background: SURFACE_2.into(),
        border: rounded_border(BORDER_STRONG, 1.0, 3.0),
        text_color: TEXT_1,
        selected_text_color: TEXT_1,
        selected_background: ACCENT_SOFT.into(),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.55),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 20.0,
        },
    }
}

/// Bolinha de controle de janela no estilo macOS do `WindowControls.tsx`.
pub fn window_dot(fill: Color, ring: Color, status: button::Status) -> button::Style {
    let width = match status {
        button::Status::Hovered | button::Status::Pressed => 1.0,
        _ => 0.5,
    };

    button::Style {
        background: Some(fill.into()),
        text_color: Color::BLACK,
        border: Border {
            color: ring,
            width,
            radius: 6.0.into(),
        },
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
        background: if focused {
            Color::from_rgba(1.0, 1.0, 1.0, 0.055).into()
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.04).into()
        },
        border: rounded_border(
            if focused {
                Color::from_rgba8(0x22, 0xd3, 0xee, 0.70)
            } else {
                Color::from_rgba8(0xff, 0xff, 0xff, 0.10)
            },
            1.0,
            3.0,
        ),
        icon: Color::from_rgba(1.0, 1.0, 1.0, 0.40),
        placeholder: Color::from_rgba(1.0, 1.0, 1.0, 0.30),
        value: TEXT_1,
        selection: ACCENT_SOFT,
    }
}

pub fn sidebar_search_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = search_input(_theme, status);
    style.background = Color::from_rgba(1.0, 1.0, 1.0, 0.025).into();
    style.border = rounded_border(Color::from_rgba8(0x22, 0xd3, 0xee, 0.18), 1.0, 2.0);
    style
}

pub fn scrollable(theme: &Theme, status: scrollable_widget::Status) -> scrollable_widget::Style {
    let mut style = scrollable_widget::default(theme, status);
    let rail = scrollable_widget::Rail {
        background: Some(TRANSPARENT.into()),
        border: Border::default(),
        scroller: scrollable_widget::Scroller {
            background: Color::from_rgba(1.0, 1.0, 1.0, 0.18).into(),
            border: Border::default().rounded(3),
        },
    };

    style.vertical_rail = rail;
    style.horizontal_rail = rail;
    style
}
