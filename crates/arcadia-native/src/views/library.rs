//! Biblioteca: mesma top bar de busca, cabeçalho e grade `minmax(170px, 1fr)`
//! da `LibraryView.tsx`, com capa 2:3, título e horas.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space, Stack};
use iced::{Element, Fill, Padding};

use crate::app::{App, LibraryStatus, Message};
use crate::theme;
use crate::views::cards;
use crate::views::icons::{self, Icon};

pub fn view(app: &App) -> Element<'_, Message> {
    let games = app.library.visible_games(&app.search);

    let search = Stack::new()
        .width(Fill)
        .push(
            text_input("Buscar na biblioteca...", &app.search)
                .on_input(Message::SearchChanged)
                .padding(Padding {
                    top: 10.0,
                    right: 14.0,
                    bottom: 10.0,
                    left: 38.0,
                })
                .size(14)
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

    let heading = container(cards::section_heading(
        "Todos os jogos",
        Some(games.len()),
        None,
    ))
    .width(Fill)
    .padding([0.0, theme::PAGE_PADDING]);

    let body: Element<'_, Message> = match &app.library.status {
        LibraryStatus::Loading => centered_message("Carregando biblioteca..."),
        LibraryStatus::Error(error) => error_state(error),
        LibraryStatus::Ready if app.library.games.is_empty() => {
            centered_message("Sua biblioteca está vazia. Adicione jogos no Arcadia web/desktop.")
        }
        LibraryStatus::Ready if games.is_empty() => {
            centered_message("Nenhum jogo corresponde à busca.")
        }
        LibraryStatus::Ready => {
            let cards: Vec<Element<'_, Message>> = games
                .into_iter()
                .map(|game| cards::game_card(game, &app.covers))
                .collect();

            scrollable(
                container(cards::grid(cards, (2.0, 3.55)))
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

    column![search_row, heading, Space::new().height(12), body]
        .width(Fill)
        .height(Fill)
        .into()
}

fn centered_message(message: &str) -> Element<'_, Message> {
    container(
        container(
            text(message)
                .size(14)
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

fn error_state(error: &str) -> Element<'_, Message> {
    container(
        container(
            column![
                text("Não foi possível carregar a biblioteca")
                    .size(20)
                    .font(theme::FONT_BOLD)
                    .color(theme::DANGER),
                text(error).size(13).color(theme::TEXT_2),
                row![
                    button(text("Tentar novamente").size(13).font(theme::FONT_MEDIUM))
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
