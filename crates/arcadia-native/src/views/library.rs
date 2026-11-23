//! Biblioteca: mesma top bar de busca, cabeçalho e grade `minmax(170px, 1fr)`
//! da `LibraryView.tsx`, com capa 2:3, título e horas.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space, Stack};
use iced::{Element, Fill, Padding};

use crate::app::{App, LibraryStatus, LibraryTab, Message};
use crate::i18n::I18n;
use crate::theme;
use crate::views::cards;
use crate::views::game_menu::{self, VectorIcon};
use crate::views::icons::{self, Icon};

pub fn view(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let games = app.library.visible_games(&app.search);

    // Agrupamento por plataforma, como o Orbit separa Steam / Retro / Xbox.
    let groups = app.library.grouped_games(&app.search);

    let search = Stack::new()
        .width(Fill)
        .push(
            text_input(i18n.t("library.buscar"), &app.search)
                .on_input(Message::SearchChanged)
                .padding(Padding {
                    top: 10.0,
                    right: 14.0,
                    bottom: 10.0,
                    left: 38.0,
                })
                .size(theme::TEXT_READ)
                .width(Fill)
                .style(theme::search_input),
        )
        .push(
            container(icons::icon(Icon::Search, 16.0, 0.40))
                .width(Fill)
                .height(Fill)
                .align_x(iced::Left)
                .align_y(iced::Center)
                .padding([0.0, 14.0]),
        );

    let search_row = container(search).width(Fill).padding(Padding {
        top: 24.0,
        right: theme::PAGE_PADDING,
        bottom: 16.0,
        left: theme::PAGE_PADDING,
    });

    let tabs = container(tab_row(app)).width(Fill).padding(Padding {
        top: 0.0,
        right: theme::PAGE_PADDING,
        bottom: 16.0,
        left: theme::PAGE_PADDING,
    });

    let heading = container(cards::section_heading(
        if app.library.tab == LibraryTab::Retro {
            i18n.t("library.retro")
        } else {
            i18n.t("library.todos_jogos")
        },
        Some(games.len()),
        None,
    ))
    .width(Fill)
    .padding([0.0, theme::PAGE_PADDING]);

    let body: Element<'_, Message> = match &app.library.status {
        LibraryStatus::Loading => centered_message(i18n.t("common.carregando")),
        LibraryStatus::Error(error) => error_state(error, i18n),
        LibraryStatus::Ready if app.library.games.is_empty() => {
            centered_message(i18n.t("ps5.biblioteca.vazia"))
        }
        LibraryStatus::Ready if games.is_empty() => {
            centered_message(if app.library.tab == LibraryTab::Retro {
                i18n.t("library.retro_vazio")
            } else {
                i18n.t("library.vazio")
            })
        }
        LibraryStatus::Ready => {
            // A Library agrupa por plataforma, como o Orbit separa
            // `Steam / Retro / Xbox`. Um cabeçalho por grupo com a contagem, e
            // a grade de sempre embaixo — o usuário acha o jogo pela
            // plataforma antes de procurar pelo título.
            let mut sections: Vec<Element<'_, Message>> = Vec::new();

            for (index, (label, group)) in groups.iter().enumerate() {
                let heading =
                    cards::section_heading(platform_title(i18n, label), Some(group.len()), None);
                sections.push(
                    container(heading)
                        .width(Fill)
                        .padding(Padding {
                            top: if index == 0 { 0.0 } else { 22.0 },
                            right: 0.0,
                            bottom: 10.0,
                            left: 0.0,
                        })
                        .into(),
                );

                let cards: Vec<Element<'_, Message>> = group
                    .iter()
                    .map(|game| {
                        game_menu::right_click(
                            cards::game_card(&app.i18n, game, &app.covers),
                            game.id.clone(),
                        )
                    })
                    .collect();
                sections.push(cards::grid(cards, (2.0, 3.55)).into());
            }

            scrollable(
                container(column(sections).spacing(0))
                    .width(Fill)
                    .padding(Padding {
                        top: 8.0,
                        right: theme::PAGE_PADDING,
                        bottom: theme::PAGE_PADDING,
                        left: theme::PAGE_PADDING,
                    }),
            )
            .height(Fill)
            .style(theme::scrollable)
            .into()
        }
    };

    // O cabeçalho geral some quando a lista está agrupada: cada grupo já tem
    // o próprio título e a contagem, e dois níveis de cabeçalho empurram a
    // primeira fileira para fora da tela.
    //
    // `heading`, `search_row` e `tabs` são widgets (não `Copy`), então cada
    // ramo é montado uma vez só — construir o layout e depois remontá-lo
    // move os valores e não compila.
    let notice = app
        .library
        .notice
        .as_deref()
        .map(|message| library_notice(i18n, message));

    if matches!(app.library.status, LibraryStatus::Ready) {
        let mut page = column![search_row, tabs];
        if let Some(notice) = notice {
            page = page.push(notice);
        }
        page.push(Space::new().height(8))
            .push(body)
            .width(Fill)
            .height(Fill)
            .into()
    } else {
        let mut page = column![search_row, tabs, heading];
        if let Some(notice) = notice {
            page = page.push(notice);
        }
        page.push(Space::new().height(12))
            .push(body)
            .width(Fill)
            .height(Fill)
            .into()
    }
}

fn library_notice<'a>(i18n: &'a I18n, message: &'a str) -> Element<'a, Message> {
    container(
        row![
            text(message)
                .size(theme::TEXT_CAPTION)
                .color(theme::DANGER)
                .width(Fill),
            button(text(i18n.t("common.fechar")).size(theme::TEXT_MICRO))
                .on_press(Message::DismissLibraryNotice)
                .padding([5.0, 9.0])
                .style(theme::ghost_button),
        ]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding([8.0, theme::PAGE_PADDING])
    .style(theme::error_panel)
    .into()
}

/// Nome de exibição de um grupo de plataforma.
fn platform_title(i18n: &crate::i18n::I18n, label: &str) -> String {
    match label {
        "steam" => "Steam".to_string(),
        "retro" => i18n.t("library.retro").to_string(),
        "epic" => "Epic Games".to_string(),
        "gog" => "GOG".to_string(),
        "xbox" => "Xbox".to_string(),
        "emulador" => "Emuladores".to_string(),
        "" => i18n.t("library.todos_jogos").to_string(),
        other => {
            // Plataforma sem rótulo conhecido aparece capitalizada, nunca
            // escondida: sumir com o jogo seria pior que um nome feio.
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

/// Abas "Todos os jogos" e "Retro", com ícone e estado ativo — o `tablist` da
/// `LibraryView.tsx`.
fn tab_row(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let hidden_active = app.library.show_hidden;
    row![
        library_tab(
            i18n.t("library.todos_jogos"),
            Icon::Grid,
            LibraryTab::All,
            app.library.tab
        ),
        library_tab(
            i18n.t("library.retro"),
            Icon::Gamepad,
            LibraryTab::Retro,
            app.library.tab
        ),
        Space::new().width(Fill),
        hidden_filter(i18n.t("library.ocultos"), hidden_active),
    ]
    .spacing(4)
    .width(Fill)
    .into()
}

/// Filtro de recuperação: os ocultos voltam à grade para poderem ser
/// reexibidos pelo menu de contexto.
fn hidden_filter(label: &str, active: bool) -> Element<'_, Message> {
    let color = if active { 0.92 } else { 0.50 };
    button(
        row![
            game_menu::vector_icon(VectorIcon::Eye, 14.0, color),
            text(label)
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_MEDIUM)
                .color(if active { theme::TEXT_1 } else { theme::TEXT_2 }),
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center),
    )
    .on_press(Message::LibraryShowHiddenToggled)
    .padding([8.0, 12.0])
    .style(if active {
        theme::library_tab_active
    } else {
        theme::library_tab
    })
    .into()
}

/// Um botão de aba da Biblioteca.
fn library_tab<'a>(
    label: &'a str,
    icon: Icon,
    tab: LibraryTab,
    active: LibraryTab,
) -> Element<'a, Message> {
    let selected = active == tab;
    let color = if selected { 0.92 } else { 0.50 };
    button(
        row![
            icons::icon(icon, 14.0, color),
            text(label)
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_MEDIUM)
                .color(if selected {
                    theme::TEXT_1
                } else {
                    theme::TEXT_2
                }),
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center),
    )
    .on_press(Message::LibraryTabSelected(tab))
    .padding([8.0, 14.0])
    .style(if selected {
        theme::library_tab_active
    } else {
        theme::library_tab
    })
    .into()
}

fn centered_message(message: &str) -> Element<'_, Message> {
    container(
        container(
            text(message)
                .size(theme::TEXT_READ)
                .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.35)),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .padding(theme::PAGE_PADDING),
    )
    .width(Fill)
    .height(Fill)
    .into()
}

fn error_state<'a>(error: &'a str, i18n: &'a I18n) -> Element<'a, Message> {
    container(
        container(
            column![
                text(i18n.t("native.library.load_error"))
                    .size(20)
                    .font(theme::FONT_BOLD)
                    .color(theme::DANGER),
                text(error).size(theme::TEXT_READ).color(theme::TEXT_2),
                row![
                    button(
                        text(i18n.t("downloads.tentar_novamente"))
                            .size(theme::TEXT_READ)
                            .font(theme::FONT_MEDIUM)
                    )
                    .on_press(Message::Reload)
                    .padding([10.0, 18.0])
                    .style(theme::primary_button),
                    Space::new().width(theme::SPACE_2),
                ],
            ]
            .spacing(theme::SPACE_3),
        )
        .width(Fill)
        .max_width(720)
        .padding(theme::SPACE_5)
        .style(theme::error_panel),
    )
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .center_y(Fill)
    .padding(theme::PAGE_PADDING)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Backend, Game};
    use iced::advanced::layout::{Layout, Limits};
    use iced::advanced::renderer::{Headless, Style};
    use iced::advanced::widget::Tree;
    use iced::{mouse, Color, Font, Pixels, Rectangle, Renderer, Size};
    use serde_json::json;

    fn test_app() -> App {
        let data_dir = std::env::temp_dir().join("arcadia-library-render-fixture");
        let backend = Backend::new(
            arcadia_ipc::AppState::new(data_dir),
            tokio::runtime::Handle::current(),
        );
        let (app, boot) = App::boot(backend);
        drop(boot);
        app
    }

    async fn render_library(app: &App) -> ::image::RgbaImage {
        let mut renderer = Renderer::new(Font::DEFAULT, Pixels(14.0), Some("tiny-skia"))
            .await
            .expect("software renderer");
        let window_size = Size::new(720u32, 530u32);
        let size = Size::new(window_size.width as f32, window_size.height as f32);
        let mut library = view(app);
        let mut tree = Tree::new(&library);
        let layout =
            library.as_widget_mut().layout(&mut tree, &renderer, &Limits::new(Size::ZERO, size));
        library.as_widget().draw(
            &tree,
            &mut renderer,
            &theme::arcadia(),
            &Style::default(),
            Layout::new(&layout),
            mouse::Cursor::Unavailable,
            &Rectangle::with_size(size),
        );
        let pixels = renderer.screenshot(window_size, 1.0, Color::BLACK);
        ::image::RgbaImage::from_raw(window_size.width, window_size.height, pixels).unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn library_cover_fills_the_same_card_when_an_old_steam_banner_has_a_portrait() {
        const BANNER: [u8; 4] = [255, 0, 255, 255];
        const RECOVERED: [u8; 4] = [0, 255, 0, 255];
        const STANDARD: [u8; 4] = [0, 255, 255, 255];
        let header_url = "https://cdn.akamai.steamstatic.com/steam/apps/413150/header.jpg";
        let recovered_url = "https://cdn.cloudflare.steamstatic.com/steam/apps/413150/library_600x900.jpg";
        let standard_url = "https://example.test/standard-portrait.jpg";
        let mut app = test_app();
        app.library.ready(vec![
            Game::from_value(json!({
                "id": "steam:413150",
                "title": "Stardew Valley",
                "launcher": "steam",
                "installed": false,
                "pendente": true,
                "cover": header_url
            })).unwrap(),
            Game::from_value(json!({
                "id": "steam:1888930",
                "title": "The Last of Us Part I",
                "launcher": "steam",
                "cover": standard_url
            })).unwrap(),
        ]);
        for (url, width, height, color) in [
            (header_url, 460, 215, BANNER),
            (recovered_url, 300, 450, RECOVERED),
            (standard_url, 300, 450, STANDARD),
        ] {
            let data = ::image::RgbaImage::from_pixel(width, height, ::image::Rgba(color));
            let handle = iced::widget::image::Handle::from_rgba(width, height, data.into_raw());
            app.covers.insert(url, handle, (width, height));
        }

        let rendered = render_library(&app).await;
        let pixel_count = |color| rendered.pixels().filter(|pixel| pixel.0 == color).count();
        let standard_pixels = pixel_count(STANDARD);
        let recovered_pixels = pixel_count(RECOVERED);
        assert!(standard_pixels > 30_000);
        assert!(
            recovered_pixels.abs_diff(standard_pixels) < 500,
            "a capa recuperada ocupou {recovered_pixels} pixels e a capa vertical padrão ocupou {standard_pixels}"
        );
        assert_eq!(pixel_count(BANNER), 0);
        assert!(app.library.image_urls().iter().any(|url| url == recovered_url));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn library_cover_renders_a_saved_horizontal_header_without_zoom_or_stretch() {
        const LEFT: [u8; 4] = [255, 0, 255, 255];
        const CENTER: [u8; 4] = [0, 255, 0, 255];
        const RIGHT: [u8; 4] = [0, 255, 255, 255];
        let mut app = test_app();

        let url = "https://example.test/header.jpg";
        let game = Game::from_value(json!({
            "id": "steam:413150",
            "title": "Header fixture",
            "launcher": "steam",
            "installed": false,
            "pendente": true,
            "cover": url
        }))
        .unwrap();
        app.library.ready(vec![game]);
        let source = Size::new(460u32, 215u32);
        let art = ::image::RgbaImage::from_fn(source.width, source.height, |x, _| {
            let color = if x < source.width / 5 {
                LEFT
            } else if x >= source.width * 4 / 5 {
                RIGHT
            } else {
                CENTER
            };
            ::image::Rgba(color)
        });
        let handle =
            iced::widget::image::Handle::from_rgba(source.width, source.height, art.into_raw());
        app.covers.insert(url, handle, (source.width, source.height));

        let pixel_art = render_library(&app).await;
        let mut colors = [0; 3];
        let (mut min_x, mut min_y) = pixel_art.dimensions();
        let (mut max_x, mut max_y) = (0, 0);
        for (x, y, pixel) in pixel_art.enumerate_pixels() {
            if let Some(band) = [LEFT, CENTER, RIGHT].iter().position(|color| *color == pixel.0) {
                colors[band] += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        assert!(colors.iter().all(|&count| count > 100), "recorte das faixas da capa: {colors:?}");
        let rendered_ratio = (max_x - min_x + 1) as f32 / (max_y - min_y + 1) as f32;
        let source_ratio = source.width as f32 / source.height as f32;
        assert!((rendered_ratio - source_ratio).abs() < 0.1, "proporção renderizada: {rendered_ratio}");
    }
}
