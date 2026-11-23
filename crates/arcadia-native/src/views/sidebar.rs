//! Sidebar fixa de 212 px, espelhando a `Sidebar.tsx` do shell desktop:
//! wordmark, perfil da conta, navegação com ícones, lista de jogos com busca,
//! e Big Picture.

#[path = "sidebar_brand.rs"]
mod sidebar_brand;
#[path = "sidebar_indicator.rs"]
mod sidebar_indicator;

use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, image, row, scrollable, text, text_input, Column, Space,
};
use iced::{Color, ContentFit, Element, Fill, Length, Padding};

use crate::app::{App, Message, Screen, Section, SettingsTab};
use crate::theme;
use crate::views::icons::{self, Icon};

pub fn view<'a>(
    app: &'a App,
    animated: &'a crate::animated_cards::AnimatedCards,
) -> Element<'a, Message> {
    let i18n = &app.i18n;

    let brand = container(
        iced::widget::canvas::Canvas::new(sidebar_brand::Brand {
            reduced_motion: app.reduce_motion(),
        })
        .width(Fill)
        .height(Fill),
    )
    .width(Fill)
    .height(42)
    .align_x(iced::Center)
    .align_y(iced::Center)
    .style(theme::sidebar_brand);

    let brand_line = container(Space::new().width(Fill).height(1)).style(theme::divider_accent);

    let profile = profile_block(app, animated);

    let mut nav = Column::new().spacing(2).padding([0.0, 8.0]);
    for section in Section::ALL {
        nav = nav.push(nav_item(app, section));
        // Sub-navegação de Configurações: aparece logo abaixo do item pai
        // quando a tela ativa é Settings, como no `Sidebar.tsx` da web.
        if section == Section::Settings && app.screen == Screen::Section(Section::Settings) {
            nav = nav.push(settings_subnav(app));
        }
    }

    let games = app.library.visible_games(&app.sidebar_search);
    let games_header = container(
        row![
            button(icons::icon(Icon::ChevronDown, 12.0, 0.50))
                .on_press(Message::ToggleSidebarGames)
                .padding(0)
                .style(theme::plain_button),
            text(i18n.t("sidebar.jogos").to_uppercase())
                .size(theme::TEXT_BODY)
                .font(theme::FONT_SEMIBOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.50)),
            container(
                text(games.len().to_string())
                    .size(theme::TEXT_MICRO)
                    .font(theme::FONT_BOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.60)),
            )
            .padding([1.0, 5.0])
            .style(theme::badge),
            Space::new().width(Fill),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding([8.0, 16.0]);

    let selected = Section::ALL
        .iter()
        .position(|&section| is_active(app, section));
    let nav = iced::widget::Stack::new()
        .width(Fill)
        .push(nav)
        .push(
            iced::widget::canvas::Canvas::new(sidebar_indicator::Indicator(selected))
                .width(Fill)
                .height(Fill),
        );
    let mut body = column![brand, brand_line, profile, nav].width(Fill);

    if app.sidebar_games_open {
        let games_search = container(
            text_input(i18n.t("library.buscar"), &app.sidebar_search)
                .on_input(Message::SidebarSearchChanged)
                .padding([5.0, 8.0])
                .size(theme::TEXT_BODY)
                .width(Fill)
                .style(theme::sidebar_search_input),
        )
        .width(Fill)
        .padding([4.0, 12.0]);

        let mut game_list = Column::new().spacing(1).padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 8.0,
            left: 8.0,
        });
        if games.is_empty() {
            game_list = game_list.push(
                container(
                    text(if app.library.games.is_empty() {
                        i18n.t("library.vazia")
                    } else {
                        i18n.t("library.vazio")
                    })
                    .size(theme::TEXT_BODY)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.30))
                    .align_x(iced::Center),
                )
                .width(Fill)
                .padding([16.0, 4.0]),
            );
        } else {
            for game in &games {
                game_list = game_list.push(game_item(app, game));
            }
        }

        body = body
            .push(games_header)
            .push(games_search)
            .push(scrollable(game_list).height(Fill).style(theme::scrollable));
    } else {
        body = body.push(games_header).push(Space::new().height(Fill));
    }

    let big_picture = button(
        row![
            icons::icon(Icon::Monitor, 16.0, 0.75),
            text(i18n.t("sidebar.bigpicture"))
                .size(theme::TEXT_READ)
                .font(theme::FONT_SEMIBOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.70)),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .on_press(Message::EnterBigPicture)
    .width(Fill)
    .padding([9.0, 10.0])
    .style(theme::big_picture);

    let footer = container(big_picture)
        .width(Fill)
        .padding([10.0, 12.0]);

    container(body.push(footer).height(Fill))
        .width(theme::SIDEBAR_WIDTH)
        .height(Fill)
        .style(theme::sidebar)
        .into()
}

/// Sub-navegação expandida de Configurações, porte do bloco `CONFIG_SUBS` do
/// `Sidebar.tsx`: itens de 14 px com ponto/cor de estado, cabeçalho `CLASSICS`
/// antes de Emulação e badge `NEW` nela. O recuo de 36 px alinha os sub-itens
/// sob o texto do item pai, deixando o ícone de engrenagem livre à esquerda.
fn settings_subnav(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let active = app.settings.tab;

    let mut items = Column::new().spacing(2).width(Fill);

    for tab in SettingsTab::ALL {
        if tab.starts_group() {
            items = items.push(
                container(
                    text(i18n.t("settings.emulacao.grupo").to_uppercase())
                        .size(theme::TEXT_MICRO)
                        .font(theme::FONT_SEMIBOLD)
                        .color(Color::from_rgba(1.0, 1.0, 1.0, 0.30)),
                )
                .width(Fill)
                .padding(Padding {
                    top: theme::SPACE_2,
                    right: 0.0,
                    bottom: theme::SPACE_1,
                    left: theme::SPACE_3,
                }),
            );
        }
        items = items.push(settings_subitem(app, tab, active == tab));
    }

    container(items)
        .width(Fill)
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: theme::SPACE_1,
            left: 36.0,
        })
        .into()
}

fn settings_subitem<'a>(app: &'a App, tab: SettingsTab, active: bool) -> Element<'a, Message> {
    let i18n = &app.i18n;

    // `s.icon || <span dot />`: Controle e Emulação usam o ícone de gamepad;
    // os demais, um ponto de 4 px que fica ciano quando ativo.
    let lead: Element<'a, Message> =
        if matches!(tab, SettingsTab::Controller | SettingsTab::Emulation) {
            icons::icon(Icon::Gamepad, 14.0, if active { 1.0 } else { 0.45 })
        } else {
            container(
                Space::new()
                    .width(Length::Fixed(4.0))
                    .height(Length::Fixed(4.0)),
            )
            .style(move |_| theme::sub_item_dot(active))
            .into()
        };

    let mut line = row![
        lead,
        text(tab.label(i18n))
            .size(theme::TEXT_READ)
            .color(if active {
                theme::TEXT_1
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.45)
            })
            .wrapping(Wrapping::None),
        Space::new().width(Fill),
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);

    // Badge `NEW` só em Emulação (`settings.emulacao.novo`).
    if tab == SettingsTab::Emulation {
        line = line.push(
            container(
                text(i18n.t("settings.emulacao.novo"))
                    .size(theme::TEXT_MICRO)
                    .font(theme::FONT_BOLD)
                    .color(theme::ACCENT),
            )
            .padding([1.0, 5.0])
            .style(theme::accent_badge),
        );
    }

    super::cards::hover_motion::wrap(
        button(line)
            .on_press(Message::SettingsTabChanged(tab))
            .width(Fill)
            .padding([5.0, 10.0])
            .style(move |style_theme, status| theme::sub_item(style_theme, status, active)),
        super::cards::hover_motion::Kind::Navigation,
    )
}

fn nav_item(app: &App, section: Section) -> Element<'_, Message> {
    let active = is_active(app, section);
    let marker = Space::new().width(Length::Fixed(2.0)).height(20);

    let label = text(section.label(&app.i18n))
        .size(theme::TEXT_READ)
        .color(if active {
            theme::TEXT_1
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.85)
        });

    super::cards::hover_motion::wrap(
        button(
            row![
                marker,
                icons::icon(icon_for(section), 18.0, if active { 1.0 } else { 0.59 }),
                label,
            ]
            .spacing(10)
            .align_y(iced::Center),
        )
        .on_press(Message::Navigate(section))
        .width(Fill)
        .height(38)
        .padding([0.0, 4.0])
        .style(move |style_theme, status| theme::sidebar_item(style_theme, status, active)),
        super::cards::hover_motion::Kind::Navigation,
    )
}

fn game_item<'a>(app: &'a App, game: &'a crate::backend::Game) -> Element<'a, Message> {
    let active = matches!(&app.screen, Screen::Game(id) if id == &game.id);
    let marker = container(Space::new().width(Length::Fixed(2.0)).height(16)).style(if active {
        theme::accent_bar
    } else {
        theme::transparent_bar
    });

    let cover: Element<'a, Message> = match app.covers.ready(game.cover_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(21)
            .height(21)
            .content_fit(ContentFit::Cover)
            .border_radius(2)
            .into(),
        None => container(
            text(initial(&game.title))
                .size(theme::TEXT_MICRO)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_2),
        )
        .width(21)
        .height(21)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::game_icon_placeholder)
        .into(),
    };

    button(
        row![
            marker,
            cover,
            container(
                text(ellipsize(&game.title, 22))
                    .size(theme::TEXT_READ)
                    .color(if active {
                        theme::TEXT_1
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.75)
                    })
                    .wrapping(Wrapping::None),
            )
            .width(Fill)
            .clip(true),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .on_press(Message::OpenGame(game.id.clone()))
    .width(Fill)
    .padding([4.0, 6.0])
    .style(move |style_theme, status| theme::game_item(style_theme, status, active))
    .into()
}

fn icon_for(section: Section) -> Icon {
    match section {
        Section::Home => Icon::Home,
        Section::Library => Icon::Grid,
        Section::Stores => Icon::Store,
        Section::Components => Icon::Plugin,
        Section::Downloads => Icon::Download,
        Section::Sources => Icon::Link,
        Section::Friends => Icon::Users,
        Section::Settings => Icon::Gear,
        Section::BigPicture => Icon::Monitor,
    }
}

fn ellipsize(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let mut short: String = text.chars().take(limit.saturating_sub(1)).collect();
    short.push('…');
    short
}

fn initial(title: &str) -> String {
    title
        .chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// Bloco "meu perfil" da sidebar — porte do `desktop-sidebar-profile` do
/// `Sidebar.tsx`: identidade clicável que expande o menu (My profile /
/// Refresh library / Settings / Sign out) em vez de ir direto à tela.
fn profile_block<'a>(
    app: &'a App,
    animated: &'a crate::animated_cards::AnimatedCards,
) -> Element<'a, Message> {
    let signed_in = app.account.is_signed_in();
    let name = app.account.display_name(app.i18n.t("native.profile.local"));
    let menu_open = app.profile_menu_open;

    // O avatar do perfil é um GIF animado servido pelo servidor. O frame do
    // pool tem prioridade; `account.avatar` (o primeiro quadro do `avatar:load`)
    // é só o que aparece antes de o pipeline entregar o primeiro frame.
    let avatar_url = app
        .account
        .avatar_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .or(app
            .account
            .fallback_avatar_url
            .as_deref()
            .filter(|url| !url.trim().is_empty()));
    let animated_avatar = avatar_url.and_then(|url| animated.texture(url).cloned());

    let avatar: Element<'a, Message> = match animated_avatar.or(app.account.avatar.clone()) {
        Some(handle) => image(handle)
            .width(40)
            .height(40)
            .content_fit(ContentFit::Cover)
            .border_radius(8)
            .into(),
        None => container(
            text(app.account.initial(app.i18n.t("native.profile.local")))
                .size(theme::TEXT_READ)
                .font(theme::FONT_SEMIBOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.80)),
        )
        .width(40)
        .height(40)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::avatar)
        .into(),
    };

    // Clicar na identidade expande/colapsa o menu (web: setProfileMenu(v => !v)).
    let identity = button(
        row![
            avatar,
            container(
                text(ellipsize(name, 18))
                    .size(theme::TEXT_READ)
                    .font(theme::FONT_MEDIUM)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.80)),
            )
            .width(Fill)
            .clip(true),
            icons::icon(Icon::ChevronDown, 12.0, 0.50),
        ]
        .spacing(12)
        .align_y(iced::Center),
    )
    .on_press(Message::ToggleProfileMenu)
    .width(Fill)
    .padding([2.0, 0.0])
    .style(move |style_theme, status| theme::profile_identity(style_theme, status, menu_open));

    let mut block = column![identity].spacing(8);

    if menu_open {
        let mut menu = Column::new().width(Fill);
        menu = menu.push(menu_item(
            app.i18n.t("profile.meu_perfil"),
            false,
            if signed_in {
                Message::OpenProfile
            } else {
                // Deslogado cai no login, igual `onProfile` do web.
                Message::OpenAuth { required: false }
            },
        ));
        menu = menu.push(menu_item(
            app.i18n.t("settings.title"),
            false,
            Message::OpenSettingsFromProfile,
        ));
        // "Sair": logado sai da conta; deslogado fecha o app (web: signOut/x onQuit).
        menu = menu.push(menu_item(
            if signed_in {
                app.i18n.t("account.sair")
            } else {
                app.i18n.t("profile.sair")
            },
            true,
            if signed_in {
                Message::SignOutRequested
            } else {
                Message::WindowClose
            },
        ));

        block = block.push(container(menu).width(Fill).style(theme::profile_menu));
    }

    container(block)
        .width(Fill)
        .padding(Padding {
            top: 12.0,
            right: 20.0,
            bottom: 12.0,
            left: 20.0,
        })
        .into()
}

/// Um item do menu do perfil — `ProfileMenuItem` do `Sidebar.tsx`.
fn menu_item(label: &str, danger: bool, message: Message) -> Element<'_, Message> {
    button(
        text(label.to_string())
            .size(theme::TEXT_READ)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(message)
    .width(Fill)
    .padding([9.0, 16.0])
    .style(move |style_theme, status| theme::profile_menu_item(style_theme, status, danger))
    .into()
}

fn is_active(app: &App, section: Section) -> bool {
    match &app.screen {
        Screen::Library | Screen::Game(_) => section == Section::Library,
        Screen::Section(current) => *current == section,
        Screen::Profile => false,
        Screen::BigPicture => section == Section::BigPicture,
    }
}
