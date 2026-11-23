//! Modo console (Big Picture): hero do jogo focado, top bar de abas, trilho
//! horizontal de capas e painel do jogo.
//!
//! Visual portado do `PS5Launcher`/`HeroSection`/`GameRail`/`GameOverview` da UI
//! web, com as simplificações possíveis no iced: sem blur/backdrop-filter,
//! gradientes viram camadas translúcidas e a capa focada ganha moldura branca
//! em vez de escala animada.

use iced::widget::scrollable::{AbsoluteOffset, Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::Id;
use iced::widget::{
    button, column, container, image, mouse_area, row, scrollable, text, text_input, Space, Stack,
};
use iced::{Color, ContentFit, Element, Fill, Length, Padding, Task};

use crate::app::{App, LibraryStatus, LoadStatus, Message, StoreDetailState};
use crate::backend::{Game, StoreGame};
use crate::big_picture::{
    home_shelf, ConsoleTab, ConsoleZone, ACTION_FAVORITE, ACTION_MORE, ACTION_PLAY, ACTION_TRAILER,
    MENU_COUNT, MENU_FAVORITE, MENU_OPEN_DETAIL, MENU_TRAILER, STORE_GRID_COLS,
};
use crate::media;
use crate::theme;
use crate::views::icons::{self, Icon};

/// Medidas do trilho compartilhadas com o cálculo de scroll.
///
/// Card quadrado do Orbit: a arte é 1:1, então a largura do card é a própria
/// altura da arte. Capa vertical (`148×188`) lê como vitrine de loja; quadrado
/// lê como console de sofá.
///
/// 208 px é a medida que o Orbit usa numa tela 1080p: oito tiles cabem na
/// largura e o card ainda tem presença a três metros. Com 168 o trilho parecia
/// uma lista de ícones em vez de uma fileira de capas.
pub const RAIL_CARD_WIDTH: f32 = 208.0;
pub const RAIL_CARD_GAP: f32 = 18.0;
/// Altura da arte dentro do card (quadrada).
const RAIL_COVER_HEIGHT: f32 = 208.0;
/// Card = arte (208) + título (18) + espaçamento (8).
const RAIL_CARD_HEIGHT: f32 = 234.0;
/// Bloco do trilho = cabeçalho (~54) + respiro (10) + card (234) + folga (14).
const RAIL_HEIGHT: f32 = 314.0;
/// Ganho de escala do card em foco. O Orbit usa `scale(1.04)` na transição;
/// 1.08 lê bem a três metros sem desalinhar o trilho.
const ZOOM_FOCUSED: f32 = 1.08;
const GUTTER: f32 = 56.0;
const RAIL_ID: &str = "arcadia-console-rail";
/// Id do `text_input` da busca da Loja; o App usa para focar via teclado/A.
pub const STORE_SEARCH_ID: &str = "arcadia-console-store-search";

const MUTED: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.62);
const FAINT: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.40);

/// Rola o trilho para manter o jogo focado visível (centralizado quando cabe).
pub fn rail_scroll_task(index: usize, viewport_width: f32) -> Task<Message> {
    let x = (index as f32 * (RAIL_CARD_WIDTH + RAIL_CARD_GAP) + RAIL_CARD_WIDTH / 2.0
        - viewport_width / 2.0)
        .max(0.0);

    iced::advanced::widget::operate(iced::advanced::widget::operation::scrollable::scroll_to(
        iced::widget::Id::new(RAIL_ID),
        AbsoluteOffset {
            x: Some(x),
            y: Some(0.0),
        },
    ))
}

pub fn view(app: &App) -> Element<'_, Message> {
    let tab = app.big_picture.tab();
    let library = app.library.visible_all();
    let (games, has_continue) = if tab == ConsoleTab::Home {
        let (shelf, has_continue) = home_shelf(&library);
        (shelf, has_continue)
    } else {
        (library, false)
    };
    let selected = app.big_picture.selected(&games);

    let base: Element<'_, Message> = if tab == ConsoleTab::Stores {
        column![top_bar(app), super::big_picture_store::view(app)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if tab.has_game_rail() {
        // O trilho fica abaixo do centro, não colado no pé: o bloco do hero
        // ocupa 11 porções da sobra vertical e o respiro embaixo leva 5. Numa
        // tela 1080p isso põe o centro do trilho em ~63% da altura — a faixa
        // que o Orbit usa. Sem o respiro, o `Fill` do hero empurrava o trilho
        // até o rodapé e a tela parecia desequilibrada para baixo.
        column![
            top_bar(app),
            container(copy_area(app, selected))
                .width(Fill)
                .height(Length::FillPortion(11))
                .align_y(iced::Bottom)
                .padding(Padding {
                    top: 0.0,
                    right: GUTTER,
                    bottom: 20.0,
                    left: GUTTER,
                }),
            rail(app, &games, has_continue),
            Space::new().height(Length::FillPortion(5)),
        ]
        .width(Fill)
        .height(Fill)
        .into()
    } else {
        column![
            top_bar(app),
            container(copy_area(app, selected))
                .width(Fill)
                .height(Fill)
                .padding(Padding {
                    top: 0.0,
                    right: GUTTER,
                    bottom: 16.0,
                    left: GUTTER,
                }),
        ]
        .width(Fill)
        .height(Fill)
        .into()
    };

    let mut layers = Stack::new()
        .width(Fill)
        .height(Fill)
        .push(background(app, selected))
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::console_wash),
        )
        // Gradiente inferior em três degraus. O iced não expõe gradiente de
        // container, então a queda de luminância é feita com faixas
        // sobrepostas de alfa crescente — o mesmo resultado visual do véu do
        // Orbit, que escurece o pé da arte para o trilho e os prompts lerem.
        .push(
            container(
                container(Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(theme::console_wash_step_low),
            )
            .width(Fill)
            .height(Fill)
            .align_y(iced::Bottom),
        )
        .push(
            container(
                container(Space::new())
                    .width(Fill)
                    .height(Length::FillPortion(24))
                    .style(theme::console_wash_step_mid),
            )
            .width(Fill)
            .height(Fill)
            .align_y(iced::Bottom),
        )
        .push(
            container(
                container(Space::new())
                    .width(Fill)
                    .height(Length::Fixed(420.0))
                    .style(theme::console_bottom_wash),
            )
            .width(Fill)
            .height(Fill)
            .align_y(iced::Bottom),
        )
        .push(base);

    if app.big_picture.overview_open() && tab.has_game_rail() {
        if let Some(game) = selected {
            layers = layers.push(super::big_picture_detail::view(app, game));
            if app.big_picture.menu_open() {
                layers = layers.push(more_menu(app, game));
            }
        }
    }

    if app.big_picture.store_detail_open() {
        if let Some(detail) = app.store.detail.as_ref() {
            layers = layers.push(store_detail_overlay(app, detail));
        }
    }

    if app.launch_options.open {
        if let Some(game) = app.library.game(&app.launch_options.game_id) {
            layers = layers.push(super::game::launch_options_overlay(
                &app.i18n,
                &app.launch_options,
                game,
            ));
        }
    }

    if let Some(detail) = app.store.detail.as_ref().filter(|detail| detail.downloads.open) {
        layers = layers.push(super::store_downloads::overlay(app, detail));
    }
    layers.into()
}

fn background<'a>(app: &'a App, selected: Option<&'a Game>) -> Element<'a, Message> {
    let url = selected.and_then(art_url);

    match app.covers.ready(url) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            column![
                text("ARCADIA")
                    .size(76)
                    .font(theme::FONT_BOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.10)),
                text(match &app.library.status {
                    LibraryStatus::Loading => app.i18n.t("native.console.library_loading"),
                    LibraryStatus::Error(_) => app.i18n.t("native.console.library_unavailable"),
                    LibraryStatus::Ready => app.i18n.t("native.console.mode"),
                })
                .size(theme::TEXT_READ)
                .color(FAINT),
            ]
            .spacing(8)
            .align_x(iced::Center),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .style(theme::app_background)
        .into(),
    }
}

fn top_bar(app: &App) -> Element<'_, Message> {
    let bp = &app.big_picture;
    let mut tabs = iced::widget::Row::new().spacing(2).align_y(iced::Center);

    // Pílula centralizada do Orbit: os itens ficam dentro de uma cápsula
    // escura flutuante no centro da tela, não alinhados à esquerda. O item
    // ativo ganha fundo claro; o focado (gamepad/teclado) ganha o anel branco.
    for tab in ConsoleTab::ALL {
        let active = bp.tab() == tab;
        let focused = bp.zone() == ConsoleZone::Tabs && bp.tab_index() == tab.index();

        // Container com `align_y` porque o `button` do iced alinha o conteúdo
        // no topo: sem isto o rótulo fica colado em cima e a pílula parece
        // desalinhada, com todo o respiro sobrando embaixo.
        tabs = tabs.push(
            button(
                container(
                    text(app.i18n.t(tab.i18n_key()))
                        .size(theme::TEXT_BODY)
                        .font(if active {
                            theme::FONT_SEMIBOLD
                        } else {
                            theme::FONT
                        })
                        .color(if active && bp.tab() == ConsoleTab::Stores { theme::BACKGROUND } else if active { theme::TEXT_1 } else { MUTED }),
                )
                .height(Fill)
                .align_y(iced::Center),
            )
            .on_press(Message::ConsoleSelectTab(tab))
            .height(Length::Fixed(38.0))
            .padding([0.0, 20.0])
            .style(move |style_theme, status| {
                if bp.tab() == ConsoleTab::Stores {
                    theme::storefront_pill(style_theme, status, active, focused, bp.focus_glow())
                } else { theme::console_pill_item(style_theme, status, active, focused, bp.focus_glow()) }
            }),
        );
    }

    let pill = container(tabs)
        .padding(4)
        .style(theme::console_pill_shell);

    // A direita da top bar ficou só com o relógio do lado esquerdo e a pílula
    // no centro. O status de conexão, o bloco de perfil e o "Sign out" saíram:
    // o modo console é a tela de sofá, e nada ali é acionável com o controle —
    // o perfil vive na Biblioteca e a saída do modo é o botão Guide/B do
    // gamepad ou Esc no teclado, ambos já mapeados.
    //
    // O lado direito continua existindo como `Fill` vazio: é o que mantém a
    // pílula no centro exato da tela, já que os dois lados têm a mesma
    // largura elástica.
    let left = container(
        container(
            text(app.big_picture.clock())
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_MEDIUM)
                .color(MUTED),
        )
        .padding([0.0, 6.0]),
    )
    .width(Fill)
    .align_x(iced::Left);

    let right: Element<'_,Message> = if bp.tab() == ConsoleTab::Stores {
        use super::big_picture_store::{glyph, Glyph};
        let control = |content: Element<'static,Message>, message| button(container(content).width(Fill).height(Fill).align_x(iced::Center).align_y(iced::Center))
            .on_press(message).width(28).height(32).padding(0).style(theme::plain_button);
        container(row![control(icons::icon(Icon::Search,18.0,0.9),Message::ConsoleStoreFocus),
            control(glyph(Glyph::Minimize,15.0,theme::TEXT_STRONG),Message::WindowMinimize),
            control(glyph(Glyph::Maximize,15.0,theme::TEXT_STRONG),Message::WindowToggleMaximize),
            control(glyph(Glyph::Close,15.0,theme::TEXT_STRONG),Message::WindowClose)].spacing(6).align_y(iced::Center)).width(Fill).align_x(iced::Right).into()
    } else {Space::new().width(Fill).into()};
    container(row![left, pill, right].align_y(iced::Center))
        .width(Fill)
        .height(if bp.tab() == ConsoleTab::Stores { 64 } else { 88 })
        .padding([0.0, if bp.tab() == ConsoleTab::Stores { theme::STOREFRONT_INSET } else { GUTTER }])
        .style(theme::console_topbar)
        .into()
}

fn copy_area<'a>(app: &'a App, selected: Option<&'a Game>) -> Element<'a, Message> {
    let tab = app.big_picture.tab();

    if tab == ConsoleTab::News {
        return news_view(app);
    }

    if !tab.has_game_rail() {
        return tab_placeholder(app, tab);
    }

    match (&app.library.status, selected) {
        (LibraryStatus::Loading, _) => empty_panel(
            app.i18n.t("common.carregando"),
            app.i18n.t("native.home.history_loading"),
            Vec::new(),
        ),
        (LibraryStatus::Error(error), _) => {
            let actions = vec![
                button(
                    text(app.i18n.t("downloads.tentar_novamente"))
                        .size(theme::TEXT_READ)
                        .font(theme::FONT_MEDIUM),
                )
                .on_press(Message::Reload)
                .padding([10.0, 18.0])
                .style(theme::primary_button)
                .into(),
                exit_button(app),
            ];
            container(
                column![
                    text(app.i18n.t("native.library.load_error"))
                        .size(24)
                        .font(theme::FONT_BOLD)
                        .color(theme::DANGER),
                    text(error.as_str())
                        .size(theme::TEXT_READ)
                        .color(theme::TEXT_2)
                        .wrapping(Wrapping::WordOrGlyph),
                    row(actions).spacing(12),
                ]
                .spacing(14)
                .max_width(720),
            )
            .padding(24)
            .style(theme::error_panel)
            .into()
        }
        (_, Some(game)) => hero_copy(app, game, tab),
        (LibraryStatus::Ready, None) => empty_panel(
            app.i18n.t("native.console.home.empty_title"),
            app.i18n.t("native.console.home.empty_desc"),
            vec![exit_button(app)],
        ),
    }
}

fn hero_copy<'a>(app: &'a App, game: &'a Game, tab: ConsoleTab) -> Element<'a, Message> {
    let i18n = &app.i18n;
    // A etiqueta de contexto ("EM DESTAQUE" / "Featured") saiu: o bloco abre
    // direto na logo do jogo. Uma palavra em caixa alta acima da arte só
    // empurrava o título para baixo sem dizer nada que o olho já não soubesse.
    let _ = tab;

    // Logo transparente do jogo quando existe: substitui o título em texto
    // pelo wordmark desenhado, que é a assinatura visual do hero do Orbit.
    // Sem logo (jogo fora da Steam), o título em texto continua — nunca um
    // espaço vazio no lugar da identidade.
    let logo = crate::backend::game_logo_url(game);
    let title: Element<'_, Message> = match app.covers.ready(logo.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(Length::Fixed(380.0))
            .height(Length::Fixed(120.0))
            .content_fit(ContentFit::Contain)
            .into(),
        None => text(game.title.clone())
            .size(52)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1)
            .wrapping(Wrapping::WordOrGlyph)
            .into(),
    };

    // Descrição curta: o Orbit mostra um parágrafo de sinopse aqui, cortado em
    // duas ou três linhas. É o que dá peso ao hero — sem ela o bloco vira uma
    // ficha técnica e o olho não tem o que ler.
    let description = game
        .description
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    let mut copy = column![title].spacing(12);

    if let Some(description) = description {
        copy = copy.push(
            text(ellipsize(description, 190))
                .size(theme::TEXT_READ)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.68))
                .wrapping(Wrapping::WordOrGlyph),
        );
    }

    // Linha de meta: plataforma, tempo e instalação, no mesmo tom discreto do
    // Orbit ("Steam" acima do título, "8m · Last played today" abaixo).
    let mut meta = row![
        text(game.launcher.clone())
            .size(theme::TEXT_CAPTION)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.60)),
        text("·").size(theme::TEXT_CAPTION).color(FAINT),
        text(game.playtime_label_i18n(i18n))
            .size(theme::TEXT_CAPTION)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.60)),
        text("·").size(theme::TEXT_CAPTION).color(FAINT),
        text(if game.installed {
            i18n.t("native.game.installed")
        } else {
            i18n.t("native.game.not_installed")
        })
        .size(theme::TEXT_CAPTION)
        .color(if game.installed {
            theme::SUCCESS
        } else {
            theme::WARNING
        }),
    ]
    .spacing(8)
    .align_y(iced::Center);

    if let Some(year) = game.year.as_deref().filter(|year| !year.is_empty()) {
        meta = meta.push(text("·").size(theme::TEXT_CAPTION).color(FAINT));
        meta = meta.push(
            text(year)
                .size(theme::TEXT_CAPTION)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.60)),
        );
    }

    // Botões do hero: primário claro ("Jogar agora") e secundário escuro
    // ("Detalhes"), mais o coração de favorito — o trio do Orbit.
    let play_label = if game.installed {
        i18n.t("native.console.hero.play")
    } else {
        i18n.t("native.console.install_desktop")
    };
    // O `button` do iced não centraliza o conteúdo no eixo vertical: com
    // altura fixa, o texto gruda no topo. Um `container` com `align_y`
    // resolve, e é por isso que cada botão embrulha o texto.
    let play = button(
        container(
            text(play_label)
                .size(theme::TEXT_BODY)
                .font(theme::FONT_SEMIBOLD)
                .color(Color::from_rgb8(0x0c, 0x0e, 0x12)),
        )
        .height(Fill)
        .align_y(iced::Center),
    )
    .on_press(Message::ConsolePlay)
    .height(Length::Fixed(44.0))
    .padding([0.0, 24.0])
    .style(theme::console_hero_primary);

    let details = button(
        container(
            text(i18n.t("native.console.hero.details"))
                .size(theme::TEXT_BODY)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
        )
        .height(Fill)
        .align_y(iced::Center),
    )
    .on_press(Message::ConsoleMore)
    .height(Length::Fixed(44.0))
    .padding([0.0, 20.0])
    .style(theme::console_hero_secondary);

    // Trio dentro de uma pílula de vidro, como o `LiquidGlass` que envolve
    // os botões do hero no Orbit. O container dá a moldura; os botões ficam
    // soltos dentro dele com 6 px de respiro.
    let favorite = button(
        container(
            icons::icon(
                Icon::Heart,
                18.0,
                if game.favorite { 0.92 } else { 0.72 },
            ),
        )
        .width(Fill)
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center),
    )
    .on_press(Message::ConsoleFavorite)
    .width(Length::Fixed(44.0))
    .height(Length::Fixed(44.0))
    .padding(0)
    .style(theme::console_hero_icon);

    let actions = container(
        row![play, details, favorite]
            .spacing(6)
            .align_y(iced::Center),
    )
    .padding(6)
    .style(theme::console_glass_pill);

    // Aviso de rodapé (ou dica de foco) fica por último, discreto: no Orbit
    // esse espaço é dos prompts de controle, que agora vivem na barra fixa do
    // pé — aqui só sobra o recado pontual.
    let mut block = column![copy, meta, actions].spacing(16);

    if let Some(notice) = app.big_picture.notice() {
        block = block.push(
            text(notice)
                .size(theme::TEXT_CAPTION)
                .color(theme::WARNING)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }

    block.max_width(560).into()
}

/// Texto de foco/controles equivalente ao `aria-label` da web para cada zona.
fn focus_hint(app: &App) -> &str {
    let bp = &app.big_picture;
    let key = if bp.menu_open() {
        "native.console.hint.menu"
    } else if bp.store_detail_open() {
        "native.console.hint.store_detail"
    } else if bp.overview_open() {
        "native.console.hint.overview"
    } else {
        match bp.zone() {
            ConsoleZone::Tabs => "native.console.hint.tabs",
            ConsoleZone::Rail => "native.console.hint.rail",
            ConsoleZone::StoreSearch if bp.store_editing() => "native.console.hint.store_editing",
            ConsoleZone::StoreSearch => "native.console.hint.store_search",
            ConsoleZone::StoreHero | ConsoleZone::StoreCategories => "native.console.hint.store_grid",
            ConsoleZone::StoreGrid => "native.console.hint.store_grid",
            ConsoleZone::StoreDetail => "native.console.hint.store_detail",
            ConsoleZone::Overview => "native.console.hint.overview",
        }
    };
    app.i18n.t(key)
}

fn tab_placeholder(app: &App, tab: ConsoleTab) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let (title, detail) = match tab {
        ConsoleTab::Home => (
            i18n.t("native.console.home.empty_title"),
            i18n.t("native.console.home.empty_desc"),
        ),
        ConsoleTab::Stores => (
            i18n.t("store.titulo"),
            i18n.t("native.console.store.subtitle"),
        ),
        ConsoleTab::News => (
            i18n.t("topbar.noticias"),
            i18n.t("native.console.news_feed_note"),
        ),
        ConsoleTab::Library => (
            i18n.t("sidebar.biblioteca"),
            i18n.t("native.console.library_hint"),
        ),
    };

    container(
        column![
            text(i18n.t(tab.i18n_key()).to_uppercase())
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::ACCENT),
            text(title)
                .size(28)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(detail)
                .size(theme::TEXT_READ)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph),
            row![
                button(
                    text(i18n.t("sidebar.biblioteca"))
                        .size(theme::TEXT_READ)
                        .font(theme::FONT_MEDIUM)
                )
                .on_press(Message::ConsoleSelectTab(ConsoleTab::Library))
                .padding([10.0, 18.0])
                .style(theme::primary_button),
                exit_button(app),
            ]
            .spacing(12),
        ]
        .spacing(14)
        .max_width(680),
    )
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .center_y(Fill)
    .into()
}

fn news_view(app: &App) -> Element<'_, Message> {
    let state = &app.console_news;
    let header = row![
        column![
            text(app.i18n.t("native.console.news_title"))
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::ACCENT),
            text(app.i18n.t("native.console.news_subtitle"))
                .size(theme::TEXT_CAPTION)
                .color(FAINT)
                .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(3),
    ]
    .align_y(iced::Center);
    // O bloco acima já fecha o cabeçalho; as linhas seguintes mantêm o corpo.

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Idle | LoadStatus::Loading if state.items.is_empty() => {
            super::loading_state(app.i18n.t("native.console.news_loading"))
        }
        LoadStatus::Error(error) if state.items.is_empty() => container(
            column![
                text(app.i18n.t("native.console.news_error"))
                    .size(20)
                    .font(theme::FONT_BOLD)
                    .color(theme::DANGER),
                text(error.as_str())
                    .size(theme::TEXT_READ)
                    .color(theme::TEXT_2)
                    .wrapping(Wrapping::WordOrGlyph),
                button(
                    text(app.i18n.t("downloads.tentar_novamente"))
                        .size(theme::TEXT_BODY)
                        .font(theme::FONT_MEDIUM),
                )
                .on_press(Message::ConsoleNewsReload)
                .padding([8.0, 14.0])
                .style(theme::primary_button),
            ]
            .spacing(10)
            .max_width(680),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .into(),
        LoadStatus::Ready if state.items.is_empty() => super::empty_state(
            app.i18n.t("native.console.news_empty_title"),
            app.i18n.t("native.console.news_empty_desc"),
        ),
        _ => {
            let mut list = column![].spacing(8);
            for (index, item) in state.items.iter().enumerate() {
                let selected = app.big_picture.news_index() == index;
                let mut content = column![
                    text(item.title.clone())
                        .size(16)
                        .font(theme::FONT_MEDIUM)
                        .color(theme::TEXT_1)
                        .wrapping(Wrapping::WordOrGlyph),
                    row![
                        text(item.source.clone())
                            .size(theme::TEXT_MICRO)
                            .font(theme::FONT_SEMIBOLD)
                            .color(theme::ACCENT),
                        text("·").size(theme::TEXT_MICRO).color(FAINT),
                        text(item.date_label_i18n(&app.i18n))
                            .size(theme::TEXT_MICRO)
                            .color(MUTED),
                    ]
                    .spacing(8)
                    .align_y(iced::Center),
                    text(item.url.clone())
                        .size(theme::TEXT_MICRO)
                        .color(FAINT)
                        .wrapping(Wrapping::WordOrGlyph),
                ]
                .spacing(4);

                if selected && !item.summary.is_empty() {
                    content = content.push(
                        text(shorten(&item.summary, 240))
                            .size(theme::TEXT_BODY)
                            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.74))
                            .wrapping(Wrapping::WordOrGlyph),
                    );
                }

                list = list.push(
                    button(content)
                        .on_press(Message::ConsoleNewsSelected(index))
                        .width(Fill)
                        .padding([10.0, 14.0])
                        .style(move |style_theme, status| {
                            theme::console_action(style_theme, status, selected, 0.0)
                        }),
                );
            }

            scrollable(list)
                .height(Fill)
                .style(theme::console_scrollable)
                .into()
        }
    };

    container(column![header, body].spacing(14))
        .width(Fill)
        .height(Fill)
        .padding([20.0, 0.0])
        .into()
}

fn empty_panel<'a>(
    title: &'a str,
    detail: &'a str,
    actions: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut content = column![
        text(title)
            .size(26)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text(detail)
            .size(theme::TEXT_READ)
            .color(theme::TEXT_2)
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(12);

    if !actions.is_empty() {
        content = content.push(row(actions).spacing(12));
    }

    container(content.max_width(620))
        .padding(22)
        .style(theme::console_panel)
        .into()
}

fn exit_button(app: &App) -> Element<'_, Message> {
    button(
        text(app.i18n.t("sidebar.modo_desktop"))
            .size(theme::TEXT_BODY)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::ExitBigPicture)
    .padding([10.0, 16.0])
    .style(theme::ghost_button)
    .into()
}

fn rail<'a>(app: &'a App, games: &[&'a Game], has_continue: bool) -> Element<'a, Message> {
    // Cabeçalho da fileira como no Orbit: título grande, contagem embaixo,
    // filtros à direita. A dica de controle saiu daqui porque agora vive na
    // barra fixa do pé — repetir nos dois lugares polui a tela.
    let shelf_title = app.i18n.t("native.console.shelf.title");
    let count_label = app
        .i18n
        .t_args("native.console.shelf.count", &[("count", &games.len().to_string())]);

    let _ = has_continue;

    let label = column![
        text(shelf_title)
            .size(theme::TEXT_TITLE)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text(count_label)
            .size(theme::TEXT_CAPTION)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.52)),
    ]
    .spacing(2);

    // Os chips ficam numa pílula de vidro, como o `All games / Favorites` do
    // Orbit: o agrupamento é o que diz que os dois são o mesmo controle, e a
    // moldura compartilhada dá o mesmo peso visual da pílula de navegação.
    // "Ver biblioteca" fica fora dela porque navega para outra seção.
    let filters = row![
        container(
            row![
                filter_chip(app, "console.filter.all"),
                filter_chip(app, "console.filter.favorites"),
            ]
            .spacing(2)
            .align_y(iced::Center),
        )
        .padding(4)
        .style(theme::console_glass_pill),
        Space::new().width(8),
        view_all_chip(app),
    ]
    .spacing(0)
    .align_y(iced::Center);

    let header = row![label, Space::new().width(Fill), filters].align_y(iced::Center);

    if games.is_empty() {
        return container(
            column![
                header,
                container(
                    text(app.i18n.t("native.home.grid_empty"))
                        .size(theme::TEXT_BODY)
                        .color(FAINT),
                )
                .width(Fill)
                .padding([18.0, 4.0]),
            ]
            .spacing(6),
        )
        .width(Fill)
        .height(Length::Fixed(120.0))
        .padding(Padding {
            top: 0.0,
            right: GUTTER,
            bottom: 16.0,
            left: GUTTER,
        })
        .into();
    }

    let cards: Vec<Element<'a, Message>> = games
        .iter()
        .enumerate()
        .map(|(index, game)| card(app, index, game))
        .collect();

    let content = iced::widget::Row::with_children(cards)
        .spacing(RAIL_CARD_GAP)
        .padding(Padding {
            top: 6.0,
            right: GUTTER,
            bottom: 6.0,
            left: GUTTER,
        });

    // Sem barra de rolagem visível: o trilho anda por gamepad, teclado e
    // clique, e a barra fina só sujava o pé da fileira.
    let scroller = scrollable(content)
        .direction(Direction::Horizontal(
            Scrollbar::new().width(0).margin(0).scroller_width(0),
        ))
        .id(RAIL_ID)
        .height(Length::Fixed(RAIL_CARD_HEIGHT + 14.0))
        .style(theme::console_scrollable);

    container(column![header, scroller].spacing(10))
        .width(Fill)
        .height(Length::Fixed(RAIL_HEIGHT))
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 12.0,
            left: 0.0,
        })
        .into()
}

fn card<'a>(app: &'a App, index: usize, game: &'a Game) -> Element<'a, Message> {
    let selected = app.big_picture.active_index() == index;
    let focused = selected && app.big_picture.zone() == ConsoleZone::Rail;

    // Keep one artwork source per card. Switching from the library cover to a
    // remotely resolved square image after entry made the rail visibly change.
    let cover: Element<'a, Message> = match app.covers.ready(game.cover_url.as_deref()) {
        Some(handle) => {
            container(
                image(handle.clone())
                    .width(Fill)
                    .height(Fill)
                    .content_fit(ContentFit::Cover)
                    .border_radius(12),
            )
            .width(Fill)
            .height(Fill)
            .style(theme::cover_portrait_backdrop)
            .into()
        }
        None => container(
            text(initial(&game.title))
                .size(38)
                .font(theme::FONT_BOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.18)),
        )
        .width(Fill)
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::cover_placeholder)
        .into(),
    };

    let title_color = if selected {
        theme::TEXT_1
    } else {
        Color::from_rgba(1.0, 1.0, 1.0, 0.62)
    };

    // Só o card focado mostra o título; é o comportamento do Orbit — a fileira
    // fica limpa e a informação aparece no item em que o controle está.
    // Um único `button` envolve capa e título: botão dentro de botão quebra o
    // hit-testing do iced, o clique externo nunca dispara.
    // Zoom do card focado: o iced não escala um widget, então o focado é
    // desenhado maior e recuado dentro da mesma célula — o efeito é o mesmo
    // salto de escala do Orbit, e o trilho não reflui porque a célula mantém
    // o tamanho base.
    let scale = if focused { ZOOM_FOCUSED } else { 1.0 };
    let card_width = RAIL_CARD_WIDTH * scale;
    let card_height = RAIL_CARD_HEIGHT * scale;

    button(
        column![
            container(cover).width(Fill).height(Fill).clip(true),
            container(
                text(ellipsize(&game.title, 22))
                    .size(theme::TEXT_BODY)
                    .font(if selected {
                        theme::FONT_SEMIBOLD
                    } else {
                        theme::FONT_MEDIUM
                    })
                    .color(title_color)
                    .wrapping(Wrapping::None),
            )
            .width(Fill)
            .height(18)
            .clip(true),
        ]
        .spacing(8),
    )
    .on_press(Message::ConsoleSelectGame(index))
        .width(Length::Fixed(card_width))
        .height(Length::Fixed(card_height))
        .padding(0)
        .style(move |style_theme, status| {
            theme::console_card(
                style_theme,
                status,
                selected,
                focused,
                app.big_picture.focus_glow(),
            )
        })
        .into()
}

/// Chip de filtro da fileira (`Todos`, `Favoritos`), como o `Recent /
/// Installed / Favorites` do Orbit.
///
/// O filtro ativo ainda é o "Todos": a fileira do console mostra a biblioteca
/// inteira, e alternar recorte aqui exigiria uma variante de estado que o
/// `BigPicture` não tem. O chip existe, responde ao clique levando o foco para
/// a fileira e não mente sobre o que está mostrando.
fn filter_chip<'a>(app: &'a App, key: &'static str) -> Element<'a, Message> {
    let active = key == "console.filter.all";
    // Container com `align_y` porque o `button` do iced gruda o conteúdo no
    // topo quando a altura é fixa.
    button(
        container(
            text(app.i18n.t(key))
                .size(theme::TEXT_CAPTION)
                .font(if active {
                    theme::FONT_SEMIBOLD
                } else {
                    theme::FONT
                })
                .color(if active { theme::TEXT_1 } else { MUTED }),
        )
        .height(Fill)
        .align_y(iced::Center),
    )
    .on_press(Message::ConsoleFocusRail)
    .height(Length::Fixed(32.0))
    .padding([0.0, 14.0])
    .style(move |style_theme, status| {
        theme::console_filter_chip(style_theme, status, active)
    })
    .into()
}

/// "Ver biblioteca": leva a top bar para a aba Biblioteca.
fn view_all_chip(app: &App) -> Element<'_, Message> {
    button(
        row![
            text(app.i18n.t("native.console.shelf.view_all"))
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_MEDIUM)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.82)),
            icons::icon(Icon::ChevronDown, 12.0, 0.72),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::ConsoleSelectTab(ConsoleTab::Library))
    .padding([7.0, 12.0])
    .style(|style_theme, status| theme::console_filter_chip(style_theme, status, false))
    .into()
}

fn overview<'a>(app: &'a App, game: &'a Game) -> Element<'a, Message> {
    let bp = &app.big_picture;
    let focused = |index: usize| bp.zone() == ConsoleZone::Overview && bp.action_index() == index;

    let back = button(
        row![
            text("‹").size(20).color(theme::TEXT_1),
            text(app.i18n.t("native.console.back"))
                .size(theme::TEXT_READ)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::ConsoleCloseOverview)
    .padding([8.0, 14.0])
    .style(theme::ghost_button);

    let header = row![
        back,
        Space::new().width(Fill),
        text(game.title.clone()).size(theme::TEXT_READ).color(MUTED),
        Space::new().width(Fill),
        text(bp.clock())
            .size(theme::TEXT_TITLE)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_1),
    ]
    .align_y(iced::Center);

    let running = app.launch.is_running(&game.id);
    let (play_glyph, play_label, play_message) = if app.launch.is_launching(&game.id) {
        ("▶", app.i18n.t("common.abrindo").to_string(), None)
    } else if running {
        (
            "■",
            app.i18n.t("hero.parar").to_string(),
            Some(Message::StopGame),
        )
    } else if app.launch.is_stopping(&game.id) {
        ("■", app.i18n.t("native.game.stopping").to_string(), None)
    } else {
        (
            "▶",
            app.i18n.t("hero.jogar").to_string(),
            Some(Message::ConsolePlay),
        )
    };

    let play = button(
        row![
            text(play_glyph).size(theme::TEXT_BODY),
            text(play_label).size(17).font(theme::FONT_SEMIBOLD),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .on_press_maybe(play_message)
    .padding([13.0, 26.0])
    .style(move |style_theme, status| {
        theme::console_play(style_theme, status, focused(ACTION_PLAY), bp.focus_glow())
    });

    let trailer = button(
        text(app.i18n.t("gameoverview.trailer"))
            .size(theme::TEXT_READ)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::ConsoleTrailer)
    .padding([12.0, 20.0])
    .style(move |style_theme, status| {
        theme::console_action(
            style_theme,
            status,
            focused(ACTION_TRAILER),
            bp.focus_glow(),
        )
    });

    let favorite_label = if game.favorite {
        app.i18n.t("native.console.favorite.remove")
    } else {
        app.i18n.t("native.console.favorite.add")
    };
    let favorite = button(
        row![
            text(if game.favorite { "♥" } else { "♡" })
                .size(theme::TEXT_HEADING)
                .color(if game.favorite { theme::TEXT_1 } else { MUTED }),
            text(favorite_label)
                .size(theme::TEXT_BODY)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .on_press(Message::ConsoleFavorite)
    .padding([12.0, 16.0])
    .style(move |style_theme, status| {
        theme::console_action(
            style_theme,
            status,
            focused(ACTION_FAVORITE),
            bp.focus_glow(),
        )
    });

    let more = button(
        row![
            text("•••").size(theme::TEXT_READ).color(theme::TEXT_1),
            text(app.i18n.t("native.console.more"))
                .size(theme::TEXT_BODY)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .on_press(Message::ConsoleMore)
    .padding([12.0, 16.0])
    .style(move |style_theme, status| {
        theme::console_action(style_theme, status, focused(ACTION_MORE), bp.focus_glow())
    });

    // Entrada mouse para o diálogo de opções; a navegação por gamepad fica
    // nas quatro ações do `FocusGrid` (escopo desta onda).
    let launch_options = button(
        text(app.i18n.t("native.console.options"))
            .size(theme::TEXT_READ)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_1),
    )
    .on_press(Message::OpenLaunchOptions(game.id.clone()))
    .padding([10.0, 16.0])
    .style(|style_theme, status| theme::console_action(style_theme, status, false, 0.0));

    let tag = game
        .genre
        .as_deref()
        .filter(|genre| !genre.is_empty())
        .unwrap_or_else(|| app.i18n.t("native.console.game_fallback"))
        .to_uppercase();

    let description = game
        .description
        .as_deref()
        .map(|description| shorten(description, 300))
        .unwrap_or_else(|| app.i18n.t("native.console.no_description").to_string());

    let mut activity = column![
        container(
            text(tag)
                .size(theme::TEXT_MICRO)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::TEXT_1)
        )
        .padding([4.0, 9.0])
        .style(theme::badge),
        text(game.title.clone())
            .size(44)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1)
            .wrapping(Wrapping::WordOrGlyph),
        text(game.metadata_line_i18n(&app.i18n))
            .size(theme::TEXT_READ)
            .color(MUTED),
        text(description)
            .size(theme::TEXT_READ)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.76))
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(12)
    .width(Fill)
    .max_width(700);

    if let Some(notice) = bp.notice() {
        activity = activity.push(super::warning_banner(notice));
    }

    if let Some(error) = app.launch.error.as_deref() {
        activity = activity.push(super::action_error_banner(error, None));
    }
    let warnings = app.launch.warnings_for(&game.id);
    if !warnings.is_empty() {
        activity = activity.push(super::game::launch_warning_banner(warnings.join("  ·  ")));
    }

    activity = activity.push(
        row![
            text(game.playtime_label_i18n(&app.i18n))
                .size(theme::TEXT_BODY)
                .color(MUTED),
            text("·").size(theme::TEXT_BODY).color(FAINT),
            text(if game.installed {
                app.i18n.t("native.game.installed")
            } else {
                app.i18n.t("native.console.install_desktop")
            })
            .size(theme::TEXT_BODY)
            .color(if game.installed {
                theme::SUCCESS
            } else {
                theme::WARNING
            }),
            text("·").size(theme::TEXT_BODY).color(FAINT),
            text(game.launcher.clone())
                .size(theme::TEXT_BODY)
                .color(MUTED),
        ]
        .spacing(10)
        .align_y(iced::Center),
    );

    activity = activity.push(
        row![play, trailer, favorite, more, launch_options]
            .spacing(16)
            .align_y(iced::Center),
    );

    if running {
        activity = activity.push(
            text(app.i18n.t("native.game.running_note"))
                .size(theme::TEXT_CAPTION)
                .color(theme::SUCCESS),
        );
    }

    activity = activity.push(text(focus_hint(app)).size(theme::TEXT_MICRO).color(FAINT));

    let trailer = console_trailer(app, game);

    container(
        column![
            header,
            Space::new().height(Fill),
            row![activity, Space::new().width(Fill), trailer,]
                .spacing(48)
                .align_y(iced::Bottom),
        ]
        .width(Fill)
        .height(Fill),
    )
    .width(Fill)
    .height(Fill)
    .padding([28.0, GUTTER])
    .style(theme::console_overview_alpha(bp.overview_alpha()))
    .into()
}

/// Frame do GStreamer (ou poster do jogo) + play/pause no painel do console.
fn console_trailer<'a>(app: &'a App, game: &'a Game) -> Element<'a, Message> {
    let media = &app.media;

    let surface: Element<'a, Message> = match media.texture() {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(203)
            .content_fit(ContentFit::Contain)
            .into(),
        None => match app.covers.ready(game.cover_url.as_deref()) {
            Some(handle) => image(handle.clone())
                .width(Fill)
                .height(203)
                .content_fit(ContentFit::Cover)
                .border_radius(6)
                .into(),
            None => container(
                text(initial(&game.title))
                    .size(42)
                    .font(theme::FONT_BOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.18)),
            )
            .width(Fill)
            .height(203)
            .align_x(iced::Center)
            .align_y(iced::Center)
            .style(theme::cover_placeholder)
            .into(),
        },
    };

    let notice = app
        .game_media
        .as_ref()
        .and_then(|game_media| game_media.notice.as_deref());
    let (status, status_color) = if let Some(notice) = notice {
        (notice.to_string(), theme::WARNING)
    } else if media.status().is_error() {
        (media_status_label(app, media.status()), theme::DANGER)
    } else if app.game_media.is_none() {
        (
            app.i18n.t("native.console.trailer_querying").to_string(),
            MUTED,
        )
    } else {
        (media_status_label(app, media.status()), MUTED)
    };

    let mut toggle = button(
        text(if media.is_playing() {
            app.i18n.t("native.game.pause")
        } else {
            app.i18n.t("native.game.play")
        })
        .size(theme::TEXT_CAPTION)
        .font(theme::FONT_MEDIUM),
    )
    .padding([6.0, 12.0])
    .style(theme::ghost_button);
    if media.can_control() {
        toggle = toggle.on_press(Message::MediaTogglePlayback);
    }

    let mut controls = row![toggle].spacing(10).align_y(iced::Center);
    let duration = media.duration_seconds();
    if duration > 0.5 {
        controls = controls.push(
            text(format!(
                "{} / {}",
                media::format_seconds(media.position_seconds()),
                media::format_seconds(duration)
            ))
            .size(theme::TEXT_MICRO)
            .color(MUTED),
        );
    } else {
        controls = controls.push(
            text(app.i18n.t("native.console.no_duration"))
                .size(theme::TEXT_MICRO)
                .color(FAINT),
        );
    }

    container(
        column![
            container(surface)
                .width(Fill)
                .height(203)
                .clip(true)
                .style(theme::cover_placeholder),
            controls,
            text(status)
                .size(theme::TEXT_MICRO)
                .color(status_color)
                .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(8),
    )
    .width(340)
    .padding(10)
    .style(theme::console_panel)
    .into()
}

/// Menu "•••" do painel: Abrir ficha, Favoritar/Desfavoritar e Trailer.
fn more_menu<'a>(app: &'a App, game: &'a Game) -> Element<'a, Message> {
    let bp = &app.big_picture;
    let focused = |index: usize| bp.menu_index() == index;

    let favorite_label = if game.favorite {
        app.i18n.t("native.console.favorite.remove")
    } else {
        app.i18n.t("native.console.favorite.add")
    };
    let trailer_label = if app.media.is_playing() {
        app.i18n.t("native.console.pause_trailer")
    } else {
        app.i18n.t("native.console.play_trailer")
    };
    let labels = [
        app.i18n.t("native.console.open_detail"),
        favorite_label,
        trailer_label,
    ];
    let actions = [MENU_OPEN_DETAIL, MENU_FAVORITE, MENU_TRAILER];

    let mut items = column![].spacing(4);
    for index in 0..MENU_COUNT {
        let selected = focused(index);
        items = items.push(
            button(text(labels[index]).size(17).font(theme::FONT_MEDIUM))
                .on_press(Message::ConsoleMenuAction(actions[index]))
                .width(Fill)
                .padding([12.0, 16.0])
                .style(move |style_theme, status| {
                    theme::console_action(style_theme, status, selected, bp.focus_glow())
                }),
        );
    }

    let panel = container(
        column![
            container(
                text(game.title.clone())
                    .size(16)
                    .font(theme::FONT_SEMIBOLD)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::None),
            )
            .width(Fill)
            .padding([12.0, 16.0])
            .style(theme::console_panel_header),
            container(items).padding([8.0, 10.0]),
            container(
                text(app.i18n.t("native.console.hint.menu"))
                    .size(theme::TEXT_MICRO)
                    .color(FAINT),
            )
            .width(Fill)
            .padding([8.0, 16.0]),
        ]
        .spacing(0),
    )
    .width(420)
    .style(theme::console_panel_alpha(bp.menu_alpha()));

    let veil = mouse_area(
        container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(theme::console_veil(bp.menu_alpha())),
    )
    .on_press(Message::ConsoleCloseMenu);

    let panel_layer = container(panel)
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill);

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(veil)
        .push(panel_layer)
        .into()
}

// ── Loja do console ────────────────────────────────────────────────────────

/// Itens da Loja para a aba atual: busca quando existe, senão `store:recent`.
fn store_games(app: &App) -> Vec<&StoreGame> {
    if let Some(search) = app.store.search.as_ref() {
        search
            .result
            .as_ref()
            .map(|result| result.games.iter().collect())
            .unwrap_or_default()
    } else {
        app.store.browse.games.iter().collect()
    }
}

fn console_store(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let state = &app.store;
    let games = store_games(app);

    let (status, error, has_more) = if let Some(search) = state.search.as_ref() {
        (search.status.clone(), search.error.as_deref(), false)
    } else {
        (
            state.browse.status.clone(),
            state.browse.error.as_deref(),
            state.browse.has_more,
        )
    };

    let count = games.len();
    let count_label = if state.search.is_some() {
        i18n.t_args("store.resultados_count", &[("count", &count.to_string())])
    } else {
        i18n.t_args(
            "native.console.store.catalog_count",
            &[("count", &count.to_string())],
        )
    };

    let mut header = column![
        text(i18n.t("store.titulo").to_uppercase())
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_SEMIBOLD)
            .color(theme::ACCENT),
        row![
            text(i18n.t("native.console.store.subtitle"))
                .size(theme::TEXT_BODY)
                .color(FAINT),
            Space::new().width(Fill),
            text(count_label).size(theme::TEXT_CAPTION).color(MUTED),
        ]
        .align_y(iced::Center),
    ]
    .spacing(4);

    if let Some(search) = state.search.as_ref() {
        header = header.push(
            text(format!("“{}”", search.query))
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_3),
        );
    }

    let search_row = store_search_row(app);

    let body: Element<'_, Message> = match status {
        LoadStatus::Idle | LoadStatus::Loading if games.is_empty() => {
            store_loading(if state.search.is_some() {
                i18n.t("native.console.store.searching")
            } else {
                i18n.t("native.console.store.loading")
            })
        }
        LoadStatus::Error(_) => store_error(
            app,
            error.unwrap_or(i18n.t("store.busca_falhou")),
            if state.search.is_some() {
                Message::StoreSearchSubmitted
            } else {
                Message::Refresh(crate::app::Section::Stores)
            },
        ),
        _ if games.is_empty() => store_empty(app),
        _ => store_grid(app, &games, has_more),
    };

    let footer = text(focus_hint(app)).size(theme::TEXT_MICRO).color(FAINT);

    container(
        scrollable(
            column![header, search_row, body, footer]
                .spacing(16)
                .width(Fill),
        )
        .height(Fill)
        .style(theme::console_scrollable),
    )
    .width(Fill)
    .height(Fill)
    .padding(Padding {
        top: 20.0,
        right: GUTTER,
        bottom: 16.0,
        left: GUTTER,
    })
    .style(theme::app_background)
    .into()
}

pub(super) fn store_search_row(app: &App) -> Element<'_, Message> {
    let bp = &app.big_picture;
    let focused = bp.zone() == ConsoleZone::StoreSearch;
    let glow = bp.focus_glow();
    let editing = bp.store_editing();

    let input = text_input(app.i18n.t("store.buscar_placeholder"), &app.store.query)
        .id(Id::new(STORE_SEARCH_ID))
        .on_input(Message::StoreQueryChanged)
        .on_submit(Message::StoreSearchSubmitted)
        .padding(Padding {
            top: 12.0,
            right: 14.0,
            bottom: 12.0,
            left: 16.0,
        })
        .size(16)
        .width(Fill)
        .style(move |style_theme, status| {
            theme::console_search_input(style_theme, status, focused, editing, glow)
        });

    // O clique também entra no modo de edição e foca o campo; o `mouse_area`
    // não engole o clique porque o foco é aplicado pela Task da mensagem.
    let input = mouse_area(input).on_press(Message::ConsoleStoreFocus);

    let busy = app
        .store
        .search
        .as_ref()
        .is_some_and(|search| search.status == LoadStatus::Loading);
    let mut search_button = button(
        text(if busy {
            app.i18n.t("store.buscando")
        } else {
            app.i18n.t("store.buscar")
        })
        .size(theme::TEXT_BODY)
        .font(theme::FONT_MEDIUM),
    )
    .padding([10.0, 18.0]);
    if !busy && !app.store.query.trim().is_empty() {
        search_button = search_button.on_press(Message::StoreSearchSubmitted);
    }

    let mut clear_button = button(
        text(app.i18n.t("native.console.store.clear"))
            .size(theme::TEXT_BODY)
            .font(theme::FONT_MEDIUM),
    )
    .padding([10.0, 14.0])
    .style(theme::ghost_button);
    if !app.store.query.is_empty() || app.store.search.is_some() {
        clear_button = clear_button.on_press(Message::StoreClearSearch);
    }

    let hint = if editing {
        app.i18n.t("native.console.hint.store_editing")
    } else {
        app.i18n.t("native.console.hint.store_search")
    };

    column![
        row![
            input,
            search_button.style(theme::primary_button),
            clear_button
        ]
        .spacing(10)
        .align_y(iced::Center),
        text(hint)
            .size(theme::TEXT_MICRO)
            .color(if focused { theme::ACCENT } else { FAINT }),
    ]
    .spacing(6)
    .into()
}

fn store_loading(message: &str) -> Element<'_, Message> {
    container(
        column![
            text(message).size(theme::TEXT_READ).color(MUTED),
            text("store:search / store:recent")
                .size(theme::TEXT_MICRO)
                .color(FAINT),
        ]
        .spacing(6),
    )
    .width(Fill)
    .padding(32)
    .center_x(Fill)
    .into()
}

fn store_empty(app: &App) -> Element<'_, Message> {
    container(
        column![
            text(app.i18n.t("native.console.store.empty_title"))
                .size(theme::TEXT_HEADING)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(app.i18n.t("native.console.store.empty_desc"))
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(8)
        .max_width(560),
    )
    .width(Fill)
    .padding(24)
    .style(theme::console_panel)
    .into()
}

fn store_error<'a>(app: &'a App, error: &'a str, retry: Message) -> Element<'a, Message> {
    container(
        column![
            text(app.i18n.t("native.console.store.error_title"))
                .size(20)
                .font(theme::FONT_BOLD)
                .color(theme::DANGER),
            text(error)
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph),
            button(
                text(app.i18n.t("downloads.tentar_novamente"))
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(retry)
            .padding([8.0, 14.0])
            .style(theme::primary_button),
        ]
        .spacing(10)
        .max_width(720),
    )
    .width(Fill)
    .padding(20)
    .style(theme::console_panel)
    .into()
}

fn store_grid<'a>(app: &'a App, games: &[&'a StoreGame], has_more: bool) -> Element<'a, Message> {
    let bp = &app.big_picture;
    let focused = bp.zone() == ConsoleZone::StoreGrid;
    let selected_index = bp.store_index();
    let glow = bp.focus_glow();

    let mut cards: Vec<Element<'a, Message>> = games
        .iter()
        .enumerate()
        .map(|(index, game)| store_card(app, game, index, focused && selected_index == index, glow))
        .collect();

    if has_more {
        let index = games.len();
        cards.push(store_more_card(
            app,
            focused && selected_index == index,
            glow,
        ));
    }

    container(
        iced::widget::Grid::with_children(cards)
            .columns(STORE_GRID_COLS)
            .spacing(16),
    )
    .width(Fill)
    .into()
}

fn store_card<'a>(
    app: &'a App,
    game: &'a StoreGame,
    index: usize,
    focused: bool,
    glow: f32,
) -> Element<'a, Message> {
    let cover: Element<'a, Message> = match app.covers.ready(game.cover_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(104)
            .content_fit(ContentFit::Cover)
            .border_radius(6)
            .into(),
        None => container(
            text(initial(&game.title))
                .size(26)
                .font(theme::FONT_BOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.18)),
        )
        .width(Fill)
        .height(104)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::cover_placeholder)
        .into(),
    };

    let mut meta = row![].spacing(6).align_y(iced::Center);
    if let Some(price) = game
        .price
        .as_deref()
        .filter(|price| !price.trim().is_empty())
    {
        meta = meta.push(
            text(price)
                .size(theme::TEXT_MICRO)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::SUCCESS),
        );
    }
    if let Some(source) = game
        .source
        .as_deref()
        .filter(|source| !source.trim().is_empty())
    {
        meta = meta.push(
            text(ellipsize(source, 18))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }

    iced::widget::mouse_area(button(
        column![
            container(cover).width(Fill).height(104).clip(true),
            container(
                column![
                    container(
                        text(ellipsize(&game.title, 26))
                            .size(theme::TEXT_BODY)
                            .font(theme::FONT_MEDIUM)
                            .color(theme::TEXT_1)
                            .wrapping(Wrapping::None),
                    )
                    .width(Fill)
                    .height(18)
                    .clip(true),
                    meta,
                ]
                .spacing(3),
            )
            .width(Fill)
            .height(48)
            .padding([8.0, 10.0]),
        ]
        .width(Fill),
    )
    .on_press(Message::ConsoleStoreOpenDetail(game.clone()))
    .width(Fill)
    .padding(0)
    .style(move |style_theme, status| {
        theme::console_store_card(
            style_theme,
            status,
            index == app.big_picture.store_index(),
            focused,
            glow,
        )
    })).on_enter(Message::PrefetchStore(game.clone()))
    .into()
}

fn store_more_card<'a>(app: &'a App, focused: bool, glow: f32) -> Element<'a, Message> {
    let loading = app.store.browse.loading_more;
    button(
        column![
            text(if loading { "…" } else { "＋" })
                .size(26)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_2),
            text(app.i18n.t("native.console.store.load_more"))
                .size(theme::TEXT_BODY)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
        ]
        .spacing(8)
        .align_x(iced::Center),
    )
    .on_press_maybe((!loading).then_some(Message::StoreLoadMore))
    .width(Fill)
    .height(Length::Fixed(152.0))
    .padding([16.0, 12.0])
    .style(move |style_theme, status| {
        theme::console_store_card(style_theme, status, false, focused, glow)
    })
    .into()
}

fn store_detail_overlay<'a>(app: &'a App, detail: &'a StoreDetailState) -> Element<'a, Message> {
    let bp = &app.big_picture;
    let i18n = &app.i18n;
    let game = &detail.game;
    let info = detail.info.as_ref();
    let alpha = bp.store_alpha();

    let back = crate::views::back_button::labeled(
        i18n.t("native.console.store.back"),
        Message::ConsoleStoreCloseDetail,
        move |style_theme, status| {
            theme::console_action(
                style_theme,
                status,
                bp.zone() == ConsoleZone::StoreDetail,
                bp.focus_glow(),
            )
        },
    );


    let mut meta = row![].spacing(8).align_y(iced::Center);
    if let Some(price) = game
        .price
        .as_deref()
        .filter(|price| !price.trim().is_empty())
    {
        meta = meta.push(
            text(price)
                .size(theme::TEXT_READ)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::SUCCESS),
        );
    } else {
        meta = meta.push(
            text(i18n.t("native.console.store.price_missing"))
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_3),
        );
    }
    if let Some(source) = game
        .source
        .as_deref()
        .filter(|source| !source.trim().is_empty())
    {
        meta = meta.push(badge(source, theme::TEXT_2));
    }
    if game.manifest {
        meta = meta.push(badge(
            i18n.t("native.console.store.manifest"),
            theme::ACCENT,
        ));
    }

    let description: Element<'_, Message> = match (
        detail.status.clone(),
        info.and_then(|info| info.about.as_deref().or(info.description.as_deref())),
    ) {
        (_, Some(description)) => super::game::description_blocks(
            crate::rich_text::parse_description(description),
            &app.covers,
            &app.animated_cards,
        ),
        (status, None) => {
            let key = match status {
                LoadStatus::Loading => "native.console.store.description_loading",
                LoadStatus::Error(_) => "native.console.store.description_error",
                _ => "native.console.store.description_empty",
            };
            text(i18n.t(key))
                .size(theme::TEXT_READ)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph)
                .into()
        }
    };

    let mut info_rows = column![].spacing(8);
    let mut has_info = false;
    if let Some(info) = info {
        if !info.developers.is_empty() {
            info_rows = info_rows.push(detail_row(
                i18n.t("native.console.store.developer"),
                info.developers.join(", "),
            ));
            has_info = true;
        }
        if !info.publishers.is_empty() {
            info_rows = info_rows.push(detail_row(
                i18n.t("native.console.store.publisher"),
                info.publishers.join(", "),
            ));
            has_info = true;
        }
        if let Some(release) = info.release_date.as_deref() {
            info_rows = info_rows.push(detail_row(
                i18n.t("native.console.store.release"),
                release.to_string(),
            ));
            has_info = true;
        }
    }
    if !game.genres.is_empty() {
        info_rows = info_rows.push(detail_row(
            i18n.t("native.console.store.genres"),
            game.genres.join(", "),
        ));
        has_info = true;
    }
    if !has_info {
        info_rows = info_rows.push(
            text(if detail.status == LoadStatus::Loading {
                i18n.t("native.console.store.info_loading")
            } else {
                i18n.t("native.console.store.info_empty")
            })
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_3),
        );
    }

    let install = super::store_downloads::summary(app, detail);

    let mut panel_content = column![
        row![
            back,
            Space::new().width(Fill),
            text(bp.clock()).size(20).color(theme::TEXT_1)
        ]
        .align_y(iced::Center),
        Space::new().height(12),
        row![
            column![
                text(game.title.clone())
                    .size(34)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::WordOrGlyph),
                meta,
                text(i18n.t_args(
                    "native.console.store.detail_meta",
                    &[("appid", game.appid.as_str())],
                ))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
            ]
            .spacing(8)
            .width(Fill),
        ]
        .spacing(18)
        .align_y(iced::Center),
        Space::new().height(14),
        container(
            column![
                text(i18n.t("native.console.store.about").to_uppercase())
                    .size(theme::TEXT_MICRO)
                    .font(theme::FONT_SEMIBOLD)
                    .color(theme::TEXT_3),
                description,
            ]
            .spacing(8),
        )
        .width(Fill)
        .padding(16)
        .style(theme::console_panel),
        Space::new().height(12),
        row![
            container(
                column![
                    text(i18n.t("native.console.store.info").to_uppercase())
                        .size(theme::TEXT_MICRO)
                        .font(theme::FONT_SEMIBOLD)
                        .color(theme::TEXT_3),
                    info_rows,
                ]
                .spacing(8),
            )
            .width(Fill)
            .padding(16)
            .style(theme::console_panel),
            install,
        ]
        .spacing(12)
        .align_y(iced::Top),
        Space::new().height(8),
        text(focus_hint(app)).size(theme::TEXT_MICRO).color(FAINT),
    ]
    .spacing(0)
    .width(Fill);

    if let Some(error) = detail.error.as_deref() {
        panel_content = panel_content.push(
            container(text(error).size(theme::TEXT_CAPTION).color(theme::DANGER))
                .width(Fill)
                .padding([4.0, 0.0]),
        );
    }

    let panel = container(
        scrollable(panel_content)
            .height(Fill)
            .style(theme::console_scrollable),
    )
    .width(Fill)
    .height(Fill)
    .max_width(980)
    .padding(24)
    .style(theme::console_panel_alpha(alpha));

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::console_veil(alpha)),
        )
        .push(
            container(panel)
                .width(Fill)
                .height(Fill)
                .center_x(Fill)
                .center_y(Fill)
                .padding(24),
        )
        .into()
}

fn media_status_label(app: &App, status: &crate::media::MediaStatus) -> String {
    match status {
        crate::media::MediaStatus::Error(message) => message.clone(),
        crate::media::MediaStatus::Idle => app.i18n.t("native.game.media.idle").to_string(),
        crate::media::MediaStatus::Loading => app.i18n.t("native.game.media.loading").to_string(),
        crate::media::MediaStatus::Playing => app.i18n.t("native.game.media.playing").to_string(),
        crate::media::MediaStatus::Paused => app.i18n.t("native.game.media.paused").to_string(),
        crate::media::MediaStatus::Ended => app.i18n.t("native.game.media.ended").to_string(),
    }
}

fn detail_row<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    column![
        text(label).size(theme::TEXT_MICRO).color(theme::TEXT_3),
        text(value)
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_STRONG)
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(2)
    .into()
}

fn badge<'a>(label: &'a str, color: Color) -> Element<'a, Message> {
    container(
        text(label)
            .size(theme::TEXT_MICRO)
            .font(theme::FONT_MEDIUM)
            .color(color),
    )
    .padding([2.0, 6.0])
    .style(theme::badge)
    .into()
}

fn art_url(game: &Game) -> Option<&str> {
    game.hero_url
        .as_deref()
        .or(game.cover_url.as_deref())
        .filter(|url| !url.is_empty())
}
fn initial(title: &str) -> String {
    title
        .chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn ellipsize(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let mut short: String = text.chars().take(limit.saturating_sub(1)).collect();
    short.push('…');
    short
}

fn shorten(text: &str, limit: usize) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.chars().count() <= limit {
        return flattened;
    }

    let mut short: String = flattened.chars().take(limit).collect();
    short.push('…');
    short
}
