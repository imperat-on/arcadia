//! Tokens visuais do Arcadia nativo, portados de `ui/src/index.css`.
//!
//! Os valores batem com os tokens do shell desktop: preto OLED, superfícies
//! #0d0d0f/#141419/#1c1c22, acento ciano #22d3ee e a geometria curta do
//! `.arcadia-desktop-retro` (raios de 3 px, sidebar de 212 px). A fonte é a
//! Inter empacotada em `assets/fonts` e carregada no `main.rs`.

use iced::border::Radius;
use iced::font::{Family, Stretch, Style as FontStyle, Weight};
use iced::gradient;
use iced::widget::overlay::menu as menu_widget;
use iced::widget::scrollable as scrollable_widget;
use iced::widget::slider;
use iced::widget::{button, container, pick_list as pick_list_widget, text_input, toggler};
use iced::{Border, Color, Font, Shadow, Theme, Vector};

// ── Cores (mesmos valores de `ui/src/index.css`) ────────────────────────────
pub const BACKGROUND: Color = Color::from_rgb8(0x00, 0x00, 0x00);
pub const SIDEBAR_BG: Color = Color::from_rgb8(0x03, 0x06, 0x0a);
pub const SURFACE_1: Color = Color::from_rgb8(0x0d, 0x0d, 0x0f);
pub const SURFACE_2: Color = Color::from_rgb8(0x14, 0x14, 0x19);
pub const SURFACE_3: Color = Color::from_rgb8(0x1c, 0x1c, 0x22);
pub const CARD_BG: Color = Color::from_rgb8(0x08, 0x0b, 0x0d);
/// Fundo do card da vitrine: um tom acima de `SURFACE_1`, para o card se
/// destacar do preto do fundo sem competir com a capa.
pub const HOME_CARD_BG: Color = Color::from_rgb8(0x11, 0x14, 0x18);

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

// ── Escala tipográfica ──────────────────────────────────────────────────────
//
// Antes desta escala, cada view escolhia `.size(12)`/`.size(12)`/`.size(13)` a
// olho, e o resultado era texto ilegível: 46 usos de 9 px e 125 de 10 px numa
// janela de 1400 px. A escala abaixo sobe o piso para 12 px e mantém a
// hierarquia — micro-rótulo, legenda, corpo, ênfase, título.
//
// Regra: usar sempre um destes nomes, nunca um número solto.

/// Rótulo minúsculo (badges, contadores em pill, texto sobre imagem).
pub const TEXT_MICRO: f32 = 13.0;
/// Legenda: metadados, linhas de apoio, rodapé de card.
pub const TEXT_CAPTION: f32 = 14.5;
/// Corpo padrão de interface.
pub const TEXT_BODY: f32 = 15.5;
/// Texto de leitura contínua (descrições longas da loja).
pub const TEXT_READ: f32 = 16.5;
/// Subtítulo e rótulo de controle em destaque.
pub const TEXT_LABEL: f32 = 17.5;
/// Título de seção e cabeçalho de painel.
pub const TEXT_HEADING: f32 = 20.0;
/// Título de página.
pub const TEXT_TITLE: f32 = 25.0;
/// Título de tela cheia (perfil, big picture).
pub const TEXT_DISPLAY: f32 = 30.0;

// ── Controles ───────────────────────────────────────────────────────────────
/// Altura do trilho do interruptor (`Toggler::size`).
pub const TOGGLE_SIZE: f32 = 26.0;
/// Altura de botão de linha de configuração.
pub const CONTROL_HEIGHT: f32 = 36.0;
pub const COMPONENT_COMPACT_WIDTH: f32 = 760.0;
/// Altura da ação centralizada nos cards da Home.
pub const HOME_ACTION_HEIGHT: f32 = 44.0;
pub const HOME_ACTION_RADIUS: f32 = 10.0;
pub const AUTH_FORM_WIDTH: f32 = 420.0;
pub const AUTH_CONTROL_HEIGHT: f32 = 44.0;
pub const AUTH_INPUT_HEIGHT: f32 = 46.0;
pub const AUTH_RADIUS: f32 = 24.0;
pub const AUTH_BLUE: Color = Color::from_rgb8(0x00, 0x59, 0xa5);
pub const AUTH_BLUE_HOVER: Color = Color::from_rgb8(0x05, 0x70, 0xc5);
pub const AUTH_BLUE_DIM: Color = Color::from_rgb8(0x04, 0x27, 0x44);
pub const AUTH_FIELD: Color = Color::from_rgb8(0x11, 0x16, 0x1b);
pub const AUTH_ICON: Color = Color::from_rgb8(0x70, 0x7c, 0x87);
pub const AUTH_GLOW: Color = Color::from_rgba8(0x00, 0x83, 0xca, 0.003);

// ── Espaçamento, raios e medidas ────────────────────────────────────────────
/// Acompanha a escala tipográfica: com corpo de 15,5 px a coluna de 212 px
/// apertava os rótulos de duas palavras ("Configurações").
pub const SIDEBAR_WIDTH: f32 = 232.0;
pub const TOPBAR_HEIGHT: f32 = 36.0;
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

// ── Autenticação ───────────────────────────────────────────────────────────

pub fn auth_background(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(BACKGROUND.into()),
        ..container::Style::default()
    }
}

pub fn auth_card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba8(0x06, 0x09, 0x0e, 0.90).into()),
        border: rounded_border(BORDER, 1.0, AUTH_RADIUS),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 30.0,
        },
        ..container::Style::default()
    }
}

pub fn auth_tab_bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba8(0x00, 0x00, 0x00, 0.45).into()),
        border: rounded_border(BORDER, 1.0, 10.0),
        ..container::Style::default()
    }
}

pub fn auth_tab(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let mut style = if active {
        library_tab_active(theme, status)
    } else {
        library_tab(theme, status)
    };
    if active {
        style.text_color = TEXT_1;
        style.border = rounded_border(TRANSPARENT, 0.0, 8.0);
        style.background = Some(AUTH_BLUE.into());
        style.shadow = Shadow {
            color: Color::from_rgba8(0x00, 0x72, 0xce, 0.25),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 16.0,
        };
    }
    style
}

/// Campo de autenticação: fundo sólido, raio curto e anel ciano só quando
/// focado — o par `bg-white/[0.04]` + `focus:border-accent/60` do `AuthDialog`.
/// O ícone é desenhado por cima, na faixa reservada pelo `padding` esquerdo.
pub fn auth_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: AUTH_FIELD.into(),
        border: rounded_border(if focused { ACCENT } else { BORDER_STRONG }, 1.0, 4.0),
        icon: AUTH_ICON,
        placeholder: Color::from_rgba(1.0, 1.0, 1.0, 0.28),
        value: TEXT_1,
        selection: ACCENT_SOFT,
    }
}

pub fn auth_submit(theme: &Theme, status: button::Status, populated: bool) -> button::Style {
    let mut style = primary_button(theme, status);
    style.background = Some(
        if !populated || matches!(status, button::Status::Disabled) {
            AUTH_BLUE_DIM
        } else if matches!(status, button::Status::Hovered) {
            AUTH_BLUE_HOVER
        } else {
            AUTH_BLUE
        }
        .into(),
    );
    style.text_color = if populated { TEXT_1 } else { TEXT_2 };
    style.border.radius = 3.0.into();
    style
}

pub fn auth_close(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(
            Color::from_rgba(
                1.0,
                1.0,
                1.0,
                if matches!(status, button::Status::Hovered) {
                    0.10
                } else {
                    0.04
                },
            )
            .into(),
        ),
        text_color: TEXT_2,
        border: rounded_border(BORDER, 1.0, 24.0),
        ..button::Style::default()
    }
}

// ── Perfil ────────────────────────────────────────────────────────────────

/// Botão de identidade da sidebar: anel ciano quando o menu está aberto,
/// espelhando o `focus` do card do `Sidebar.tsx`.
pub fn profile_identity(_theme: &Theme, status: button::Status, open: bool) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.04).into())
        }
        _ => None,
    };
    let border_color = if open {
        ACCENT_BORDER
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => BORDER_STRONG,
            _ => TRANSPARENT,
        }
    };

    button::Style {
        background,
        text_color: TEXT_1,
        border: rounded_border(border_color, if open { 1.0 } else { 0.0 }, 12.0),
        ..button::Style::default()
    }
}

/// Painel expandido do perfil: `rounded-xl border white/10 bg surface-2`.
pub fn profile_menu(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_2.into()),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// Item do menu do perfil: `px-4 py-2.5 hover:bg-white/[0.07]`, vermelho no
/// "Sair" (`--state-danger`).
pub fn profile_menu_item(_theme: &Theme, status: button::Status, danger: bool) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.07).into())
        }
        _ => None,
    };

    button::Style {
        background,
        text_color: if danger {
            DANGER
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.80)
        },
        border: Border::default(),
        ..button::Style::default()
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

/// Painel sem fundo nem borda: só agrupa conteúdo.
///
/// Usado no container pai do player/galeria, onde a placa preta em volta do
/// vídeo e das miniaturas era justamente o que se queria eliminar.
pub fn panel_bare(_theme: &Theme) -> container::Style {
    container::Style {
        background: None,
        border: Border::default(),
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

/// Componentes: superfície discreta e borda ciano quando ativo.
pub fn component_panel(theme: &Theme, active: bool) -> container::Style {
    container::Style {
        border: rounded_border(if active {ACCENT_BORDER} else {CARD_BORDER},1.0,CARD_RADIUS),
        ..stub_panel(theme)
    }
}

pub fn component_primary_button(theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        text_color: if status==button::Status::Disabled {TEXT_3} else {BACKGROUND},
        ..primary_button(theme,status)
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

/// Véu escuro atrás de overlays modais (seletor de trailer, confirmações).
pub fn scrim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.72).into()),
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

/// Faixa larga do banner do perfil (`rounded-2xl border border-white/10`).
pub fn profile_banner(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_2.into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.10), 1.0, 16.0),
        ..container::Style::default()
    }
}

/// Banner sem imagem: gradiente escuro do `profile-banner` da web.
pub fn banner_placeholder(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(
            iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_4))
                .add_stop(0.0, Color::from_rgba8(0x1c, 0x2a, 0x3a, 1.0))
                .add_stop(0.6, Color::from_rgba8(0x0e, 0x12, 0x1a, 1.0))
                .add_stop(1.0, Color::from_rgba8(0x05, 0x06, 0x08, 1.0))
                .into(),
        ),
        ..container::Style::default()
    }
}

/// Botão de capa do perfil: sem fundo, sem borda, sem padding visual.
pub fn tile_button(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: TEXT_1,
        border: Border::default().rounded(8),
        ..button::Style::default()
    }
}

/// Badge de horas sobre a capa (`bg-black/65 backdrop-blur-sm`).
pub fn hours_badge(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.65).into()),
        border: Border::default().rounded(6),
        ..container::Style::default()
    }
}

/// Cabeçalho de card (`bg-white/[0.03]` + borda inferior sutil).
pub fn card_header(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.03).into()),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.06),
            width: 0.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// Linha de estatística (`rgba(255,255,255,0.03)`, canto 8px).
pub fn stat_row(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.03).into()),
        border: Border::default().rounded(8),
        ..container::Style::default()
    }
}

/// Véu escuro sobre o fundo do perfil — o gradiente de três paradas do
/// `ProfilePage.tsx` (`0.72` → `0.82` → `0.94`), aqui numa média que mantém a
/// atmosfera e ainda deixa o banner legível.
pub fn profile_scrim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(
            iced::gradient::Linear::new(iced::Radians(0.0))
                .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.72))
                .add_stop(0.40, Color::from_rgba(0.0, 0.0, 0.0, 0.82))
                .add_stop(1.0, Color::from_rgba(0.0, 0.0, 0.0, 0.94))
                .into(),
        ),
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

/// Preserva a arte acima e escurece onde ficam título e descrição.
pub fn home_hero_scrim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(
            gradient::Linear::new(iced::Radians::PI)
                .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.10))
                .add_stop(0.28, Color::from_rgba(0.0, 0.0, 0.0, 0.28))
                .add_stop(0.70, Color::from_rgba(0.0, 0.0, 0.0, 0.85))
                .add_stop(1.0, BACKGROUND)
                .into(),
        ),
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

/// Botão "Remover" da barra da ficha (`DangerBtn` da `StoreGamePage.tsx`):
/// pílula, borda vermelha a 40%, texto da cor de perigo, fundo só no hover.
pub fn store_danger_pill(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Some(Color::from_rgba(1.0, 0.42, 0.51, 0.10).into())
        }
        button::Status::Disabled => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.02).into()),
        button::Status::Active => None,
    };
    let text_color = if matches!(status, button::Status::Disabled) {
        Color::from_rgba(1.0, 0.42, 0.51, 0.45)
    } else {
        DANGER
    };
    button::Style {
        background,
        text_color,
        border: rounded_border(Color::from_rgba(1.0, 0.42, 0.51, 0.40), 1.0, 999.0),
        ..button::Style::default()
    }
}

/// "Salvar" da chave Hubcap: fundo ciano, texto preto, cantos de 8 px.
pub fn hubcap_save_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Color::from_rgb8(0x67, 0xe8, 0xf9),
        button::Status::Disabled => Color::from_rgba(0.13, 0.83, 0.93, 0.45),
        button::Status::Active => ACCENT,
    };
    button::Style {
        background: Some(background.into()),
        text_color: Color::BLACK,
        border: Border::default().rounded(8.0),
        ..button::Style::default()
    }
}

/// "Validar" da chave Hubcap: borda branca suave, fundo transparente.
pub fn hubcap_validate_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (border_color, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => {
            (Color::from_rgba(1.0, 1.0, 1.0, 0.30), TEXT_1)
        }
        button::Status::Disabled => (Color::from_rgba(1.0, 1.0, 1.0, 0.08), TEXT_3),
        button::Status::Active => (Color::from_rgba(1.0, 1.0, 1.0, 0.15), Color::from_rgba(1.0, 1.0, 1.0, 0.80)),
    };
    button::Style {
        background: None,
        text_color,
        border: rounded_border(border_color, 1.0, 8.0),
        ..button::Style::default()
    }
}

/// Botão branco da barra da ficha (`PrimaryBtn` da `StoreGamePage.tsx`):
/// pílula, fundo branco, texto preto, 12.5 px em negrito.
pub fn store_primary_pill(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (
            Color::from_rgb8(0xf4, 0xf4, 0xf5),
            Color::BLACK,
        ),
        button::Status::Disabled => (
            Color::from_rgba(1.0, 1.0, 1.0, 0.50),
            Color::from_rgba(0.0, 0.0, 0.0, 0.55),
        ),
        button::Status::Active => (Color::WHITE, Color::BLACK),
    };
    button::Style {
        background: Some(background.into()),
        text_color,
        border: Border::default().rounded(999.0),
        ..button::Style::default()
    }
}

/// Botão fantasma da barra da ficha (`GhostBtn` da `StoreGamePage.tsx`):
/// pílula, borda branca a 15%, fundo quase transparente, texto a 85%.
pub fn store_ghost_pill(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (
            Color::from_rgba(1.0, 1.0, 1.0, 0.07),
            Color::from_rgba(1.0, 1.0, 1.0, 0.25),
            Color::WHITE,
        ),
        button::Status::Disabled => (
            Color::from_rgba(1.0, 1.0, 1.0, 0.02),
            Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            Color::from_rgba(1.0, 1.0, 1.0, 0.40),
        ),
        button::Status::Active => (
            Color::from_rgba(1.0, 1.0, 1.0, 0.03),
            Color::from_rgba(1.0, 1.0, 1.0, 0.15),
            Color::from_rgba(1.0, 1.0, 1.0, 0.85),
        ),
    };
    button::Style {
        background: Some(background.into()),
        text_color,
        border: rounded_border(border_color, 1.0, 999.0),
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

/// Aba da Biblioteca: ativa com fundo claro e texto branco, inativa apagada —
/// o mesmo par `bg-white/[0.12] text-white` / `text-white/50` da web.
pub fn library_tab(_theme: &Theme, status: button::Status) -> button::Style {
    // O estado ativo é escolhido pela view via `library_tab_active`; aqui só
    // tratamos o hover do inativo.
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
        text_color: if hovered { TEXT_1 } else { TEXT_2 },
        border: Border::default().rounded(8),
        ..button::Style::default()
    }
}

/// Aba ativa da Biblioteca.
pub fn library_tab_active(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.12).into()),
        text_color: TEXT_1,
        border: Border::default().rounded(8),
        ..button::Style::default()
    }
}

/// Campo de caminho: fundo escuro com borda sutil, como o `input` da web.
pub fn path_input(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_STRONG),
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.40).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.12), 1.0, 8.0),
        ..container::Style::default()
    }
}

/// Botão de linha de lista: sem fundo, sem borda, só o conteúdo clicável.
pub fn row_button(_theme: &Theme, status: button::Status) -> button::Style {
    let text_color = match status {
        button::Status::Hovered | button::Status::Pressed => TEXT_1,
        _ => TEXT_STRONG,
    };
    button::Style {
        background: None,
        text_color,
        border: Border::default(),
        ..button::Style::default()
    }
}

/// Linha divisória de 1 px entre itens de uma lista.
pub fn divider(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
        ..container::Style::default()
    }
}

/// Interruptor das configurações: trilho ciano quando ligado, cinza quando
/// desligado — o mesmo desenho do toggle da web.
pub fn toggler_switch(_theme: &Theme, status: toggler::Status) -> toggler::Style {
    let (track, knob) = match status {
        toggler::Status::Active { is_toggled: true } => (ACCENT, BACKGROUND),
        toggler::Status::Hovered { is_toggled: true } => {
            (Color::from_rgb8(0x67, 0xe8, 0xf9), BACKGROUND)
        }
        toggler::Status::Active { is_toggled: false } => {
            (SURFACE_3, Color::from_rgb8(0x9a, 0xa3, 0xb2))
        }
        toggler::Status::Hovered { is_toggled: false } => {
            (Color::from_rgb8(0x26, 0x26, 0x2e), TEXT_1)
        }
        toggler::Status::Disabled { is_toggled: true } => {
            (Color::from_rgba8(0x22, 0xd3, 0xee, 0.40), BACKGROUND)
        }
        toggler::Status::Disabled { is_toggled: false } => {
            (SURFACE_2, Color::from_rgb8(0x5a, 0x62, 0x70))
        }
    };

    toggler::Style {
        background: track.into(),
        background_border_width: 0.0,
        background_border_color: track,
        foreground: knob.into(),
        foreground_border_width: 0.0,
        foreground_border_color: knob,
        text_color: None,
        // Totalmente arredondado, como o switch da web.
        border_radius: None,
        padding_ratio: 0.14,
    }
}

/// Ação de item já presente na biblioteca, com destaque ciano discreto.
pub fn library_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color) = match status {
        button::Status::Hovered => (Some(ACCENT_SOFT.into()), ACCENT),
        button::Status::Pressed => (Some(ACCENT_BORDER.into()), ACCENT),
        button::Status::Disabled => (None, BORDER),
        button::Status::Active => (Some(BACKGROUND.into()), ACCENT_BORDER),
    };

    button::Style {
        background,
        text_color: ACCENT,
        border: rounded_border(border_color, 1.0, HOME_ACTION_RADIUS),
        ..button::Style::default()
    }
}

/// Cartão da vitrine da Home (arte + título + ação).
/// Capa retrato flutuante do herói (150×225), com aro e sombra.
pub fn portrait_frame(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(BACKGROUND.into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.10), 1.0, 8.0),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.65),
            offset: Vector::new(0.0, 18.0),
            blur_radius: 36.0,
        },
        ..container::Style::default()
    }
}

/// Botão voltar circular de vidro: fundo escuro translúcido, borda que
/// acende no hover e sombra suave que descola do herói.
pub fn back_circle(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (
            Color::from_rgba(0.04, 0.05, 0.07, 0.88),
            Color::from_rgba(1.0, 1.0, 1.0, 0.22),
        ),
        button::Status::Disabled => (
            Color::from_rgba(0.0, 0.0, 0.0, 0.30),
            Color::from_rgba(1.0, 1.0, 1.0, 0.06),
        ),
        _ => (
            Color::from_rgba(0.0, 0.0, 0.0, 0.58),
            Color::from_rgba(1.0, 1.0, 1.0, 0.12),
        ),
    };

    button::Style {
        background: Some(background.into()),
        text_color: TEXT_1,
        border: rounded_border(border_color, 1.0, 20.0),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 14.0,
        },
        ..button::Style::default()
    }
}

/// Véu escuro sobre o poster enquanto o trailer faz o pré-roll.
pub fn media_loading_veil(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.42).into()),
        ..container::Style::default()
    }
}

/// Superfície do player: caixa arredondada preta que recorta o vídeo.
/// Superfície do vídeo sem placa: fundo e borda totalmente transparentes.
///
/// O container só existe para recortar o canto arredondado. Antes ele pintava
/// um retângulo preto com borda 6% branco, que virava uma moldura visível em
/// volta do filme — a "caixa preta" indesejada.
pub fn player_bare_surface(_theme: &Theme) -> container::Style {
    container::Style {
        background: None,
        border: Border::default(),
        ..container::Style::default()
    }
}

pub fn player_surface(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgb8(0x00, 0x00, 0x00).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.06), 1.0, 12.0),
        ..container::Style::default()
    }
}

/// Trilho da barra de controles do player.
///
/// Sem caixa: só um degradê sutil para baixo, que garante contraste do texto
/// sobre qualquer frame do vídeo sem desenhar um retângulo escuro — o
/// "vidro" antigo pesava e cortava a imagem.
pub fn player_controls(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(
            gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
                .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.0))
                .add_stop(0.45, Color::from_rgba(0.0, 0.0, 0.0, 0.22))
                .add_stop(1.0, Color::from_rgba(0.0, 0.0, 0.0, 0.55))
                .into(),
        ),
        ..container::Style::default()
    }
}

/// Controle redondo/compacto da barra do player.
pub fn player_control(_theme: &Theme, status: button::Status) -> button::Style {
    // Sem caixa em repouso: os controles são só glifo. O realce aparece no
    // hover, e mesmo assim bem discreto — o retângulo cinza fixo por trás de
    // cada botão era o que "matava o visual".
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Color::from_rgba(1.0, 1.0, 1.0, 0.12),
        button::Status::Disabled => Color::TRANSPARENT,
        _ => Color::TRANSPARENT,
    };

    button::Style {
        background: Some(background.into()),
        text_color: TEXT_1,
        border: rounded_border(Color::TRANSPARENT, 0.0, 8.0),
        ..button::Style::default()
    }
}

/// Scrubber do player: trilho baixo e accent preenchendo o progresso.
pub fn player_slider(_theme: &Theme, status: slider::Status) -> slider::Style {
    let handle_active = matches!(status, slider::Status::Dragged | slider::Status::Hovered);
    // `backgrounds` é (trilho cheio, trilho vazio): o accent preenche o
    // progresso e o restante fica num branco baixo sobre o vidro.
    slider::Style {
        rail: slider::Rail {
            backgrounds: (ACCENT.into(), Color::from_rgba(1.0, 1.0, 1.0, 0.18).into()),
            width: 3.0,
            border: rounded_border(Color::TRANSPARENT, 0.0, 2.0),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle {
                radius: if handle_active { 6.5 } else { 5.5 },
            },
            background: ACCENT.into(),
            border_width: 2.0,
            border_color: Color::from_rgba(1.0, 1.0, 1.0, 0.92),
        },
    }
}

pub fn chip(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_STRONG),
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.07).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.10), 1.0, 20.0),
        ..container::Style::default()
    }
}

pub fn chip_accent(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(ACCENT),
        background: Some(ACCENT_SOFT.into()),
        border: rounded_border(ACCENT_BORDER, 1.0, 20.0),
        ..container::Style::default()
    }
}

pub fn action_bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.06),
            width: 1.0,
            radius: Radius::default(),
        },
        ..container::Style::default()
    }
}

pub fn home_store_card(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(HOME_CARD_BG.into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.06), 1.0, 12.0),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..container::Style::default()
    }
}

/// Ação principal do card da Home, com contraste e hover do tema.
pub fn home_detail_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color, text_color) = match status {
        button::Status::Hovered => (SURFACE_2, ACCENT_BORDER, TEXT_1),
        button::Status::Pressed => (SURFACE_2, ACCENT, TEXT_1),
        button::Status::Disabled => (SURFACE_1, BORDER, TEXT_3),
        button::Status::Active => (BACKGROUND, BORDER, TEXT_2),
    };
    button::Style {
        background: Some(background.into()),
        text_color,
        border: rounded_border(border_color, 1.0, HOME_ACTION_RADIUS),
        ..button::Style::default()
    }
}

/// Botão de ação secundária da vitrine.
/// Pill de altura fixa: fundo baixo por padrão, tinge 8% no hover.
pub fn card_action_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.10).into()),
            Color::from_rgba(1.0, 1.0, 1.0, 0.20),
        ),
        button::Status::Disabled => (Some(Color::from_rgba(1.0, 1.0, 1.0, 0.02).into()), BORDER),
        _ => (
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.05).into()),
            Color::from_rgba(1.0, 1.0, 1.0, 0.10),
        ),
    };

    button::Style {
        background,
        text_color: Color::from_rgba(1.0, 1.0, 1.0, 0.86),
        border: rounded_border(border_color, 1.0, 8.0),
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

/// Linha do catálogo da loja (`.store-catalog-row` da `ui/src/index.css`:
/// fundo #0b0b0b, borda branca 9%, raio 3, hover #171717 com borda 16%).
pub fn store_row(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color) = match status {
        button::Status::Hovered => (Color::from_rgb8(0x17, 0x17, 0x17), BORDER_STRONG),
        button::Status::Pressed => (Color::from_rgb8(0x1c, 0x1c, 0x22), ACCENT_BORDER),
        button::Status::Disabled => (Color::from_rgb8(0x0b, 0x0b, 0x0b), BORDER),
        button::Status::Active => (Color::from_rgb8(0x0b, 0x0b, 0x0b), CARD_BORDER),
    };

    button::Style {
        background: Some(background.into()),
        text_color: Color::from_rgba(1.0, 1.0, 1.0, 0.86),
        border: rounded_border(border_color, 1.0, 3.0),
        ..button::Style::default()
    }
}

/// `.store-catalog-art`: a capa à esquerda da linha, com a divisória de 7%.
pub fn store_row_art(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::BLACK.into()),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.07),
            width: 1.0,
            radius: Radius::default(),
        },
        ..container::Style::default()
    }
}

/// Etiqueta da fonte do jogo (`border-white/15 bg-white/[.035] px-2 py-0.5`).
pub fn store_row_tag(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.035).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.15), 1.0, 3.0),
        ..container::Style::default()
    }
}

/// Sub-item de Configurações na sidebar (`CONFIG_SUBS` do `Sidebar.tsx`):
/// fundo branco 6% quando ativo, raio 8, texto 14 px.
pub fn sub_item(_theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let background = if active {
        Color::from_rgba(1.0, 1.0, 1.0, 0.06)
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => {
                Color::from_rgba(1.0, 1.0, 1.0, 0.03)
            }
            _ => TRANSPARENT,
        }
    };

    button::Style {
        background: Some(background.into()),
        text_color: if active {
            TEXT_1
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.45)
        },
        border: Border::default().rounded(8.0),
        ..button::Style::default()
    }
}

/// Ponto de estado do sub-item: ciano quando ativo, branco 25% quando não.
pub fn sub_item_dot(active: bool) -> container::Style {
    container::Style {
        background: Some(
            if active {
                ACCENT
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.25)
            }
            .into(),
        ),
        border: Border::default().rounded(2.0),
        ..container::Style::default()
    }
}

/// Badge `NEW` de Emulação: contorno e texto no acento.
pub fn accent_badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(TRANSPARENT.into()),
        border: Border {
            color: Color::from_rgba(0.133, 0.827, 0.933, 0.60),
            width: 1.0,
            radius: 999.0.into(),
        },
        text_color: Some(ACCENT),
        ..container::Style::default()
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

// ── Modo console (Big Picture) ──────────────────────────────────────────────
/// Top bar translúcida por cima do hero, como o gradiente do `ps5-topbar`.
/// Faixa da top bar: só o gradiente de escurecimento no topo, sem caixa.
///
/// Antes tinha fundo e borda próprios, o que desenhava um retângulo cinza de
/// largura total sobre a arte do hero. A barra do Orbit não é uma caixa — é o
/// fundo escurecendo atrás do conteúdo solto (relógio, pílula, perfil).
pub fn console_topbar(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        // Sem fundo nenhum: o único retângulo arredondado da barra é a pílula
        // de navegação. Um véu de largura total aqui pintava uma faixa cinza
        // sobre a arte do hero e engolia a pílula.
        background: None,
        ..container::Style::default()
    }
}

/// Cabeçalho de painel flutuante (menu "•••", ficha da loja): fundo escuro
/// com cantos arredondados só no topo, para emendar no corpo do painel.
pub fn console_panel_header(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.28).into()),
        ..container::Style::default()
    }
}

/// Escurecimento geral sobre o hero para o texto continuar legível.
/// Véu sobre a arte do hero. O iced não tem gradiente de verdade nesta
/// camada, então o degradê é montado em faixas empilhadas na view — este é o
/// véu de topo, que só segura o contraste da pílula e do relógio.
pub fn console_wash(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.22).into()),
        ..container::Style::default()
    }
}

/// Primeiro degrau do gradiente inferior (33% da altura).
pub fn console_wash_step_low(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.28).into()),
        ..container::Style::default()
    }
}

/// Segundo degrau do gradiente inferior (24% da altura).
pub fn console_wash_step_mid(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.44).into()),
        ..container::Style::default()
    }
}

/// Base do gradiente: escurece o pé da tela para o trilho e os prompts.
pub fn console_bottom_wash(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.72).into()),
        ..container::Style::default()
    }
}

/// Camada do painel do jogo (Game Overview): fundo escuro quase opaco.
#[allow(dead_code)] // mantido como fallback estático; a view usa a versão com alfa.
pub fn console_overview(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.82).into()),
        ..container::Style::default()
    }
}

/// Igual ao [`console_overview`], com alfa para a entrada animada do painel.
pub fn console_overview_alpha(alpha: f32) -> impl Fn(&Theme) -> container::Style {
    let alpha = alpha.clamp(0.0, 1.0);
    move |_theme| container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.82 * alpha).into()),
        ..container::Style::default()
    }
}

pub fn console_panel(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba8(8, 12, 17, 0.94).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.14), 1.0, 10.0),
        ..container::Style::default()
    }
}

/// Painel com alfa animado; o texto continua legível no estado final.
pub fn console_panel_alpha(alpha: f32) -> impl Fn(&Theme) -> container::Style {
    let alpha = alpha.clamp(0.0, 1.0);
    move |_theme| container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba8(8, 12, 17, 0.94 * alpha).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.14 * alpha), 1.0, 10.0),
        ..container::Style::default()
    }
}

/// Véu do menu "•••" e da ficha da loja com alfa de entrada.
pub fn console_veil(alpha: f32) -> impl Fn(&Theme) -> container::Style {
    let alpha = alpha.clamp(0.0, 1.0);
    move |_theme| container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.72 * alpha).into()),
        ..container::Style::default()
    }
}

#[allow(dead_code)] // o bloco de perfil saiu da top bar do console; estilo guardado
pub fn console_profile(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.10), 1.0, 20.0),
        ..container::Style::default()
    }
}

/// Bolinha do indicador de conexão; o diâmetro vem do widget.
pub fn console_status_dot(color: Color) -> container::Style {
    container::Style {
        background: Some(color.into()),
        border: Border {
            color: color,
            width: 0.0,
            radius: 5.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn console_tab(
    _theme: &Theme,
    status: button::Status,
    active: bool,
    focused: bool,
    glow: f32,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let background = if focused {
        Some(Color::from_rgba(1.0, 1.0, 1.0, 0.12).into())
    } else if hovered {
        Some(Color::from_rgba(1.0, 1.0, 1.0, 0.07).into())
    } else {
        None
    };

    button::Style {
        background,
        text_color: if active {
            TEXT_1
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.64)
        },
        border: rounded_border(
            if focused {
                Color::from_rgba(1.0, 1.0, 1.0, 0.62 + 0.28 * glow.clamp(0.0, 1.0))
            } else {
                TRANSPARENT
            },
            if focused { 2.0 } else { 0.0 },
            6.0,
        ),
        ..button::Style::default()
    }
}

/// Tecla do rodapé de prompts do Big Picture: cápsula pequena com a letra do
/// botão do controle, como o `A Select / B Back / LB RB Sections` do Orbit.
pub fn console_key_cap(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.12).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.16), 1.0, 5.0),
        ..container::Style::default()
    }
}

/// Faixa do rodapé de prompts: escurece o pé da tela para o texto não brigar
/// com a arte do hero.
pub fn console_prompt_bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.42).into()),
        ..container::Style::default()
    }
}

/// Chip de filtro da fileira do Big Picture (`Todos`, `Favoritos`,
/// `Ver biblioteca`). Ativo = fundo claro; inativo = só texto.
pub fn console_filter_chip(_theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(
            if active {
                Color::from_rgba(1.0, 1.0, 1.0, 0.14)
            } else if hovered {
                Color::from_rgba(1.0, 1.0, 1.0, 0.08)
            } else {
                Color::TRANSPARENT
            }
            .into(),
        ),
        text_color: TEXT_1,
        border: rounded_border(Color::TRANSPARENT, 0.0, 999.0),
        shadow: Shadow::default(),
        ..button::Style::default()
    }
}

/// Fundo atrás de uma capa vertical num card quadrado.
///
/// A arte 2:3 não preenche 1:1 — sobra faixa nas laterais. Um fundo quase
/// preto faz a sobra ler como moldura em vez de erro de carregamento, que é
/// o mesmo truque do `bg-black` + `bg-contain` do `SquareGameArt` do Orbit.
pub fn cover_portrait_backdrop(_theme: &Theme) -> container::Style {
    container::Style {
        // Só o fundo, sem borda: `Border::default().rounded(12)` desenhava um
        // contorno de 1 px porque o `Default` do iced deixa `width = 1.0` e
        // `color = Color::BLACK` com alfa cheio — o raio sozinho não anula a
        // borda. Zerar a largura é o que remove o risco.
        background: Some(Color::from_rgb8(0x0a, 0x0b, 0x0e).into()),
        border: rounded_border(Color::TRANSPARENT, 0.0, 12.0),
        ..container::Style::default()
    }
}

/// Container de vidro atrás do trio de ações do hero — o `LiquidGlass` do
/// Orbit.
///
/// No CSS do Orbit o vidro tem três partes: um véu branco translúcido, uma
/// borda clara de 1 px que simula a luz pegando na quina, e um realce
/// interno no topo (`inset 0 1px 0`) com sombra interna embaixo. O iced não
/// tem `backdrop-filter`, então o desfoque não existe — o que dá a leitura
/// de vidro é a soma de alfa + borda + brilho interno, que é o que este
/// estilo monta.
pub fn console_glass_pill(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.075).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.16), 1.0, 999.0),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 26.0,
        },
        ..container::Style::default()
    }
}

/// Botão primário do hero: pílula clara com texto escuro — o "Jump back in"
/// do Orbit, que é o único elemento claro da tela e por isso puxa o olho.
pub fn console_hero_primary(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(
            if hovered {
                Color::from_rgb8(0xff, 0xff, 0xff)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.94)
            }
            .into(),
        ),
        text_color: Color::from_rgb8(0x0c, 0x0e, 0x12),
        border: rounded_border(Color::TRANSPARENT, 0.0, 999.0),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.28),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 10.0,
        },
        ..button::Style::default()
    }
}

/// Botão secundário do hero: vidro escuro com borda sutil — o "Details" do
/// Orbit. Nunca compete com o primário.
/// Botão secundário do hero: vidro translúcido sem borda — o "Details" do
/// Orbit. A borda de 1 px que existia aqui desenhava um contorno que o print
/// de referência não tem: o botão é só um véu claro sobre a arte, com a
/// mesma linguagem da pílula de navegação (forma sem contorno).
pub fn console_hero_secondary(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(
            if hovered {
                Color::from_rgba(1.0, 1.0, 1.0, 0.20)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.12)
            }
            .into(),
        ),
        text_color: TEXT_1,
        border: rounded_border(Color::TRANSPARENT, 0.0, 999.0),
        shadow: Shadow::default(),
        ..button::Style::default()
    }
}

/// Botão circular de favorito do hero. Mesma linguagem de vidro do
/// secundário, mas com padding simétrico — o `Button` do iced não tem raio
/// automático, então largura e altura iguais com raio 999 é o que faz o
/// círculo. O coração fica laranja quando o jogo já é favorito.
pub fn console_hero_icon(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(
            if hovered {
                Color::from_rgba(1.0, 1.0, 1.0, 0.20)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.12)
            }
            .into(),
        ),
        text_color: TEXT_1,
        border: rounded_border(Color::TRANSPARENT, 0.0, 999.0),
        shadow: Shadow::default(),
        ..button::Style::default()
    }
}

/// Casca da pílula de navegação do Big Picture: cápsula escura translúcida
/// com blur simulado por alfa, como a barra flutuante do Orbit.
pub fn console_pill_shell(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba8(10, 12, 16, 0.72).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.08), 1.0, 999.0),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 24.0,
        },
        ..container::Style::default()
    }
}

/// Item da pílula de navegação.
///
/// Ativo = fundo claro e texto branco (o `bg-white/[0.12] text-white` do
/// Orbit); focado = anel branco para leitura de gamepad à distância.
pub fn console_pill_item(
    _theme: &Theme,
    status: button::Status,
    active: bool,
    focused: bool,
    glow: f32,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let background = if active {
        Color::from_rgba(1.0, 1.0, 1.0, 0.14)
    } else if hovered {
        Color::from_rgba(1.0, 1.0, 1.0, 0.08)
    } else {
        Color::TRANSPARENT
    };

    button::Style {
        background: Some(background.into()),
        text_color: TEXT_1,
        border: rounded_border(
            if focused {
                Color::from_rgba(1.0, 1.0, 1.0, 0.52 + 0.28 * glow.clamp(0.0, 1.0))
            } else {
                Color::TRANSPARENT
            },
            if focused { 1.5 } else { 0.0 },
            999.0,
        ),
        shadow: Shadow::default(),
        ..button::Style::default()
    }
}

// Ficha do Big Picture: referência fornecida em 1121 × 756.
pub const CONSOLE_DETAIL_BG: Color = Color::from_rgb8(0x05, 0x06, 0x08);
pub const CONSOLE_DETAIL_CARD: Color = Color::from_rgb8(0x11, 0x15, 0x1b);
pub const CONSOLE_DETAIL_ACCENT: Color = Color::from_rgb8(0x93, 0x3f, 0xff);
pub const CONSOLE_DETAIL_HERO: f32 = 0.53;
pub const CONSOLE_DETAIL_GAP: f32 = 14.0;
pub const CONSOLE_DETAIL_RADIUS: f32 = 14.0;
pub const CONSOLE_DETAIL_ACTION: f32 = 44.0;
pub const CONSOLE_DETAIL_STAT: f32 = 66.0;
pub const CONSOLE_DETAIL_THUMB: f32 = 242.0;
pub const CONSOLE_DETAIL_THUMB_HEIGHT: f32 = 136.0;

pub fn console_detail_background(_theme: &Theme) -> container::Style {
    container::Style { background: Some(CONSOLE_DETAIL_BG.into()), ..Default::default() }
}

pub fn console_detail_stat(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(CONSOLE_DETAIL_CARD.into()),
        border: Border::default().rounded(CONSOLE_DETAIL_RADIUS),
        ..Default::default()
    }
}

pub fn console_detail_scrim(vertical: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(if vertical {
            gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
                .add_stop(0.0, Color::TRANSPARENT)
                .add_stop(0.58, Color::from_rgba(0.02, 0.024, 0.031, 0.15))
                .add_stop(1.0, CONSOLE_DETAIL_BG).into()
        } else {
            gradient::Linear::new(iced::Radians(0.0))
                .add_stop(0.0, Color::from_rgba(0.02, 0.024, 0.031, 0.95))
                .add_stop(0.52, Color::from_rgba(0.02, 0.024, 0.031, 0.60))
                .add_stop(1.0, Color::TRANSPARENT).into()
        }),
        ..Default::default()
    }
}

pub fn console_detail_action(_theme: &Theme, status: button::Status, primary: bool, focused: bool) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(if primary { TEXT_1 } else if hovered { SURFACE_3 } else { CONSOLE_DETAIL_CARD }.into()),
        text_color: if primary { CONSOLE_DETAIL_BG } else { TEXT_1 },
        border: rounded_border(if focused { CONSOLE_DETAIL_ACCENT } else { Color::TRANSPARENT }, if focused { 2.0 } else { 0.0 }, 999.0),
        ..Default::default()
    }
}

pub fn console_detail_tab(_theme: &Theme, status: button::Status, active: bool, focused: bool) -> button::Style {
    button::Style {
        text_color: if active || focused { TEXT_1 } else { TEXT_STRONG },
        background: Some(if focused || matches!(status, button::Status::Hovered) { SURFACE_2 } else { Color::TRANSPARENT }.into()),
        ..Default::default()
    }
}

pub fn console_detail_underline(active: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(if active { CONSOLE_DETAIL_ACCENT } else { Color::TRANSPARENT }.into()),
        ..Default::default()
    }
}

pub fn console_detail_thumb(_theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    button::Style {
        background: Some(CONSOLE_DETAIL_CARD.into()),
        border: rounded_border(if selected || matches!(status, button::Status::Hovered) { CONSOLE_DETAIL_ACCENT } else { Color::TRANSPARENT }, 2.0, 12.0),
        text_color: TEXT_1,
        ..Default::default()
    }
}

pub fn console_tab_underline(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(TEXT_1.into()),
        border: Border::default().rounded(2),
        ..container::Style::default()
    }
}

/// Card do trilho: o foco ganha a moldura branca do `retro-library-card`.
pub fn console_card(
    _theme: &Theme,
    status: button::Status,
    selected: bool,
    focused: bool,
    glow: f32,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    // O card não desenha fundo em NENHUM estado, nem selecionado.
    //
    // A arte da capa é recortada com raio 12; qualquer preenchimento atrás
    // dela (mesmo `rgba8(13,16,21,0.92)`, que é quase preto) aparece na quina
    // do recorte como um fio claro — era essa a linha branca em volta da
    // caixa. Fundo aqui só existe se for invisível.
    let background = if hovered {
        Color::from_rgba(1.0, 1.0, 1.0, 0.04)
    } else {
        Color::TRANSPARENT
    };

    // Nenhuma borda, em nenhum estado.
    //
    // Este card é a própria arte da capa: qualquer contorno — mesmo 1 px de
    // branco a 30% — desenha um risco em volta da imagem que corta a sangria
    // e aparece como moldura. A seleção se lê pelo tamanho (o card em foco é
    // 8% maior) e pela opacidade do título, que são mudanças de conteúdo, não
    // de moldura.
    button::Style {
        background: Some(background.into()),
        text_color: TEXT_1,
        border: rounded_border(Color::TRANSPARENT, 0.0, 8.0),
        // Sombra só quando há algo a destacar: no foco (o card cresce) ou no
        // card selecionado. Em repouso, zero — uma sombra difusa de 24 px
        // também desenha um halo visível sobre a arte escura.
        shadow: if focused || selected {
            Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.38 + 0.12 * glow.clamp(0.0, 1.0)),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 20.0,
            }
        } else {
            Shadow::default()
        },
        ..button::Style::default()
    }
}

/// Botão principal "Jogar": pílula branca do `ps5-overview-play`.
pub fn console_play(
    _theme: &Theme,
    status: button::Status,
    focused: bool,
    glow: f32,
) -> button::Style {
    let background = match status {
        button::Status::Hovered => Color::from_rgb8(0xe6, 0xe6, 0xe6),
        button::Status::Pressed => Color::from_rgb8(0xcf, 0xcf, 0xcf),
        button::Status::Disabled => SURFACE_3,
        button::Status::Active => TEXT_1,
    };

    button::Style {
        background: Some(background.into()),
        text_color: BACKGROUND,
        border: Border {
            color: if focused {
                Color::from_rgba(
                    0x22 as f32 / 255.0,
                    0xd3 as f32 / 255.0,
                    0xee as f32 / 255.0,
                    0.55 + 0.45 * glow.clamp(0.0, 1.0),
                )
            } else {
                TRANSPARENT
            },
            width: if focused { 2.0 } else { 0.0 },
            radius: 26.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 22.0,
        },
        ..button::Style::default()
    }
}

/// Botões redondos do painel (♥ e •••); reservado para ações compactas.
#[allow(dead_code)]
pub fn console_round(
    _theme: &Theme,
    status: button::Status,
    focused: bool,
    glow: f32,
) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Color::from_rgba(1.0, 1.0, 1.0, 0.24),
        _ => Color::from_rgba(1.0, 1.0, 1.0, 0.12),
    };

    button::Style {
        background: Some(background.into()),
        text_color: TEXT_1,
        border: Border {
            color: if focused {
                Color::from_rgba(1.0, 1.0, 1.0, 0.62 + 0.28 * glow.clamp(0.0, 1.0))
            } else {
                TRANSPARENT
            },
            width: if focused { 2.0 } else { 0.0 },
            radius: 26.0.into(),
        },
        ..button::Style::default()
    }
}

/// Ação secundária do painel (Trailer), com foco visível.
pub fn console_action(
    _theme: &Theme,
    status: button::Status,
    focused: bool,
    glow: f32,
) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => {
            (Color::from_rgba(1.0, 1.0, 1.0, 0.18), TEXT_1)
        }
        _ => (Color::from_rgba(1.0, 1.0, 1.0, 0.08), TEXT_STRONG),
    };

    button::Style {
        background: Some(background.into()),
        text_color,
        border: rounded_border(
            if focused {
                Color::from_rgba(1.0, 1.0, 1.0, 0.62 + 0.28 * glow.clamp(0.0, 1.0))
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.18)
            },
            2.0,
            22.0,
        ),
        ..button::Style::default()
    }
}

// Vitrine da Loja Big Picture: mesmas cores Arcadia; geometria da referência.
pub const STOREFRONT_INSET: f32 = 20.0;
pub const STOREFRONT_GAP: f32 = 12.0;
pub const STOREFRONT_RADIUS: f32 = 12.0;
pub const STOREFRONT_HERO: f32 = 244.0;
pub const STOREFRONT_CATEGORY_HEIGHT: f32 = 64.0;
pub const STOREFRONT_FOOTER: f32 = 42.0;
pub const STOREFRONT_ART_HEIGHT: f32 = 112.0;

pub fn storefront_panel(_theme: &Theme) -> container::Style {
    container::Style { background: Some(HOME_CARD_BG.into()), border: Border::default().rounded(STOREFRONT_RADIUS), text_color: Some(TEXT_1), ..Default::default() }
}

pub fn storefront_offer(_theme: &Theme) -> container::Style {
    container::Style { background: Some(ACCENT_SOFT.into()), border: rounded_border(ACCENT_BORDER, 1.0, STOREFRONT_RADIUS), text_color: Some(TEXT_1), ..Default::default() }
}

pub fn storefront_control(_theme: &Theme, status: button::Status, active: bool, focused: bool) -> button::Style {
    let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(if active { ACCENT_SOFT } else if hover { SURFACE_3 } else { HOME_CARD_BG }.into()),
        text_color: if active { ACCENT } else { TEXT_1 },
        border: rounded_border(if focused { ACCENT } else if active { ACCENT_BORDER } else { TRANSPARENT }, if focused { 2.0 } else { 1.0 }, STOREFRONT_RADIUS),
        ..Default::default()
    }
}

pub fn storefront_pill(theme: &Theme, status: button::Status, active: bool, focused: bool, glow: f32) -> button::Style {
    let mut style = console_pill_item(theme, status, active, focused, glow);
    if active { style.background = Some(ACCENT.into()); style.text_color = BACKGROUND; }
    if focused { style.border.color = ACCENT; }
    style
}

pub fn storefront_scrim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(gradient::Linear::new(iced::Radians(0.0))
            .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.92))
            .add_stop(0.48, Color::from_rgba(0.0, 0.0, 0.0, 0.55))
            .add_stop(1.0, Color::TRANSPARENT).into()),
        border: Border::default().rounded(STOREFRONT_RADIUS),
        ..Default::default()
    }
}

/// Card da grade da Loja no console (foco com halo e seleção persistente).
pub fn console_store_card(
    _theme: &Theme,
    status: button::Status,
    selected: bool,
    focused: bool,
    glow: f32,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let background = if selected {
        Color::from_rgba8(13, 16, 21, 0.96)
    } else if hovered {
        Color::from_rgb8(0x16, 0x16, 0x1b)
    } else {
        Color::from_rgba(0.0, 0.0, 0.0, 0.55)
    };

    button::Style {
        background: Some(background.into()),
        text_color: TEXT_1,
        border: rounded_border(
            if focused {
                Color::from_rgba(1.0, 1.0, 1.0, 0.58 + 0.24 * glow.clamp(0.0, 1.0))
            } else if selected {
                Color::from_rgba(0.133, 0.827, 0.933, 0.55)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.10)
            },
            if focused { 2.0 } else { 1.0 },
            8.0,
        ),
        ..button::Style::default()
    }
}

pub fn console_scrollable(
    theme: &Theme,
    status: scrollable_widget::Status,
) -> scrollable_widget::Style {
    let mut style = scrollable(theme, status);
    let rail = scrollable_widget::Rail {
        background: Some(TRANSPARENT.into()),
        border: Border::default(),
        scroller: scrollable_widget::Scroller {
            background: Color::from_rgba(1.0, 1.0, 1.0, 0.22).into(),
            border: Border::default().rounded(2),
        },
    };

    style.vertical_rail = rail;
    style.horizontal_rail = rail;
    style
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

/// Busca da Loja no console: o anel de foco acompanha o gamepad/teclado mesmo
/// antes de o `text_input` receber o foco interno.
pub fn console_search_input(
    _theme: &Theme,
    status: text_input::Status,
    zone_focused: bool,
    editing: bool,
    glow: f32,
) -> text_input::Style {
    let mut style = search_input(_theme, status);
    let hovered = matches!(
        status,
        text_input::Status::Focused { .. } | text_input::Status::Hovered
    );

    if zone_focused {
        let alpha = if editing {
            0.75 + 0.25 * glow.clamp(0.0, 1.0)
        } else {
            0.52 + 0.20 * glow.clamp(0.0, 1.0)
        };
        style.border = rounded_border(Color::from_rgba(0.133, 0.827, 0.933, alpha), 2.0, 8.0);
        style.background = Color::from_rgba(1.0, 1.0, 1.0, 0.07).into();
    } else if !hovered {
        style.border = rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.12), 1.0, 8.0);
    }

    style
}

pub fn sidebar_search_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = search_input(_theme, status);
    style.background = Color::from_rgba(1.0, 1.0, 1.0, 0.025).into();
    style.border = rounded_border(Color::from_rgba8(0x22, 0xd3, 0xee, 0.18), 1.0, 2.0);
    style
}

/// Scrollable sem trilho visível: usado no strip de miniaturas e na base do
/// player, onde qualquer faixa cinza aparece como uma "caixa" indesejada.
pub fn scrollable_bare(
    theme: &Theme,
    status: scrollable_widget::Status,
) -> scrollable_widget::Style {
    let mut style = scrollable_widget::default(theme, status);
    let rail = scrollable_widget::Rail {
        background: Some(TRANSPARENT.into()),
        border: Border::default(),
        scroller: scrollable_widget::Scroller {
            background: TRANSPARENT.into(),
            border: Border::default().rounded(3),
        },
    };
    style.vertical_rail = rail;
    style.horizontal_rail = rail;
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

// ── Editor de perfil (porte do `EditProfile.tsx`) ───────────────────────────
//
// A página do editor não é o shell "retro" de 3 px: a referência é a tela
// cheia do `EditProfile.tsx` da web, com cards de 12 px, abas em pill e
// controles de 8 px. Os estilos abaixo são locais a essa tela para não mexer
// na geometria curta do resto do launcher.

/// Raio dos cards do editor (`rounded-2xl`).
pub const EDIT_CARD_RADIUS: f32 = 12.0;
/// Raio de inputs, botões e molduras de prévia (`rounded-lg`/`rounded-xl`).
pub const EDIT_CONTROL_RADIUS: f32 = 8.0;
/// Largura da coluna de prévia do perfil na aba Geral (`380px` da web).
pub const EDIT_PREVIEW_WIDTH: f32 = 360.0;

/// Fundo da página: o preto do shell com um leve roxo por cima, como o
/// `retro-edit-profile` da web (que é quase preto).
pub fn edit_shell(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgb8(0x06, 0x05, 0x0a).into()),
        ..container::Style::default()
    }
}

/// Faixa do cabeçalho (voltar + identidade + abas).
pub fn edit_header(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgba8(0x05, 0x05, 0x09, 0.92).into()),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.07),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// Card do conteúdo (`glass-1 rounded-2xl` do editor).
pub fn edit_card(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_2),
        background: Some(Color::from_rgba8(0x0a, 0x0a, 0x0e, 1.0).into()),
        border: rounded_border(
            Color::from_rgba(1.0, 1.0, 1.0, 0.075),
            1.0,
            EDIT_CARD_RADIUS,
        ),
        ..container::Style::default()
    }
}

/// Sub-card dentro de um card (bloco de requisitos).
pub fn edit_subcard(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_2),
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.035).into()),
        border: rounded_border(
            Color::from_rgba(1.0, 1.0, 1.0, 0.06),
            1.0,
            EDIT_CONTROL_RADIUS,
        ),
        ..container::Style::default()
    }
}

/// Moldura da prévia de imagem (avatar/banner/fundo). O anel ciano marca
/// "tem imagem" como o contorno de foco da referência.
pub fn edit_frame(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba8(0x0b, 0x0b, 0x10, 1.0).into()),
        border: rounded_border(
            Color::from_rgba8(0x22, 0xd3, 0xee, 0.55),
            1.0,
            EDIT_CONTROL_RADIUS,
        ),
        ..container::Style::default()
    }
}

/// Moldura da prévia vazia (sem imagem): só o traço neutro.
pub fn edit_frame_empty(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(
            gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_4))
                .add_stop(0.0, Color::from_rgba8(0x12, 0x14, 0x1b, 1.0))
                .add_stop(1.0, Color::from_rgba8(0x05, 0x05, 0x08, 1.0))
                .into(),
        ),
        border: rounded_border(
            Color::from_rgba(1.0, 1.0, 1.0, 0.10),
            1.0,
            EDIT_CONTROL_RADIUS,
        ),
        ..container::Style::default()
    }
}

/// Aba horizontal do topo. Ativa: borda e fundo ciano translúcidos.
pub fn edit_tab(_theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

    if active {
        return button::Style {
            background: Some(Color::from_rgba8(0x22, 0xd3, 0xee, 0.14).into()),
            text_color: TEXT_1,
            border: rounded_border(Color::from_rgba8(0x22, 0xd3, 0xee, 0.58), 1.0, 10.0),
            shadow: Shadow {
                color: Color::from_rgba8(0x22, 0xd3, 0xee, 0.16),
                offset: Vector::new(0.0, 3.0),
                blur_radius: 14.0,
            },
            ..button::Style::default()
        };
    }

    button::Style {
        background: if hovered {
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.05).into())
        } else {
            None
        },
        text_color: if hovered { TEXT_1 } else { TEXT_2 },
        border: rounded_border(Color::from_rgba(0.0, 0.0, 0.0, 0.0), 1.0, 10.0),
        ..button::Style::default()
    }
}

/// Traço sob a aba ativa (a barrinha ciano da referência).
pub fn edit_indicator(active: bool) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(if active {
            gradient::Linear::new(iced::Radians(0.0))
                .add_stop(0.0, ACCENT)
                .add_stop(1.0, Color::from_rgba8(0x8b, 0x5c, 0xf6, 1.0))
                .into()
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.0).into()
        }),
        border: Border::default().rounded(2),
        ..container::Style::default()
    }
}

/// Pill "‹ Voltar ao perfil".
pub fn edit_back(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

    button::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, if hovered { 0.07 } else { 0.03 }).into()),
        text_color: if hovered { TEXT_1 } else { TEXT_2 },
        border: rounded_border(
            if hovered {
                Color::from_rgba(1.0, 1.0, 1.0, 0.28)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.14)
            },
            1.0,
            10.0,
        ),
        ..button::Style::default()
    }
}

/// Aro do avatar do cabeçalho (2 px ciano + brilho).
pub fn edit_avatar_ring(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba8(0x0a, 0x0a, 0x0e, 1.0).into()),
        border: rounded_border(Color::from_rgba8(0x22, 0xd3, 0xee, 0.85), 2.0, 10.0),
        shadow: Shadow {
            color: Color::from_rgba8(0x22, 0xd3, 0xee, 0.28),
            offset: Vector::new(0.0, 0.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}

/// Botão primário do editor: ciano cheio, texto branco (o "Choose image / GIF"
/// da referência).
pub fn edit_primary(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Color::from_rgb8(0x2f, 0xcf, 0xe6),
        button::Status::Pressed => Color::from_rgb8(0x0d, 0x9c, 0xb4),
        button::Status::Disabled => SURFACE_3,
        button::Status::Active => Color::from_rgb8(0x17, 0xb8, 0xd4),
    };

    button::Style {
        background: Some(background.into()),
        text_color: if matches!(status, button::Status::Disabled) {
            TEXT_3
        } else {
            TEXT_1
        },
        border: rounded_border(
            Color::from_rgba(0.0, 0.0, 0.0, 0.0),
            0.0,
            EDIT_CONTROL_RADIUS,
        ),
        shadow: Shadow {
            color: Color::from_rgba8(0x22, 0xd3, 0xee, 0.22),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..button::Style::default()
    }
}

/// Botão secundário do editor ("Remover", "Cancelar").
pub fn edit_ghost(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

    button::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, if hovered { 0.07 } else { 0.02 }).into()),
        text_color: if hovered { TEXT_1 } else { TEXT_STRONG },
        border: rounded_border(
            if hovered {
                Color::from_rgba(1.0, 1.0, 1.0, 0.30)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.13)
            },
            1.0,
            EDIT_CONTROL_RADIUS,
        ),
        ..button::Style::default()
    }
}

/// Campo de texto do editor: fundo branco/6%, borda neutra e anel ciano no foco.
pub fn edit_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    let hovered = matches!(status, text_input::Status::Hovered);

    text_input::Style {
        background: Color::from_rgba(1.0, 1.0, 1.0, if focused { 0.065 } else { 0.045 }).into(),
        border: rounded_border(
            if focused {
                Color::from_rgba8(0x22, 0xd3, 0xee, 0.80)
            } else if hovered {
                Color::from_rgba(1.0, 1.0, 1.0, 0.20)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.12)
            },
            if focused { 1.0 } else { 1.0 },
            EDIT_CONTROL_RADIUS,
        ),
        icon: Color::from_rgba(1.0, 1.0, 1.0, 0.40),
        placeholder: Color::from_rgba(1.0, 1.0, 1.0, 0.32),
        value: TEXT_1,
        selection: ACCENT_SOFT,
    }
}

/// `pick_list` do editor (país): mesmo desenho do input.
pub fn edit_pick_list(_theme: &Theme, status: pick_list_widget::Status) -> pick_list_widget::Style {
    let focused = matches!(
        status,
        pick_list_widget::Status::Hovered | pick_list_widget::Status::Opened { .. }
    );

    pick_list_widget::Style {
        text_color: TEXT_1,
        placeholder_color: Color::from_rgba(1.0, 1.0, 1.0, 0.32),
        handle_color: TEXT_2,
        background: Color::from_rgba(1.0, 1.0, 1.0, if focused { 0.065 } else { 0.045 }).into(),
        border: rounded_border(
            if focused {
                Color::from_rgba8(0x22, 0xd3, 0xee, 0.80)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.12)
            },
            1.0,
            EDIT_CONTROL_RADIUS,
        ),
    }
}

/// Rótulo em caixa alta sobre o campo ("PROFILE NAME").
pub fn edit_label_color() -> Color {
    Color::from_rgba8(0xb8, 0xc0, 0xce, 1.0)
}

/// Pill de status no rodapé ("Sincronizado"/"Salvo").
pub fn edit_status_pill(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(SUCCESS),
        background: Some(Color::from_rgba8(0x0d, 0x14, 0x12, 0.92).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.10), 1.0, 999.0),
        ..container::Style::default()
    }
}

/// Véu por cima da prévia de banner: o nome do perfil precisa de contraste.
pub fn edit_preview_scrim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(
            gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
                .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.0))
                .add_stop(0.6, Color::from_rgba(0.0, 0.0, 0.0, 0.18))
                .add_stop(1.0, Color::from_rgba(0.0, 0.0, 0.0, 0.62))
                .into(),
        ),
        ..container::Style::default()
    }
}

/// Quadradinho com o "i" dos requisitos do avatar.
pub fn edit_badge(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(ACCENT),
        background: Some(Color::from_rgba8(0x22, 0xd3, 0xee, 0.14).into()),
        border: rounded_border(Color::from_rgba8(0x22, 0xd3, 0xee, 0.34), 1.0, 9.0),
        ..container::Style::default()
    }
}

/// Fio de 1 px entre blocos de um mesmo card.
pub fn edit_hairline(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.08).into()),
        ..container::Style::default()
    }
}

// ── Diálogo de opções de inicialização (estilos locais) ─────────────────────
//
// O modal não usa a geometria curta de 3 px do shell: é uma superfície
// grafite própria, com cantos de 12–16 px e o ciano só no modo selecionado.
// Nada aqui altera os tokens/estilos globais.

/// Superfície principal do diálogo.
pub fn launch_dialog(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgb8(0x0f, 0x10, 0x14).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.12), 1.0, 16.0),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.58),
            offset: Vector::new(0.0, 18.0),
            blur_radius: 42.0,
        },
        ..container::Style::default()
    }
}

/// Botão quadrado do "✕" no cabeçalho: discreto até o hover.
pub fn launch_icon_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => {
            (Some(Color::from_rgba(1.0, 1.0, 1.0, 0.08).into()), TEXT_1)
        }
        _ => (None, TEXT_2),
    };

    button::Style {
        background,
        text_color,
        border: Border::default().rounded(8),
        ..button::Style::default()
    }
}

/// Balão do tooltip do "✕".
pub fn launch_tooltip(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT_1),
        background: Some(Color::from_rgb8(0x1c, 0x1e, 0x25).into()),
        border: rounded_border(Color::from_rgba(1.0, 1.0, 1.0, 0.12), 1.0, 8.0),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}

/// Cartão grande de modo (Automático/Steam/Executável). O contorno ciano e o
/// fundo levemente tingido marcam a seleção; os indisponíveis ficam apagados.
pub fn launch_mode_card(
    _theme: &Theme,
    status: button::Status,
    active: bool,
    enabled: bool,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let (background, border_color) = if !enabled {
        (
            Color::from_rgba(1.0, 1.0, 1.0, 0.015),
            Color::from_rgba(1.0, 1.0, 1.0, 0.06),
        )
    } else if active {
        (
            Color::from_rgba8(0x22, 0xd3, 0xee, if hovered { 0.20 } else { 0.14 }),
            Color::from_rgba8(0x22, 0xd3, 0xee, 0.62),
        )
    } else if hovered {
        (
            Color::from_rgba(1.0, 1.0, 1.0, 0.055),
            Color::from_rgba(1.0, 1.0, 1.0, 0.22),
        )
    } else {
        (
            Color::from_rgba(1.0, 1.0, 1.0, 0.028),
            Color::from_rgba(1.0, 1.0, 1.0, 0.10),
        )
    };

    button::Style {
        background: Some(background.into()),
        text_color: if enabled { TEXT_1 } else { TEXT_3 },
        border: rounded_border(border_color, 1.0, 12.0),
        ..button::Style::default()
    }
}

/// Linha clicável que abre/fecha os ajustes avançados.
pub fn launch_advanced_toggle(
    _theme: &Theme,
    status: button::Status,
    open: bool,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let (background, border_color) = if open {
        (
            Color::from_rgba8(0x22, 0xd3, 0xee, if hovered { 0.10 } else { 0.07 }),
            ACCENT_BORDER,
        )
    } else if hovered {
        (
            Color::from_rgba(1.0, 1.0, 1.0, 0.055),
            Color::from_rgba(1.0, 1.0, 1.0, 0.20),
        )
    } else {
        (
            Color::from_rgba(1.0, 1.0, 1.0, 0.028),
            Color::from_rgba(1.0, 1.0, 1.0, 0.10),
        )
    };

    button::Style {
        background: Some(background.into()),
        text_color: TEXT_1,
        border: rounded_border(border_color, 1.0, 12.0),
        ..button::Style::default()
    }
}

/// Interruptor em pílula dos ajustes avançados: tingido de ciano quando ligado.
pub fn launch_toggle_button(
    _theme: &Theme,
    status: button::Status,
    enabled: bool,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let (background, border_color, text_color) = if enabled {
        (
            Color::from_rgba8(0x22, 0xd3, 0xee, if hovered { 0.20 } else { 0.14 }),
            Color::from_rgba8(0x22, 0xd3, 0xee, 0.52),
            TEXT_1,
        )
    } else if hovered {
        (
            Color::from_rgba(1.0, 1.0, 1.0, 0.075),
            Color::from_rgba(1.0, 1.0, 1.0, 0.24),
            TEXT_1,
        )
    } else {
        (
            Color::from_rgba(1.0, 1.0, 1.0, 0.03),
            Color::from_rgba(1.0, 1.0, 1.0, 0.10),
            TEXT_2,
        )
    };

    button::Style {
        background: Some(background.into()),
        text_color,
        border: rounded_border(border_color, 1.0, 8.0),
        ..button::Style::default()
    }
}

/// Ação principal do rodapé, com o raio do diálogo (sem tocar no botão global).
pub fn launch_primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Color::from_rgb8(0x67, 0xe8, 0xf9),
        button::Status::Pressed => Color::from_rgb8(0x0e, 0xa5, 0xbd),
        button::Status::Disabled => SURFACE_3,
        button::Status::Active => ACCENT,
    };

    button::Style {
        background: Some(background.into()),
        text_color: if matches!(status, button::Status::Disabled) {
            TEXT_3
        } else {
            BACKGROUND
        },
        border: Border::default().rounded(8),
        ..button::Style::default()
    }
}

/// Ação secundária do rodapé (Cancelar), no mesmo raio dos campos.
pub fn launch_secondary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.07).into()),
            Color::from_rgba(1.0, 1.0, 1.0, 0.26),
            TEXT_1,
        ),
        button::Status::Disabled => (None, BORDER, TEXT_3),
        button::Status::Active => (
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.03).into()),
            Color::from_rgba(1.0, 1.0, 1.0, 0.14),
            Color::from_rgba(1.0, 1.0, 1.0, 0.78),
        ),
    };

    button::Style {
        background,
        text_color,
        border: rounded_border(border_color, 1.0, 8.0),
        ..button::Style::default()
    }
}

/// Campo do diálogo: mesmo vidro do `search_input`, com raio de 8 px.
pub fn launch_input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = search_input(theme, status);
    style.border.radius = 8.0.into();
    style
}
