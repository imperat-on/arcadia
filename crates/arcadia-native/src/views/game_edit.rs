//! Diálogos de "Editar App/Jogo" e "Metadados" do menu da biblioteca.
//!
//! Ambos produzem apenas os patches necessários: nada aqui toca `installed`,
//! `launcher`, `launch_cmd` ou dispara instalador. A gravação e a recarga
//! enriquecida da biblioteca ficam no `App`.

use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, text, text_input, Space,
    Stack,
};
use iced::{Border, Color, Element, Fill, Length, Padding, Shadow, Size, Vector};

use crate::app::Message;
use crate::backend::Game;
use crate::i18n::I18n;
use crate::picker::{ArtworkKind, PickPurpose};
use crate::theme;
use crate::views::game_menu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPlatform {
    Windows,
    Linux,
}

impl EditPlatform {
    pub fn from_value(value: Option<&str>) -> Option<Self> {
        match value.unwrap_or("windows") {
            "windows" => Some(Self::Windows),
            "linux" => Some(Self::Linux),
            _ => None,
        }
    }

    /// O `pick_list` entrega o rótulo exibido ("Windows"/"Linux"), nunca o
    /// valor persistido. Converter aqui evita perder a escolha do usuário.
    pub fn from_label(label: &str) -> Option<Self> {
        let value = label.trim().to_ascii_lowercase();
        Self::from_value(Some(&value))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Linux => "Linux",
        }
    }

    pub fn value(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}

/// Rascunho do editor de App/Jogo.
#[derive(Debug, Clone)]
pub struct GameEditState {
    pub game_id: String,
    pub title: String,
    pub original_title: String,
    pub description: String,
    pub original_description: Option<String>,
    pub description_reset: bool,
    pub platform: Option<EditPlatform>,
    pub original_platform: Option<String>,
    pub exe: String,
    pub original_exe: Option<String>,
    pub is_custom: bool,
    pub saving: bool,
    pub request: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomGamePatch {
    pub title: Option<String>,
    pub platform: Option<String>,
    pub exe: Option<String>,
}

impl CustomGamePatch {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.platform.is_none() && self.exe.is_none()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditPayload {
    pub custom: CustomGamePatch,
    pub overrides: serde_json::Value,
}

impl EditPayload {
    pub fn is_empty(&self) -> bool {
        self.custom.is_empty() && self.overrides.as_object().is_some_and(|map| map.is_empty())
    }
}

impl GameEditState {
    pub fn new(game: &Game) -> Self {
        let is_custom = game.launcher.eq_ignore_ascii_case("custom");
        let platform = EditPlatform::from_value(game.platform.as_deref());
        Self {
            game_id: game.id.clone(),
            title: game.title.clone(),
            original_title: game.title.clone(),
            description: game.description.clone().unwrap_or_default(),
            original_description: game.description.clone(),
            description_reset: false,
            platform,
            original_platform: game.platform.clone(),
            exe: game.exe.clone().unwrap_or_default(),
            original_exe: game.exe.clone(),
            is_custom,
            saving: false,
            request: 0,
            error: None,
        }
    }

    /// Valida o rascunho; devolve a chave de erro de i18n quando inválido.
    pub fn validate(&self) -> Option<&'static str> {
        if self.title.trim().is_empty() {
            return Some("addgame.erro_titulo");
        }
        if self.is_custom
            && self.platform.is_some()
            && self.exe.trim().is_empty()
            && self.original_exe.is_some()
        {
            return Some("addgame.erro_exe");
        }
        None
    }

    /// Campos para `customgame:update`; vazio para jogos de loja.
    pub fn custom_patch(&self) -> CustomGamePatch {
        if !self.is_custom {
            return CustomGamePatch {
                title: None,
                platform: None,
                exe: None,
            };
        }
        let title = (self.title.trim() != self.original_title)
            .then(|| self.title.trim().to_string())
            .filter(|title| !title.is_empty());
        let platform = self.platform.map(EditPlatform::value).and_then(|value| {
            (self.original_platform.as_deref() != Some(value)).then(|| value.to_string())
        });
        let exe = if self.platform.is_some() {
            let trimmed = self.exe.trim();
            (!trimmed.is_empty() && self.original_exe.as_deref() != Some(trimmed))
                .then(|| trimmed.to_string())
        } else {
            None
        };
        CustomGamePatch {
            title,
            platform,
            exe,
        }
    }

    /// Patch de `overrides:set`: título (para jogos de loja) e descrição.
    /// `null` só aparece no reset explícito da descrição.
    pub fn overrides_patch(&self) -> serde_json::Value {
        let mut patch = serde_json::Map::new();
        if !self.is_custom && self.title.trim() != self.original_title {
            if !self.title.trim().is_empty() {
                patch.insert("title".into(), self.title.trim().into());
            }
        }
        if self.description_reset {
            patch.insert("description".into(), serde_json::Value::Null);
        } else if self.description != self.original_description.clone().unwrap_or_default() {
            patch.insert("description".into(), self.description.clone().into());
        }
        serde_json::Value::Object(patch)
    }

    pub fn payload(&self) -> EditPayload {
        EditPayload {
            custom: self.custom_patch(),
            overrides: self.overrides_patch(),
        }
    }
}

/// Rascunho de uma arte: intocado, reset explícito ou caminho novo.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ArtDraft {
    #[default]
    Unchanged,
    Reset,
    Path(String),
}

impl ArtDraft {
    fn value(&self) -> Option<serde_json::Value> {
        match self {
            Self::Unchanged => None,
            Self::Reset => Some(serde_json::Value::Null),
            Self::Path(path) => Some(serde_json::Value::String(path.clone())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetadataState {
    pub game_id: String,
    pub title: String,
    pub original_title: String,
    pub description: String,
    pub original_description: Option<String>,
    pub description_reset: bool,
    pub cover: ArtDraft,
    pub original_cover: Option<String>,
    pub hero: ArtDraft,
    pub original_hero: Option<String>,
    pub logo: ArtDraft,
    pub original_logo: Option<String>,
    pub pick_busy: Option<PickPurpose>,
    pub saving: bool,
    pub request: u64,
    pub error: Option<String>,
}

impl MetadataState {
    pub fn new(game: &Game) -> Self {
        Self {
            game_id: game.id.clone(),
            title: game.title.clone(),
            original_title: game.title.clone(),
            description: game.description.clone().unwrap_or_default(),
            original_description: game.description.clone(),
            description_reset: false,
            cover: ArtDraft::Unchanged,
            original_cover: game.cover_url.clone(),
            hero: ArtDraft::Unchanged,
            original_hero: game.hero_url.clone(),
            logo: ArtDraft::Unchanged,
            original_logo: game.logo_url.clone(),
            pick_busy: None,
            saving: false,
            request: 0,
            error: None,
        }
    }

    pub fn set_art(&mut self, kind: ArtworkKind, path: std::path::PathBuf) {
        let path = path.to_string_lossy().into_owned();
        match kind {
            ArtworkKind::Cover => self.cover = ArtDraft::Path(path),
            ArtworkKind::Hero => self.hero = ArtDraft::Path(path),
            ArtworkKind::Logo => self.logo = ArtDraft::Path(path),
        }
    }

    pub fn reset_art(&mut self, kind: ArtworkKind) {
        match kind {
            ArtworkKind::Cover => self.cover = ArtDraft::Reset,
            ArtworkKind::Hero => self.hero = ArtDraft::Reset,
            ArtworkKind::Logo => self.logo = ArtDraft::Reset,
        }
    }

    pub fn validate(&self) -> Option<&'static str> {
        if self.title.trim().is_empty() {
            Some("addgame.erro_titulo")
        } else {
            None
        }
    }

    /// Patch de `overrides:set` para `EditMetadata.tsx`: título/descrição e
    /// as três artes. Descrição vazia (texto "") e reset (`null`) são coisas
    /// diferentes de propósito.
    pub fn patch(&self) -> serde_json::Value {
        let mut patch = serde_json::Map::new();
        if !self.title.trim().is_empty() && self.title.trim() != self.original_title {
            patch.insert("title".into(), self.title.trim().into());
        }
        if self.description_reset {
            patch.insert("description".into(), serde_json::Value::Null);
        } else if self.description != self.original_description.clone().unwrap_or_default() {
            patch.insert("description".into(), self.description.clone().into());
        }
        if let Some(value) = self.cover.value() {
            patch.insert("cover".into(), value);
        }
        if let Some(value) = self.hero.value() {
            patch.insert("hero".into(), value);
        }
        if let Some(value) = self.logo.value() {
            patch.insert("logo".into(), value);
        }
        serde_json::Value::Object(patch)
    }
}

// ── Views ───────────────────────────────────────────────────────────────────

pub fn edit_overlay<'a>(
    i18n: &'a I18n,
    state: &'a GameEditState,
    window: Size,
) -> Element<'a, Message> {
    let mut body = column![].spacing(theme::SPACE_4);
    if let Some(error) = state.error.as_deref() {
        body = body.push(error_banner(error));
    }
    body = body.push(field(
        i18n.t("editmetadata.titulo"),
        text_input(i18n.t("editmetadata.titulo"), &state.title)
            .on_input(Message::GameEditTitleChanged)
            .padding(10)
            .size(theme::TEXT_BODY)
            .width(Fill)
            .style(theme::search_input)
            .into(),
    ));
    body = body.push(field(
        i18n.t("editmetadata.descricao"),
        column![
            text_input(i18n.t("editmetadata.descricao"), &state.description)
                .on_input(Message::GameEditDescriptionChanged)
                .padding(10)
                .size(theme::TEXT_BODY)
                .width(Fill)
                .style(theme::search_input),
            row![
                text(if state.description_reset {
                    i18n.t("editmetadata.volta_original")
                } else {
                    ""
                })
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
                Space::new().width(Fill),
                button(text(i18n.t("editmetadata.restaurar")).size(theme::TEXT_MICRO))
                    .on_press(Message::GameEditDescriptionReset)
                    .padding([5.0, 9.0])
                    .style(theme::ghost_button),
            ]
            .align_y(iced::Center),
        ]
        .spacing(4)
        .into(),
    ));
    if state.is_custom && state.platform.is_some() {
        let current = state.platform.map(EditPlatform::label);
        const PLATFORMS: [&str; 2] = ["Windows", "Linux"];
        body = body.push(field(
            i18n.t("native.library.edit_platform"),
            pick_list(&PLATFORMS[..], current, |label| {
                Message::GameEditPlatformChanged(
                    EditPlatform::from_label(label).unwrap_or(EditPlatform::Windows),
                )
            })
            .width(Fill)
            .padding(9)
            .text_size(theme::TEXT_BODY)
            .into(),
        ));
        body = body.push(field(
            i18n.t("native.library.edit_executable"),
            text_input(i18n.t("native.library.edit_executable"), &state.exe)
                .on_input(Message::GameEditExeChanged)
                .padding(10)
                .size(theme::TEXT_BODY)
                .width(Fill)
                .style(theme::search_input)
                .into(),
        ));
    }

    dialog(
        i18n,
        i18n.t_args("addgame.editar_jogo", &[("title", &state.original_title)]),
        body.into(),
        Message::GameEditCancel,
        Message::GameEditSave,
        state.saving,
        window,
    )
}

pub fn metadata_overlay<'a>(
    i18n: &'a I18n,
    state: &'a MetadataState,
    window: Size,
) -> Element<'a, Message> {
    let mut body = column![].spacing(theme::SPACE_4);
    if let Some(error) = state.error.as_deref() {
        body = body.push(error_banner(error));
    }
    body = body.push(field(
        i18n.t("editmetadata.titulo"),
        text_input(i18n.t("editmetadata.titulo"), &state.title)
            .on_input(Message::GameMetadataTitleChanged)
            .padding(10)
            .size(theme::TEXT_BODY)
            .width(Fill)
            .style(theme::search_input)
            .into(),
    ));
    body = body.push(field(
        i18n.t("editmetadata.descricao"),
        column![
            text_input(i18n.t("editmetadata.descricao"), &state.description)
                .on_input(Message::GameMetadataDescriptionChanged)
                .padding(10)
                .size(theme::TEXT_BODY)
                .width(Fill)
                .style(theme::search_input),
            row![
                text(if state.description_reset {
                    i18n.t("editmetadata.volta_original")
                } else {
                    ""
                })
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
                Space::new().width(Fill),
                button(text(i18n.t("editmetadata.restaurar")).size(theme::TEXT_MICRO))
                    .on_press(Message::GameMetadataDescriptionReset)
                    .padding([5.0, 9.0])
                    .style(theme::ghost_button),
            ]
            .align_y(iced::Center),
        ]
        .spacing(4)
        .into(),
    ));

    body = body.push(art_row(
        i18n,
        state,
        ArtworkKind::Cover,
        i18n.t("editmetadata.capa"),
        &state.cover,
        state.original_cover.as_deref(),
    ));
    body = body.push(art_row(
        i18n,
        state,
        ArtworkKind::Hero,
        i18n.t("editmetadata.fundo"),
        &state.hero,
        state.original_hero.as_deref(),
    ));
    body = body.push(art_row(
        i18n,
        state,
        ArtworkKind::Logo,
        i18n.t("editmetadata.logo"),
        &state.logo,
        state.original_logo.as_deref(),
    ));

    dialog(
        i18n,
        i18n.t_args(
            "editmetadata.editar_metadados",
            &[("title", &state.original_title)],
        ),
        body.into(),
        Message::GameMetadataCancel,
        Message::GameMetadataSave,
        state.saving,
        window,
    )
}

fn art_row<'a>(
    i18n: &'a I18n,
    state: &'a MetadataState,
    kind: ArtworkKind,
    label: &'a str,
    draft: &'a ArtDraft,
    original: Option<&'a str>,
) -> Element<'a, Message> {
    let current = match draft {
        ArtDraft::Reset => i18n.t("editmetadata.volta_original").to_string(),
        ArtDraft::Path(path) => shorten(path, 64),
        ArtDraft::Unchanged => original
            .map(|path| shorten(path, 64))
            .unwrap_or_else(|| i18n.t("editmetadata.sem_imagem").to_string()),
    };
    let picking = state.pick_busy.is_some();
    let pick = button(
        text(if picking {
            i18n.t("editmetadata.procurando")
        } else {
            i18n.t("editmetadata.do_disco")
        })
        .size(theme::TEXT_MICRO),
    )
    .on_press_maybe(
        (!picking && !state.saving).then_some(Message::PickRequested(PickPurpose::Artwork {
            game_id: state.game_id.clone(),
            kind,
        })),
    )
    .padding([5.0, 9.0])
    .style(theme::ghost_button);
    let reset = button(text(i18n.t("editmetadata.restaurar")).size(theme::TEXT_MICRO))
        .on_press_maybe((!state.saving).then_some(Message::GameMetadataArtReset(kind)))
        .padding([5.0, 9.0])
        .style(theme::ghost_button);

    container(
        column![
            text(label)
                .size(theme::TEXT_MICRO)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::TEXT_2),
            text(current)
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3)
                .width(Fill),
            row![pick, reset].spacing(6),
        ]
        .spacing(4),
    )
    .width(Fill)
    .padding([8.0, 10.0])
    .style(theme::stub_panel)
    .into()
}

fn dialog<'a>(
    i18n: &'a I18n,
    title: String,
    body: Element<'a, Message>,
    cancel: Message,
    save: Message,
    saving: bool,
    window: Size,
) -> Element<'a, Message> {
    let panel = game_menu::capture_press(
        container(
            column![
                text(title)
                    .size(theme::TEXT_HEADING)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1),
                container(
                    scrollable(
                        container(body)
                            .width(Fill)
                            .padding(Padding {
                                top: 0.0,
                                right: 14.0,
                                bottom: 0.0,
                                left: 0.0,
                            }),
                    )
                    .height(Length::Shrink)
                    .style(theme::scrollable),
                )
                .max_height((window.height - 220.0).max(220.0)),
                row![
                    Space::new().width(Fill),
                    button(text(i18n.t("common.cancelar")).size(theme::TEXT_BODY))
                        .on_press(cancel.clone())
                        .padding([9.0, 14.0])
                        .style(theme::ghost_button),
                    button(
                        text(if saving {
                            i18n.t("common.carregando")
                        } else {
                            i18n.t("common.salvar")
                        })
                        .size(theme::TEXT_BODY)
                        .font(theme::FONT_MEDIUM),
                    )
                    .on_press_maybe((!saving).then_some(save))
                    .padding([9.0, 16.0])
                    .style(theme::primary_button),
                ]
                .spacing(theme::SPACE_2)
                .align_y(iced::Center),
            ]
            .spacing(theme::SPACE_4),
        )
        .width(Length::Fixed(620.0))
        .padding(theme::SPACE_5)
        .style(dialog_surface),
    );

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(
            mouse_area(container(Space::new()).width(Fill).height(Fill).style(|_| {
                container::Style {
                    background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.72).into()),
                    ..container::Style::default()
                }
            }))
            .on_press(cancel),
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

fn field<'a>(label: &'a str, control: Element<'a, Message>) -> Element<'a, Message> {
    column![
        text(label)
            .size(theme::TEXT_MICRO)
            .font(theme::FONT_SEMIBOLD)
            .color(theme::TEXT_2),
        control,
    ]
    .spacing(5)
    .into()
}

fn error_banner(message: &str) -> Element<'_, Message> {
    container(
        text(message)
            .size(theme::TEXT_MICRO)
            .color(theme::DANGER)
            .width(Fill),
    )
    .width(Fill)
    .padding([8.0, 10.0])
    .style(theme::error_panel)
    .into()
}

fn dialog_surface(_theme: &iced::Theme) -> container::Style {
    container::Style {
        text_color: Some(theme::TEXT_1),
        background: Some(Color::from_rgb8(0x0f, 0x10, 0x14).into()),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.58),
            offset: Vector::new(0.0, 18.0),
            blur_radius: 42.0,
        },
        ..container::Style::default()
    }
}

fn shorten(value: &str, limit: usize) -> String {
    let mut text: String = value.chars().take(limit).collect();
    if value.chars().count() > limit {
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, launcher: &str) -> Game {
        Game {
            id: id.into(),
            title: "Original".into(),
            launcher: launcher.into(),
            launch_cmd: Vec::new(),
            cover_url: Some("https://img/cover.jpg".into()),
            hero_url: Some("https://img/hero.jpg".into()),
            logo_url: Some("https://img/logo.png".into()),
            description: Some("Descrição base".into()),
            developer: None,
            publisher: None,
            genre: None,
            year: None,
            platform: (launcher == "custom").then(|| "windows".into()),
            exe: (launcher == "custom").then(|| "/games/original.exe".into()),
            playtime_minutes: 0,
            installed: true,
            hidden: false,
            favorite: false,
            categories: Vec::new(),
        }
    }

    #[test]
    fn store_editor_patch_only_has_title_and_description() {
        let mut state = GameEditState::new(&game("steam:440", "steam"));
        state.title = "Novo título".into();
        state.description = String::new();
        let payload = state.payload();
        assert!(payload.custom.is_empty());
        let map = payload.overrides.as_object().unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["title"], "Novo título");
        assert_eq!(map["description"], "");
        assert!(!map.contains_key("installed"));
        assert!(!map.contains_key("launcher"));
        assert!(!map.contains_key("launch_cmd"));
    }

    #[test]
    fn description_reset_is_null_not_empty_string() {
        let mut state = GameEditState::new(&game("steam:440", "steam"));
        state.description_reset = true;
        let payload = state.payload();
        assert!(payload.overrides["description"].is_null());
        state.description_reset = false;
        state.description = String::new();
        assert_eq!(state.overrides_patch()["description"], "");
    }

    #[test]
    fn custom_editor_keeps_id_and_sends_only_changed_fields() {
        let mut state = GameEditState::new(&game("custom:meu-jogo", "custom"));
        state.title = "Novo nome".into();
        state.exe = "/games/novo.exe".into();
        let payload = state.payload();
        assert_eq!(payload.custom.title.as_deref(), Some("Novo nome"));
        assert_eq!(payload.custom.exe.as_deref(), Some("/games/novo.exe"));
        assert!(payload.custom.platform.is_none());
        assert!(!payload.overrides.as_object().unwrap().contains_key("title"));
        assert!(!payload
            .overrides
            .as_object()
            .unwrap()
            .contains_key("installed"));
    }

    #[test]
    fn metadata_uses_parsed_logo_and_keeps_it_when_untouched() {
        let game = game("steam:440", "steam");
        let mut metadata = MetadataState::new(&game);
        assert_eq!(
            metadata.original_logo.as_deref(),
            Some("https://img/logo.png"),
            "logo atual não foi carregada para o editor"
        );
        assert!(!metadata.patch().as_object().unwrap().contains_key("logo"));
        metadata.reset_art(ArtworkKind::Logo);
        assert!(metadata.patch()["logo"].is_null());
    }

    #[test]
    fn metadata_patch_resets_art_with_null_and_keeps_emptied_description() {
        let mut state = MetadataState::new(&game("steam:440", "steam"));
        state.cover = ArtDraft::Reset;
        state.hero = ArtDraft::Path("/tmp/hero.png".into());
        state.description = String::new();
        let patch = state.patch();
        assert!(patch["cover"].is_null());
        assert_eq!(patch["hero"], "/tmp/hero.png");
        assert_eq!(patch["description"], "");
        assert!(!patch.as_object().unwrap().contains_key("logo"));
    }

    #[test]
    fn metadata_description_reset_wins_over_text() {
        let mut state = MetadataState::new(&game("steam:440", "steam"));
        state.description_reset = true;
        state.description = "texto digitado".into();
        assert!(state.patch()["description"].is_null());
    }
}
