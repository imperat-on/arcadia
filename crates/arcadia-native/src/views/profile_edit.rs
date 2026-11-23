//! Editor de perfil — porte da tela `EditProfile.tsx` da web.
//!
//! A web abre esta tela no lugar da `ProfilePage` (é uma página cheia, não um
//! painel dentro do perfil). Antes, o shell nativo empilhava o formulário
//! **abaixo** da grade de jogos do perfil; agora o perfil e o editor são telas
//! distintas, com o mesmo cabeçalho em todas as abas:
//!
//! ```text
//! ┌ ‹ Voltar ao perfil ······································ avatar  nome ┐
//! │ [Geral] [Avatar] [Banner] [Fundo]                                      │
//! ├────────────────────────────────────────────────────────────────────────┤
//! │ conteúdo da aba (cards de 12 px, inputs de 8 px)                      │
//! └────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Os estilos locais desta tela estão em `theme::edit_*` — a geometria curta
//! de 3 px do resto do launcher fica de fora, como no `retro-edit-profile`
//! da web (que também não herda o `rounded` do shell).

use iced::widget::canvas::{
    self, Frame, Geometry, LineCap, LineJoin, Path, Stroke,
};
use iced::widget::{
    button, canvas as canvas_widget, column, container, image, pick_list, row, scrollable, text,
    text_input, Space, Stack,
};
use iced::{
    mouse, Alignment, Color, ContentFit, Element, Fill, Length, Padding, Point, Renderer, Size,
    Theme,
};

use crate::app::{App, Message, ProfileEditTab, ProfileField, ProfileImageKind};
use crate::theme;
use crate::views::back_button;

/// Largura máxima do conteúdo (a referência usa ~1180 px numa janela de 1560).
const CONTENT_MAX_WIDTH: f32 = 1120.0;
/// Altura da faixa de banner nas prévias.
const PREVIEW_BANNER_HEIGHT: f32 = 132.0;
/// Avatar da prévia do perfil.
const PREVIEW_AVATAR: f32 = 60.0;
/// Avatar da identidade no cabeçalho.
const HEADER_AVATAR: f32 = 38.0;

/// Países oferecidos pelo editor — as mesmas chaves do `EditProfile.tsx`.
const COUNTRIES: [&str; 12] = [
    "pais.brasil",
    "pais.portugal",
    "pais.estados_unidos",
    "pais.argentina",
    "pais.chile",
    "pais.mexico",
    "pais.espanha",
    "pais.reino_unido",
    "pais.alemanha",
    "pais.franca",
    "pais.japao",
    "pais.canada",
];

pub fn view(app: &App) -> Element<'_, Message> {
    let content: Element<'_, Message> = match app.account.editor.tab {
        ProfileEditTab::General => general_tab(app),
        ProfileEditTab::Avatar => avatar_tab(app),
        ProfileEditTab::Banner => banner_tab(app),
        ProfileEditTab::Background => background_tab(app),
    };

    container(column![
        header(app),
        scrollable(
            container(content)
                .width(Fill)
                .max_width(CONTENT_MAX_WIDTH)
                .padding(Padding {
                    top: theme::SPACE_6,
                    right: theme::PAGE_PADDING,
                    bottom: theme::SPACE_6 + theme::SPACE_5,
                    left: theme::PAGE_PADDING,
                }),
        )
        .height(Fill)
        .width(Fill)
        .style(theme::scrollable),
    ])
    .width(Fill)
    .height(Fill)
    .style(theme::edit_shell)
    .into()
}

// ── Cabeçalho ───────────────────────────────────────────────────────────────

fn header(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;

    let top = row![
        back_button::labeled(
            i18n.t("editprofile.voltar_perfil"),
            Message::ProfileEditToggled,
            theme::edit_back,
        ),
        Space::new().width(Fill),
        identity(app),
    ]
    .align_y(Alignment::Center)
    .width(Fill);

    let mut tabs = row![].spacing(6).align_y(iced::Top);
    for tab in ProfileEditTab::ALL {
        tabs = tabs.push(tab_button(app, tab));
    }

    container(
        column![top, Space::new().height(theme::SPACE_5), tabs]
            .width(Fill)
            .padding(Padding {
                top: theme::SPACE_4 + 2.0,
                right: theme::PAGE_PADDING,
                bottom: 0.0,
                left: theme::PAGE_PADDING,
            }),
    )
    .width(Fill)
    .style(theme::edit_header)
    .into()
}

/// Avatar + nome no canto direito do cabeçalho.
fn identity(app: &App) -> Element<'_, Message> {
    let name = app.account.display_name(app.i18n.t("profile.jogador"));

    let avatar: Element<'_, Message> = match app.account.avatar.as_ref() {
        Some(handle) => image(handle.clone())
            .width(HEADER_AVATAR)
            .height(HEADER_AVATAR)
            .content_fit(ContentFit::Cover)
            .border_radius(7)
            .into(),
        None => container(
            text(app.account.initial("J"))
                .size(theme::TEXT_BODY)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
        )
        .width(HEADER_AVATAR)
        .height(HEADER_AVATAR)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into(),
    };

    row![
        container(avatar)
            .padding(2.0)
            .style(theme::edit_avatar_ring),
        text(name)
            .size(theme::TEXT_BODY)
            .font(theme::FONT_SEMIBOLD)
            .color(theme::TEXT_1),
    ]
    .spacing(theme::SPACE_3)
    .align_y(Alignment::Center)
    .into()
}

/// Aba horizontal: pill + barrinha de indicação sob o item ativo.
fn tab_button(app: &App, tab: ProfileEditTab) -> Element<'_, Message> {
    let active = app.account.editor.tab == tab;
    let icon_color = if active { theme::ACCENT } else { theme::TEXT_2 };
    let label_color = if active { theme::TEXT_1 } else { theme::TEXT_2 };
    let font = if active {
        theme::FONT_SEMIBOLD
    } else {
        theme::FONT_MEDIUM
    };

    let button = button(
        row![
            tab_icon(tab, icon_color, 17.0),
            text(tab.label(&app.i18n))
                .size(theme::TEXT_BODY)
                .font(font)
                .color(label_color),
        ]
        .spacing(9)
        .align_y(Alignment::Center),
    )
    .on_press(Message::ProfileEditTabChanged(tab))
    .padding(Padding {
        top: 9.0,
        right: 16.0,
        bottom: 9.0,
        left: 13.0,
    })
    .style(move |theme, status| theme::edit_tab(theme, status, active));

    let indicator = container(
        container(Space::new().width(46.0).height(2.0)).style(theme::edit_indicator(active)),
    )
    .width(Fill)
    .align_x(Alignment::Center);

    column![button, Space::new().height(9.0), indicator]
        .width(Length::Shrink)
        .into()
}

fn tab_icon(kind: ProfileEditTab, color: Color, size: f32) -> Element<'static, Message> {
    let kind = match kind {
        ProfileEditTab::General => TabIconKind::Gear,
        ProfileEditTab::Avatar => TabIconKind::Person,
        ProfileEditTab::Banner => TabIconKind::Banner,
        ProfileEditTab::Background => TabIconKind::Picture,
    };
    canvas_widget(TabIcon { kind, color })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

// ── Aba Geral ───────────────────────────────────────────────────────────────

fn general_tab(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let editor = &app.account.editor;

    let basic = card(
        i18n.t("editprofile.informacoes_basicas"),
        i18n.t("editprofile.geral_desc"),
        column![field(
            i18n.t("editprofile.nome_perfil"),
            text_input("", &editor.display_name)
                .on_input(|value| Message::ProfileFieldChanged {
                    field: ProfileField::DisplayName,
                    value,
                })
                .padding([11.0, 14.0])
                .size(theme::TEXT_BODY)
                .style(theme::edit_input)
                .into(),
        )]
        .spacing(theme::SPACE_4)
        .into(),
    );

    let location = card(
        i18n.t("editprofile.localizacao"),
        i18n.t("editprofile.localizacao_desc"),
        row![
            field(i18n.t("editprofile.pais"), country_picker(app)),
            field(
                i18n.t("editprofile.cidade"),
                text_input(i18n.t("editprofile.opcional"), &editor.city)
                    .on_input(|value| Message::ProfileFieldChanged {
                        field: ProfileField::City,
                        value,
                    })
                    .padding([11.0, 14.0])
                    .size(theme::TEXT_BODY)
                    .style(theme::edit_input)
                    .into(),
            ),
        ]
        .spacing(theme::SPACE_4)
        .into(),
    );

    let about = card(
        i18n.t("editprofile.sobre"),
        i18n.t("editprofile.sobre_desc"),
        column![
            field(
                i18n.t("editprofile.resumo"),
                text_input(i18n.t("editprofile.resumo_placeholder"), &editor.summary)
                    .on_input(|value| Message::ProfileFieldChanged {
                        field: ProfileField::Summary,
                        value,
                    })
                    .padding([11.0, 14.0])
                    .size(theme::TEXT_BODY)
                    .style(theme::edit_input)
                    .into(),
            ),
            field(
                i18n.t("native.profile.showcase_title"),
                text_input("", &editor.showcase)
                    .on_input(|value| Message::ProfileFieldChanged {
                        field: ProfileField::Showcase,
                        value,
                    })
                    .padding([11.0, 14.0])
                    .size(theme::TEXT_BODY)
                    .style(theme::edit_input)
                    .into(),
            ),
            text(i18n.t("native.profile.showcase_hint"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        ]
        .spacing(theme::SPACE_4)
        .into(),
    );

    let actions = row![
        save_button(app),
        button(
            text(i18n.t("common.cancelar"))
                .size(theme::TEXT_BODY)
                .font(theme::FONT_MEDIUM)
        )
        .on_press(Message::ProfileEditToggled)
        .padding([11.0, 22.0])
        .style(theme::edit_ghost),
        Space::new().width(Fill),
        status_pill(app),
    ]
    .spacing(theme::SPACE_3)
    .align_y(Alignment::Center)
    .width(Fill);

    let form = column![basic, location, about, actions]
        .spacing(theme::SPACE_5)
        .width(Fill);

    row![form, preview_card(app)]
        .spacing(theme::SPACE_5)
        .width(Fill)
        .into()
}

fn country_picker(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let current = app.account.editor.country.trim().to_string();

    let mut choices: Vec<CountryChoice> = Vec::with_capacity(COUNTRIES.len() + 1);
    choices.push(CountryChoice {
        value: String::new(),
        label: i18n.t("editprofile.nao_exibir").to_string(),
    });
    for key in COUNTRIES {
        choices.push(CountryChoice {
            value: key.to_string(),
            label: i18n.t(key).to_string(),
        });
    }
    // Perfil vindo de outra origem pode ter um país fora da lista: mantém o
    // valor visível em vez de mostrar o placeholder vazio.
    if !current.is_empty() && !choices.iter().any(|choice| choice.value == current) {
        choices.push(CountryChoice {
            value: current.clone(),
            label: current.clone(),
        });
    }

    let selected = choices
        .iter()
        .find(|choice| choice.value == current)
        .cloned();

    pick_list(choices, selected, |choice: CountryChoice| {
        Message::ProfileFieldChanged {
            field: ProfileField::Country,
            value: choice.value,
        }
    })
    .width(Fill)
    .padding([11.0, 14.0])
    .text_size(theme::TEXT_BODY)
    .style(theme::edit_pick_list)
    .menu_style(theme::pick_list_menu)
    .into()
}

fn save_button(app: &App) -> Element<'_, Message> {
    let editor = &app.account.editor;
    let label = if editor.saving {
        app.i18n.t("common.salvando")
    } else {
        app.i18n.t("common.salvar")
    };

    let mut control = button(
        text(label)
            .size(theme::TEXT_BODY)
            .font(theme::FONT_SEMIBOLD),
    )
    .padding([11.0, 24.0])
    .style(theme::edit_primary);

    if !editor.saving {
        control = control.on_press(Message::ProfileSave);
    }

    control.into()
}

/// Pill "Salvo"/"Sincronizado" no rodapé — o `Synced` da referência.
fn status_pill(app: &App) -> Element<'_, Message> {
    let editor = &app.account.editor;
    let (label, color) = if editor.saving {
        (app.i18n.t("common.salvando"), theme::TEXT_2)
    } else if editor.notice.is_some() {
        (app.i18n.t("common.salvo"), theme::SUCCESS)
    } else {
        ("", theme::TEXT_3)
    };

    if label.is_empty() {
        return Space::new().width(0).into();
    }

    container(
        row![
            container(Space::new().width(7.0).height(7.0))
                .style(move |_| theme::console_status_dot(color)),
            text(label)
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_MEDIUM)
                .color(color),
        ]
        .spacing(theme::SPACE_2)
        .align_y(Alignment::Center),
    )
    .padding([7.0, 14.0])
    .style(theme::edit_status_pill)
    .into()
}

/// Card lateral com a prévia do perfil (a coluna "sticky" da referência).
fn preview_card(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let editor = &app.account.editor;
    let banner = app.covers.ready(Some(&editor.banner_url)).cloned();

    let banner_layer: Element<'_, Message> = match banner {
        Some(handle) => image(handle)
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(theme::banner_placeholder)
            .into(),
    };

    let name = if editor.display_name.trim().is_empty() {
        i18n.t("profile.jogador")
    } else {
        editor.display_name.trim()
    };
    let location = [editor.city.trim(), editor.country.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| {
            // O país é gravado como chave de i18n; se for texto livre, fica
            // como veio.
            if part.starts_with("pais.") {
                i18n.t(part).to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    let avatar: Element<'_, Message> = match app.account.avatar.as_ref() {
        Some(handle) => image(handle.clone())
            .width(PREVIEW_AVATAR)
            .height(PREVIEW_AVATAR)
            .content_fit(ContentFit::Cover)
            .border_radius(10)
            .into(),
        None => container(
            text(
                name.chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_else(|| "J".to_string()),
            )
            .size(24)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        )
        .width(PREVIEW_AVATAR)
        .height(PREVIEW_AVATAR)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(theme::avatar)
        .into(),
    };

    let banner_stack = Stack::new()
        .width(Fill)
        .height(Length::Fixed(PREVIEW_BANNER_HEIGHT))
        .push(banner_layer)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::edit_preview_scrim),
        );

    let mut body = column![
        banner_stack,
        container(avatar)
            .padding(2.0)
            .style(theme::edit_avatar_ring),
        text(name.to_string())
            .size(theme::TEXT_TITLE)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1)
            .width(Fill),
        text(if location.is_empty() {
            i18n.t("profile.meu_perfil").to_string()
        } else {
            location
        })
        .size(theme::TEXT_CAPTION)
        .color(theme::TEXT_2)
        .width(Fill),
    ]
    .spacing(theme::SPACE_3)
    .width(Fill);

    if !editor.summary.trim().is_empty() {
        body = body.push(
            text(editor.summary.trim().to_string())
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_2)
                .width(Fill),
        );
    }

    container(
        container(body)
            .width(Fill)
            .padding(Padding {
                top: 0.0,
                right: theme::SPACE_4 + 2.0,
                bottom: theme::SPACE_4 + 2.0,
                left: theme::SPACE_4 + 2.0,
            }),
    )
    .width(Length::Fixed(theme::EDIT_PREVIEW_WIDTH))
    .style(theme::edit_card)
    .into()
}

// ── Aba Avatar ──────────────────────────────────────────────────────────────

fn avatar_tab(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let editor = &app.account.editor;
    let picking = editor.pick_busy.is_some()
        || editor.uploading_avatar
        || editor.uploading_background
        || editor.uploading_banner;

    let preview: Element<'_, Message> = match app.account.avatar.as_ref() {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            text(app.account.initial("J"))
                .size(56)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(theme::avatar)
        .into(),
    };

    let frame = container(preview)
        .width(Length::Fixed(176.0))
        .height(Length::Fixed(176.0))
        .style(if app.account.avatar.is_some() {
            theme::edit_frame
        } else {
            theme::edit_frame_empty
        });

    let requirements = container(
        row![
            container(
                text("i")
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_BOLD)
                    .color(theme::ACCENT),
            )
            .width(34.0)
            .height(34.0)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(theme::edit_badge),
            column![
                text(i18n.t("editprofile.avatar_requisitos"))
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_SEMIBOLD)
                    .color(theme::TEXT_1),
                text(format!(
                    "• {}\n• {}\n• {}",
                    i18n.t("editprofile.avatar_tamanho"),
                    i18n.t("editprofile.avatar_formatos"),
                    i18n.t("editprofile.avatar_recomendacao")
                ))
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_2)
                .width(Fill),
            ]
            .spacing(4)
            .width(Fill),
        ]
        .spacing(theme::SPACE_4)
        .align_y(iced::Top),
    )
    .width(Fill)
    .padding(Padding {
        top: theme::SPACE_4,
        right: theme::SPACE_4,
        bottom: theme::SPACE_4,
        left: theme::SPACE_4,
    })
    .style(theme::edit_subcard);

    let mut actions = column![
        requirements,
        pick_button(
            app,
            Message::PickRequested(crate::picker::PickPurpose::Avatar),
            picking,
        ),
    ]
    .spacing(theme::SPACE_4)
    .width(Fill);

    // Caminho manual: o seletor nativo pode não existir na máquina, e a web
    // também aceita colar um caminho.
    actions = actions.push(
        column![
            container(Space::new().height(1.0))
                .width(Fill)
                .style(theme::edit_hairline),
            text(i18n.t("native.profile.avatar_note"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
            row![
                text_input(
                    i18n.t("native.profile.avatar_path_placeholder"),
                    &editor.avatar_path,
                )
                .on_input(Message::AvatarPathChanged)
                .padding([10.0, 12.0])
                .size(theme::TEXT_CAPTION)
                .style(theme::edit_input)
                .width(Fill),
                button(
                    text(i18n.t("native.profile.use_avatar"))
                        .size(theme::TEXT_CAPTION)
                        .font(theme::FONT_MEDIUM),
                )
                .padding([10.0, 16.0])
                .style(theme::edit_ghost)
                .on_press(Message::AvatarUpload),
            ]
            .spacing(theme::SPACE_2)
            .align_y(Alignment::Center),
        ]
        .spacing(theme::SPACE_2)
        .width(Fill),
    );

    let body = row![
        column![frame, Space::new().height(theme::SPACE_2)].width(Length::Shrink),
        actions,
    ]
    .spacing(theme::SPACE_5 + theme::SPACE_2)
    .align_y(iced::Top)
    .width(Fill);

    centered_card(card(
        i18n.t("editprofile.nav.avatar"),
        i18n.t("editprofile.avatar_desc"),
        body.into(),
    ))
}

// ── Aba Banner ──────────────────────────────────────────────────────────────

fn banner_tab(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let editor = &app.account.editor;
    let handle = app.covers.ready(Some(&editor.banner_url)).cloned();
    let sem_imagem = handle.is_none();

    let layer: Element<'_, Message> = match handle {
        Some(handle) => image(handle)
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => empty_preview(i18n.t("editprofile.sem_banner")),
    };

    let preview = Stack::new()
        .width(Fill)
        .height(Length::Fixed(320.0))
        .push(layer)
        .push(if sem_imagem {
            Space::new().width(Fill).height(Fill).into()
        } else {
            preview_identity(app)
        });

    let mut actions = row![pick_button(
        app,
        Message::PickRequested(crate::picker::PickPurpose::Banner),
        editor.pick_busy.is_some() || editor.uploading_banner,
    )]
    .spacing(theme::SPACE_3);

    if !sem_imagem {
        actions = actions.push(remove_button(app, ProfileImageKind::Banner));
    }

    centered_card(card(
        i18n.t("editprofile.nav.banner"),
        i18n.t("editprofile.banner_desc"),
        column![
            container(preview)
                .width(Fill)
                .style(if sem_imagem {
                    theme::edit_frame_empty
                } else {
                    theme::edit_frame
                }),
            actions.width(Fill),
        ]
        .spacing(theme::SPACE_4)
        .width(Fill)
        .into(),
    ))
}

// ── Aba Fundo ───────────────────────────────────────────────────────────────

fn background_tab(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let editor = &app.account.editor;
    let handle = app.covers.ready(Some(&editor.background_url)).cloned();
    let sem_imagem = handle.is_none();

    let layer: Element<'_, Message> = match handle {
        Some(handle) => image(handle)
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .opacity(0.92_f32)
            .into(),
        None => empty_preview(i18n.t("editprofile.sem_fundo")),
    };

    let mut actions = row![pick_button(
        app,
        Message::PickRequested(crate::picker::PickPurpose::Background),
        editor.pick_busy.is_some() || editor.uploading_background,
    )]
    .spacing(theme::SPACE_3);

    if !sem_imagem {
        actions = actions.push(remove_button(app, ProfileImageKind::Background));
    }

    centered_card(card(
        i18n.t("editprofile.nav.background"),
        i18n.t("editprofile.background_desc"),
        column![
            container(layer)
                .width(Fill)
                .height(Length::Fixed(300.0))
                .style(if sem_imagem {
                    theme::edit_frame_empty
                } else {
                    theme::edit_frame
                }),
            actions,
        ]
        .spacing(theme::SPACE_4)
        .width(Fill)
        .into(),
    ))
}

// ── Peças comuns ────────────────────────────────────────────────────────────

fn centered_card<'a>(card: Element<'a, Message>) -> Element<'a, Message> {
    container(container(card).width(Fill).max_width(920.0))
        .width(Fill)
        .align_x(Alignment::Center)
        .into()
}

/// Card de seção: título grande, subtítulo e corpo.
fn card<'a>(title: &'a str, subtitle: &'a str, body: Element<'a, Message>) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(theme::TEXT_TITLE)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(subtitle)
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_2)
                .width(Fill),
            Space::new().height(theme::SPACE_5 - theme::SPACE_1),
            body,
        ]
        .spacing(theme::SPACE_1)
        .width(Fill),
    )
    .width(Fill)
    .padding(Padding {
        top: theme::SPACE_5,
        right: theme::SPACE_5,
        bottom: theme::SPACE_5,
        left: theme::SPACE_5,
    })
    .style(theme::edit_card)
    .into()
}

/// Rótulo em caixa alta + controle.
fn field<'a>(label: &'a str, control: Element<'a, Message>) -> Element<'a, Message> {
    column![
        text(label.to_uppercase())
            .size(theme::TEXT_MICRO)
            .font(theme::FONT_SEMIBOLD)
            .color(theme::edit_label_color()),
        control,
    ]
    .spacing(theme::SPACE_2)
    .width(Fill)
    .into()
}

/// Botão primário "Escolher imagem / GIF".
fn pick_button(app: &App, message: Message, busy: bool) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let label = if busy {
        i18n.t("native.profile.uploading")
    } else {
        i18n.t("editprofile.escolher_imagem")
    };

    let mut control = button(
        row![
            upload_icon(17.0, theme::TEXT_1),
            text(label)
                .size(theme::TEXT_BODY)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::TEXT_1),
        ]
        .spacing(theme::SPACE_2)
        .align_y(Alignment::Center),
    )
    .padding([12.0, 20.0])
    .style(theme::edit_primary);

    if !busy && app.account.editor.pick_busy.is_none() {
        control = control.on_press(message);
    }

    control.into()
}

fn remove_button(app: &App, kind: ProfileImageKind) -> Element<'_, Message> {
    let busy = *match kind {
        ProfileImageKind::Background => &app.account.editor.uploading_background,
        ProfileImageKind::Banner => &app.account.editor.uploading_banner,
    };

    let mut control = button(
        text(app.i18n.t("editprofile.remover"))
            .size(theme::TEXT_BODY)
            .font(theme::FONT_MEDIUM),
    )
    .padding([12.0, 22.0])
    .style(theme::edit_ghost);

    if !busy {
        control = control.on_press(Message::ProfileImageCleared(kind));
    }

    control.into()
}

/// Miniatura do avatar + nome sobre a prévia do banner (canto inferior
/// esquerdo, como na referência).
fn preview_identity(app: &App) -> Element<'_, Message> {
    let editor = &app.account.editor;
    let name = if editor.display_name.trim().is_empty() {
        app.i18n.t("profile.jogador").to_string()
    } else {
        editor.display_name.trim().to_string()
    };

    let avatar: Element<'_, Message> = match app.account.avatar.as_ref() {
        Some(handle) => image(handle.clone())
            .width(46.0)
            .height(46.0)
            .content_fit(ContentFit::Cover)
            .border_radius(8)
            .into(),
        None => container(
            text(app.account.initial("J"))
                .size(theme::TEXT_HEADING)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
        )
        .width(46.0)
        .height(46.0)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(theme::avatar)
        .into(),
    };

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::edit_preview_scrim),
        )
        .push(
            container(
                row![
                    container(avatar).style(theme::edit_avatar_ring).padding(1.0),
                    text(name)
                        .size(theme::TEXT_HEADING)
                        .font(theme::FONT_BOLD)
                        .color(theme::TEXT_1),
                ]
                .spacing(theme::SPACE_3)
                .align_y(Alignment::Center),
            )
            .width(Fill)
            .height(Fill)
            .align_x(iced::Left)
            .align_y(iced::Bottom)
            .padding(Padding {
                top: 0.0,
                right: theme::SPACE_4,
                bottom: theme::SPACE_4,
                left: theme::SPACE_4,
            }),
        )
        .into()
}

fn empty_preview(message: &str) -> Element<'_, Message> {
    container(
        column![
            picture_icon(34.0, 0.35),
            text(message.to_string())
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_3),
        ]
        .spacing(theme::SPACE_3)
        .align_x(Alignment::Center),
    )
    .width(Fill)
    .height(Fill)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .into()
}

// ── Ícones em canvas ────────────────────────────────────────────────────────
//
// Os PNGs da sidebar são brancos e o iced não tinge imagem: o traço é
// desenhado aqui para acompanhar o acento na aba ativa e ficar nítido em
// qualquer DPI.

#[derive(Debug, Clone, Copy)]
enum TabIconKind {
    Gear,
    Person,
    Banner,
    Picture,
}

#[derive(Debug, Clone, Copy)]
struct TabIcon {
    kind: TabIconKind,
    color: Color,
}

impl<Message> canvas::Program<Message, Theme, Renderer> for TabIcon {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let side = bounds.width.min(bounds.height);
        // Grade de 24 unidades, como os ícones da sidebar.
        let unit = side / 24.0;
        let at = |x: f32, y: f32| Point::new(x * unit, y * unit);
        let stroke = Stroke::default()
            .with_width((side * 0.082).max(1.25))
            .with_color(self.color)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round);

        match self.kind {
            TabIconKind::Gear => {
                let ring = Path::new(|builder| builder.circle(at(12.0, 12.0), 3.6 * unit));
                frame.stroke(&ring, stroke);

                let teeth = Path::new(|builder| {
                    for index in 0..8 {
                        let angle = index as f32 * std::f32::consts::PI / 4.0;
                        let (sin, cos) = angle.sin_cos();
                        builder.move_to(at(12.0 + cos * 6.4, 12.0 + sin * 6.4));
                        builder.line_to(at(12.0 + cos * 8.6, 12.0 + sin * 8.6));
                    }
                });
                frame.stroke(&teeth, stroke);
            }
            TabIconKind::Person => {
                let head = Path::new(|builder| builder.circle(at(12.0, 8.4), 3.5 * unit));
                frame.stroke(&head, stroke);

                let shoulders = Path::new(|builder| {
                    builder.move_to(at(4.8, 19.4));
                    builder.quadratic_curve_to(at(12.0, 12.6), at(19.2, 19.4));
                });
                frame.stroke(&shoulders, stroke);
            }
            TabIconKind::Banner => {
                let box_path = Path::new(|builder| {
                    builder.rounded_rectangle(at(2.6, 5.4), Size::new(18.8 * unit, 13.2 * unit), 2.6.into());
                });
                frame.stroke(&box_path, stroke);

                let lines = Path::new(|builder| {
                    builder.move_to(at(7.0, 10.6));
                    builder.line_to(at(17.0, 10.6));
                    builder.move_to(at(7.0, 14.2));
                    builder.line_to(at(13.0, 14.2));
                });
                frame.stroke(&lines, stroke);
            }
            TabIconKind::Picture => {
                let box_path = Path::new(|builder| {
                    builder.rounded_rectangle(at(2.8, 4.6), Size::new(18.4 * unit, 14.8 * unit), 2.6.into());
                });
                frame.stroke(&box_path, stroke);

                let sun = Path::new(|builder| builder.circle(at(8.4, 9.6), 1.5 * unit));
                frame.stroke(&sun, stroke);

                let hill = Path::new(|builder| {
                    builder.move_to(at(4.6, 17.4));
                    builder.line_to(at(10.2, 12.4));
                    builder.line_to(at(14.0, 15.6));
                    builder.line_to(at(16.2, 13.6));
                    builder.line_to(at(19.6, 16.6));
                });
                frame.stroke(&hill, stroke);
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: iced::Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

/// Ícone de "enviar" do botão de imagem (seta para cima sobre uma base).
fn upload_icon<'a, Message: 'a>(size: f32, color: Color) -> Element<'a, Message> {
    canvas_widget(UploadIcon { color })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

#[derive(Debug, Clone, Copy)]
struct UploadIcon {
    color: Color,
}

impl<Message> canvas::Program<Message, Theme, Renderer> for UploadIcon {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let side = bounds.width.min(bounds.height);
        let unit = side / 24.0;
        let at = |x: f32, y: f32| Point::new(x * unit, y * unit);
        let stroke = Stroke::default()
            .with_width((side * 0.09).max(1.4))
            .with_color(self.color)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round);

        let path = Path::new(|builder| {
            builder.move_to(at(12.0, 15.6));
            builder.line_to(at(12.0, 5.0));
            builder.move_to(at(7.4, 9.4));
            builder.line_to(at(12.0, 4.8));
            builder.line_to(at(16.6, 9.4));
            builder.move_to(at(4.8, 15.2));
            builder.line_to(at(4.8, 19.0));
            builder.line_to(at(19.2, 19.0));
            builder.line_to(at(19.2, 15.2));
        });
        frame.stroke(&path, stroke);

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: iced::Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

fn picture_icon<'a, Message: 'a>(size: f32, opacity: f32) -> Element<'a, Message> {
    let dim = Color::from_rgba(1.0, 1.0, 1.0, opacity);
    canvas_widget(TabIcon {
        kind: TabIconKind::Picture,
        color: dim,
    })
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .into()
}

// ── País da `pick_list` ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct CountryChoice {
    value: String,
    label: String,
}

impl std::fmt::Display for CountryChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}
