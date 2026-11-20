//! Casca cinemática compartilhada pela ficha da biblioteca e da loja.
//!
//! O desenho segue a `StoreGamePage.tsx`: backdrop largo, capa retrato
//! flutuante, título grande com chips, botão voltar fixo em vidro e corpo em
//! duas colunas. A página rola por inteiro, mas o voltar fica sempre acessível.

use iced::widget::{button, column, container, image, row, scrollable, text, Space, Stack};
use iced::{ContentFit, Element, Fill, Length, Padding};

use crate::app::Message;
use crate::covers::CoverCache;
use crate::theme;

const PORTRAIT_WIDTH: f32 = 150.0;
const PORTRAIT_HEIGHT: f32 = 225.0;

pub(crate) fn view<'a>(
    hero_height: f32,
    covers: &'a CoverCache,
    backdrop: Option<String>,
    portrait: Option<String>,
    title: &'a str,
    chips: Vec<Element<'a, Message>>,
    back: Message,
    actions: Option<Element<'a, Message>>,
    left: Vec<Element<'a, Message>>,
    right: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let hero_height = hero_height.clamp(300.0, 430.0);

    let art: Element<'a, Message> = match covers.ready(backdrop.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(theme::cover_placeholder)
            .into(),
    };

    let portrait: Element<'a, Message> = match covers.ready(portrait.as_deref()) {
        Some(handle) => container(
            image(handle.clone())
                .width(Fill)
                .height(Fill)
                .content_fit(ContentFit::Cover),
        )
        .width(Length::Fixed(PORTRAIT_WIDTH))
        .height(Length::Fixed(PORTRAIT_HEIGHT))
        .clip(true)
        .style(theme::portrait_frame)
        .into(),
        None => container(
            text(initial(title))
                .size(44)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_3),
        )
        .width(Length::Fixed(PORTRAIT_WIDTH))
        .height(Length::Fixed(PORTRAIT_HEIGHT))
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::portrait_frame)
        .into(),
    };

    let mut copy = column![text(title)
        .size(42)
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1)
        .wrapping(iced::widget::text::Wrapping::WordOrGlyph)]
    .spacing(12)
    .width(Fill);

    if !chips.is_empty() {
        let mut chip_row = row![].spacing(8).align_y(iced::Center);
        for chip in chips {
            chip_row = chip_row.push(chip);
        }
        copy = copy.push(chip_row);
    }

    let bottom = container(
        row![portrait, copy]
            .spacing(theme::SPACE_5)
            .align_y(iced::Bottom)
            .width(Fill),
    )
    .width(Fill)
    .height(Fill)
    .align_y(iced::Bottom)
    .padding(Padding {
        top: 0.0,
        right: theme::PAGE_PADDING,
        bottom: theme::SPACE_5,
        left: theme::PAGE_PADDING,
    });

    let hero = Stack::new()
        .width(Fill)
        .height(Length::Fixed(hero_height))
        .push(art)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::hero_overlay),
        )
        .push(bottom);

    let mut content = column![hero].width(Fill);

    if let Some(actions) = actions {
        content = content.push(
            container(
                row![Space::new().width(Fill), actions]
                    .spacing(theme::SPACE_2)
                    .align_y(iced::Center),
            )
            .width(Fill)
            .padding([theme::SPACE_3, theme::PAGE_PADDING])
            .style(theme::action_bar),
        );
    }

    let body = if right.is_empty() {
        row![column(left).spacing(theme::SPACE_4).width(Fill)]
            .spacing(theme::SPACE_5)
            .align_y(iced::Top)
            .width(Fill)
    } else {
        row![
            column(left).spacing(theme::SPACE_4).width(iced::FillPortion(2)),
            column(right).spacing(theme::SPACE_4).width(iced::FillPortion(1)),
        ]
        .spacing(theme::SPACE_5)
        .align_y(iced::Top)
        .width(Fill)
    };

    content = content.push(
        container(body)
            .width(Fill)
            .max_width(1400.0)
            .padding([theme::SPACE_5, theme::PAGE_PADDING]),
    );

    let page = scrollable(content)
        .width(Fill)
        .height(Fill)
        .style(theme::scrollable);

    let back = button(text("‹").size(24).font(theme::FONT_MEDIUM).color(theme::TEXT_1))
        .on_press(back)
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0))
        .padding(0)
        .style(theme::circle_button);

    let back_layer = container(back)
        .width(Fill)
        .height(Fill)
        .align_x(iced::Left)
        .align_y(iced::Top)
        .padding(20);

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(page)
        .push(back_layer)
        .into()
}

pub(crate) fn chip<'a>(label: impl Into<String>, accent: bool) -> Element<'a, Message> {
    let mut label = label.into();
    if label.chars().count() > 34 {
        label = label.chars().take(33).collect::<String>();
        label.push('…');
    }
    container(
        text(label)
            .size(12)
            .font(theme::FONT_MEDIUM)
            .color(if accent { theme::ACCENT } else { theme::TEXT_STRONG }),
    )
    .padding([5.0, 10.0])
    .style(if accent {
        theme::chip_accent
    } else {
        theme::chip
    })
    .into()
}

fn initial(title: &str) -> String {
    title
        .chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

