//! Perfil da conta logada — porte fiel da `ProfilePage.tsx`.
//!
//! Estrutura da web (desktop, `embedded`):
//!
//! 1. faixa de ações (Editar perfil) no topo;
//! 2. banner largo com avatar sobreposto e o nome;
//! 3. corpo em duas colunas: a biblioteca à esquerda (`minmax(0,1fr)`) e uma
//!    lateral de `340px` com Estatísticas e Amigos.
//!
//! A grade é de capas 2:3 com as horas em cima de cada uma — a biblioteca
//! INTEIRA, mais jogada primeiro, exatamente como o `.sort` da web.

use iced::widget::{button, column, container, image, row, scrollable, text, Space, Stack};
use iced::{ContentFit, Element, Fill, Length, Padding};

use crate::app::{App, LibraryStatus, Message};
use crate::backend::{FriendsLoad, Game};
use crate::covers::CoverCache;
use crate::i18n::I18n;
use crate::theme;
use crate::views::cards;

/// Largura da coluna lateral de Estatísticas/Amigos (`340px` na web).
const SIDEBAR_WIDTH: f32 = 340.0;
/// Altura da faixa de banner (`h-48` da web).
const BANNER_HEIGHT: f32 = 192.0;
/// Avatar sobreposto ao banner (`h-24 w-24`).
const AVATAR_SIZE: f32 = 96.0;
/// Recuo do avatar para cima do banner (`-mt-14` da web).
const AVATAR_OVERLAP: f32 = 56.0;
/// Quantos amigos aparecem na grade (`friends.slice(0, 8)`).
const FRIENDS_SHOWN: usize = 8;

pub fn view<'a>(
    app: &'a App,
    animated: &'a crate::animated_cards::AnimatedCards,
) -> Element<'a, Message> {
    if !app.account.is_signed_in() {
        return signed_out(app);
    }

    // O editor de perfil é uma TELA, não um painel: a web troca a `ProfilePage`
    // pela `EditProfile` inteira. Antes o formulário era empilhado abaixo da
    // grade de jogos, o que enterrava o "editar perfil" no fim da página.
    if app.account.editor.editing {
        return crate::views::profile_edit::view(app);
    }

    let mut content = column![].spacing(theme::SPACE_4).width(Fill);

    // Erros e avisos continuam no topo, acima do banner.
    if let Some(error) = app.account.error.as_ref() {
        content = content.push(crate::views::action_error_banner(error, None));
    }
    if let Some(notice) = app.account.notice.as_ref() {
        content = content.push(success_banner(notice));
    }
    if let Some(error) = app.account.editor.error.as_ref() {
        content = content.push(crate::views::action_error_banner(error, None));
    }
    if let Some(notice) = app.account.editor.notice.as_ref() {
        content = content.push(success_banner(notice));
    }

    content = content
        .push(actions_row(app))
        .push(banner(app, animated))
        .push(identity(app, animated))
        .push(body(app));

    let page = scrollable(
        container(content)
            .width(Fill)
            .padding(Padding {
                top: theme::SPACE_4,
                right: theme::PAGE_PADDING,
                bottom: theme::SPACE_6,
                left: theme::PAGE_PADDING,
            })
            .style(theme::app_background),
    )
    .height(Fill)
    .style(theme::scrollable);

    // Fundo do projeto (`profile.background`): a web pinta como atmosfera de
    // tela inteira, com véu escuro por cima para o banner continuar sendo o
    // destaque. Aqui é a camada de baixo de um `Stack`; o conteúdo fica em
    // cima e o véu entra entre os dois.
    match background_layer(app, animated) {
        Some(layer) => Stack::new()
            .width(Fill)
            .height(Fill)
            .push(layer)
            .push(
                container(Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(theme::profile_scrim),
            )
            .push(page)
            .into(),
        None => page.into(),
    }
}

/// Fundo do perfil (imagem ou GIF animado), já pronto para o `Stack`.
fn background_layer<'a>(
    app: &'a App,
    animated: &'a crate::animated_cards::AnimatedCards,
) -> Option<Element<'a, Message>> {
    let url = app
        .account
        .profile
        .as_ref()
        .and_then(|profile| profile.background_url.as_deref())
        .filter(|url| !url.trim().is_empty())?;

    let handle = animated
        .texture(url)
        .cloned()
        .or_else(|| app.covers.ready(Some(url)).cloned())?;

    Some(
        image(handle)
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
    )
}

/// Tela de quem não está logado.
fn signed_out(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    container(
        column![
            text(i18n.t("native.profile.signin_title"))
                .size(theme::TEXT_TITLE)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(i18n.t("native.profile.signin_desc"))
                .size(theme::TEXT_READ)
                .color(theme::TEXT_2)
                .width(Fill),
            button(
                text(i18n.t("account.entrar_btn"))
                    .size(theme::TEXT_READ)
                    .font(theme::FONT_SEMIBOLD)
            )
            .on_press(Message::OpenAuth { required: false })
            .padding([10.0, 18.0])
            .style(theme::primary_button),
        ]
        .spacing(theme::SPACE_3)
        .max_width(520),
    )
    .width(Fill)
    .height(Fill)
    .align_x(iced::Center)
    .align_y(iced::Center)
    .padding(theme::PAGE_PADDING * 2.0)
    .into()
}

/// Faixa de ações do topo: só "Editar perfil" à direita, como a web.
///
/// O botão "Atualizar" saiu daqui: a `ProfilePage.tsx` não tem recarregar na
/// faixa de ações, e o perfil já recarrega ao abrir (`OpenProfile` dispara
/// `profile:stats` e o fetch do banner).
fn actions_row(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let edit = button(
        text(i18n.t("profile.editar_perfil"))
            .size(theme::TEXT_BODY)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_1),
    )
    .on_press(Message::ProfileEditToggled)
    .padding([8.0, 16.0])
    .style(theme::ghost_button);

    row![Space::new().width(Fill), edit]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center)
        .width(Fill)
        .into()
}

/// Banner largo com a imagem de `banner_url`.
///
/// O frame animado do pool tem prioridade: a web serve o banner como GIF e o
/// estático (`covers`) é só o primeiro quadro.
fn banner<'a>(
    app: &'a App,
    animated: &'a crate::animated_cards::AnimatedCards,
) -> Element<'a, Message> {
    let banner_url = app
        .account
        .profile
        .as_ref()
        .and_then(|profile| profile.banner_url.as_deref())
        .filter(|url| !url.trim().is_empty());

    let handle = banner_url
        .and_then(|url| animated.texture(url).cloned())
        .or_else(|| banner_url.and_then(|url| app.covers.ready(Some(url)).cloned()));

    let inner: Element<'_, Message> = match handle {
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

    container(inner)
        .width(Fill)
        .height(Length::Fixed(BANNER_HEIGHT))
        .style(theme::profile_banner)
        .into()
}

/// Avatar sobreposto ao banner + nome.
fn identity<'a>(
    app: &'a App,
    animated: &'a crate::animated_cards::AnimatedCards,
) -> Element<'a, Message> {
    let name = app
        .account
        .profile
        .as_ref()
        .map(|profile| profile.display())
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            app.account
                .session
                .as_ref()
                .and_then(|session| session.username.as_deref())
        })
        .unwrap_or_else(|| app.i18n.t("profile.jogador"));

    // Frame animado do GIF tem prioridade sobre o estático do `avatar:load`.
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
    let still_avatar = app.account.avatar.clone();

    let avatar: Element<'a, Message> = match animated_avatar.or(still_avatar) {
        Some(handle) => image(handle)
            .width(AVATAR_SIZE)
            .height(AVATAR_SIZE)
            .content_fit(ContentFit::Cover)
            .border_radius(16)
            .into(),
        None => container(
            text(app.account.initial("?"))
                .size(30)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
        )
        .width(AVATAR_SIZE)
        .height(AVATAR_SIZE)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::avatar)
        .into(),
    };

    // O avatar sobe metade sobre o banner (`-mt-14`): a coluna abre com um
    // espaço equivalente e só então desenha a linha.
    container(
        column![
            Space::new().height(AVATAR_OVERLAP),
            row![
                avatar,
                text(name)
                    .size(30)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1)
                    .width(Fill),
            ]
            .spacing(theme::SPACE_4)
            .align_y(iced::Bottom),
        ]
        .width(Fill),
    )
    .width(Fill)
    .padding(Padding {
        top: 0.0,
        right: theme::SPACE_4,
        bottom: 0.0,
        left: theme::SPACE_4,
    })
    .into()
}

/// Corpo: biblioteca à esquerda, Estatísticas + Amigos à direita.
fn body(app: &App) -> Element<'_, Message> {
    row![library_section(app), sidebar(app)]
        .spacing(theme::SPACE_5)
        .width(Fill)
        .into()
}

/// `profile.todos_jogos` + contagem + grade de capas 2:3.
fn library_section(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let games = profile_games(app);

    let heading = row![
        text(i18n.t("profile.todos_jogos"))
            .size(20)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        Space::new().width(Fill),
        container(
            text(games_count_label(i18n, games.len()))
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::TEXT_3),
        )
        .padding([3.0, 10.0])
        .style(theme::badge),
    ]
    .spacing(theme::SPACE_3)
    .align_y(iced::Center)
    .width(Fill);

    let grid: Element<'_, Message> = match &app.library.status {
        LibraryStatus::Loading => hint(i18n.t("common.carregando")),
        LibraryStatus::Error(error) => hint(error),
        LibraryStatus::Ready if games.is_empty() => container(
            text(i18n.t("profile.nenhum_jogado"))
                .size(theme::TEXT_READ)
                .color(theme::TEXT_2),
        )
        .width(Fill)
        .padding([48.0, 32.0])
        .style(theme::stub_panel)
        .into(),
        LibraryStatus::Ready => {
            let tiles: Vec<Element<'_, Message>> = games
                .into_iter()
                .map(|game| {
                    profile_tile(game, &app.covers, Some(Message::OpenGame(game.id.clone())))
                })
                .collect();
            cards::grid(tiles, (2.0, 3.0))
        }
    };

    column![heading, grid]
        .spacing(theme::SPACE_3)
        .width(Fill)
        .into()
}

/// Jogos do perfil: só os com capa e não ocultos, mais jogados primeiro — o
/// mesmo filtro e o mesmo `sort` do `ProfilePage.tsx`.
fn profile_games(app: &App) -> Vec<&Game> {
    let mut games: Vec<&Game> = app
        .library
        .games
        .iter()
        .filter(|game| game.cover_url.is_some() && !game.hidden)
        .collect();
    games.sort_by(|a, b| b.playtime_minutes.cmp(&a.playtime_minutes));
    games
}

/// `profile.todos_jogos_contagem` com o número no lugar do `{count}`.
pub(super) fn games_count_label(i18n: &I18n, count: usize) -> String {
    i18n.t("profile.todos_jogos_contagem")
        .replace("{count}", &count.to_string())
}

/// Capa 2:3 com o badge de horas no canto superior esquerdo.
pub(super) fn profile_tile<'a>(
    game: &'a Game,
    covers: &'a CoverCache,
    action: Option<Message>,
) -> Element<'a, Message> {
    let cover: Element<'a, Message> = match covers.ready(game.cover_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .border_radius(8)
            .into(),
        None => container(
            text(
                game.title
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default(),
            )
            .size(30)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_3),
        )
        .width(Fill)
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::cover_placeholder)
        .into(),
    };

    let stacked = Stack::new().width(Fill).height(Fill).push(cover).push(
        container(hours_badge(game, action.is_none()))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Left)
            .align_y(iced::Top)
            .padding(Padding {
                top: 6.0,
                right: 0.0,
                bottom: 0.0,
                left: 6.0,
            }),
    );

    button(container(stacked).width(Fill).height(Fill))
        .on_press_maybe(action)
        .padding(0)
        .style(theme::tile_button)
        .into()
}

/// Badge `126h` sobre a capa (`HorasBadge` da web).
fn hours_badge(game: &Game, read_only: bool) -> Element<'_, Message> {
    container(
        text(if read_only && game.playtime_minutes == 0 {
            "0min".to_string()
        } else {
            game.playtime_label()
        })
        .size(theme::TEXT_MICRO)
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1),
    )
    .padding([3.0, 6.0])
    .style(theme::hours_badge)
    .into()
}

/// Coluna direita: Estatísticas e Amigos (`aside` da web).
fn sidebar(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let friends = match app.friends.data.as_ref() {
        Some(FriendsLoad::Ready(data)) => data.friends.as_slice(),
        _ => &[][..],
    };

    let stats_card = profile_card(
        i18n.t("profile.estatisticas"),
        column![
            stat_row(
                i18n.t("profile.estatisticas.jogos"),
                app.library.games.len().to_string(),
            ),
            stat_row(
                i18n.t("profile.estatisticas.horas"),
                match app.account.stats.as_ref() {
                    Some(stats) => i18n
                        .t("profile.estatisticas.horas_display")
                        .replace("{h}", &stats.playtime_hours.to_string()),
                    None => i18n.t("profile.estatisticas.fallback").to_string(),
                },
            ),
            stat_row(i18n.t("amigos.titulo"), friends.len().to_string()),
        ]
        .spacing(theme::SPACE_2)
        .width(Fill)
        .into(),
    );

    let friends_body: Element<'_, Message> = if friends.is_empty() {
        text(i18n.t("profile.vitrine_vazia"))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .width(Fill)
            .into()
    } else {
        let tiles: Vec<Element<'_, Message>> = friends
            .iter()
            .take(FRIENDS_SHOWN)
            .map(friend_avatar)
            .collect();
        cards::grid(tiles, (1.0, 1.0))
    };

    column![
        stats_card,
        profile_card(i18n.t("amigos.titulo"), friends_body)
    ]
    .spacing(theme::SPACE_4)
    .width(Length::Fixed(SIDEBAR_WIDTH))
    .into()
}

/// Card com cabeçalho e corpo (`ProfileCard` da web).
pub(super) fn profile_card<'a>(title: &'a str, body: Element<'a, Message>) -> Element<'a, Message> {
    container(
        column![
            container(
                text(title)
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1),
            )
            .width(Fill)
            .padding([10.0, 16.0])
            .style(theme::card_header),
            container(body).width(Fill).padding(theme::SPACE_4),
        ]
        .width(Fill),
    )
    .width(Fill)
    .style(theme::panel)
    .into()
}

/// Linha `rótulo ............ valor` (`StatRow` da web).
pub(super) fn stat_row<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    container(
        row![
            text(label).size(theme::TEXT_BODY).color(theme::TEXT_STRONG),
            Space::new().width(Fill),
            text(value)
                .size(theme::TEXT_BODY)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
        ]
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding([8.0, 12.0])
    .style(theme::stat_row)
    .into()
}

/// Avatar quadrado de amigo (`FriendAvatar` da web). Os avatares remotos
/// ficam na inicial, igual à lista de amigos já existente.
fn friend_avatar(friend: &crate::backend::FriendItem) -> Element<'_, Message> {
    container(
        text(
            friend
                .display()
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "?".to_string()),
        )
        .size(theme::TEXT_HEADING)
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1),
    )
    .width(Fill)
    .height(Fill)
    .align_x(iced::Center)
    .align_y(iced::Center)
    .style(theme::avatar)
    .into()
}

fn hint(message: &str) -> Element<'_, Message> {
    container(
        text(message)
            .size(theme::TEXT_READ)
            .color(theme::TEXT_3)
            .width(Fill),
    )
    .width(Fill)
    .padding([32.0, 24.0])
    .style(theme::stub_panel)
    .into()
}

fn success_banner(message: &str) -> Element<'_, Message> {
    container(
        text(message)
            .size(theme::TEXT_BODY)
            .color(theme::SUCCESS)
            .width(Fill),
    )
    .width(Fill)
    .padding([10.0, 14.0])
    .style(theme::stub_panel)
    .into()
}
