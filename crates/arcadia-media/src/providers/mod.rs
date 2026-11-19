//! Provedores de metadados/arte.
//!
//! Cada módulo mantém os mesmos endpoints, parâmetros, cabeçalhos e regras de
//! parsing do cliente Electron. As bases são configuráveis para os testes
//! usarem um servidor HTTP local.

pub mod igdb;
pub mod psn;
pub mod steam;
pub mod steamgriddb;
pub mod wallhaven;
pub mod xbox;

use serde::{Deserialize, Serialize};

/// Tipo de arte usado pelos nossos campos `Game` (capa, hero e logo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtKind {
    Cover,
    Hero,
    Logo,
}

impl ArtKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Hero => "hero",
            Self::Logo => "logo",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "cover" => Some(Self::Cover),
            "hero" => Some(Self::Hero),
            "logo" => Some(Self::Logo),
            _ => None,
        }
    }
}

/// Candidato de arte no formato compartilhado pela UI:
/// `{ fonte, url, thumb, largura, altura, animado, autor }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artwork {
    pub fonte: String,
    pub url: String,
    pub thumb: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub largura: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altura: Option<u32>,
    pub animado: bool,
    pub autor: String,
}

impl Artwork {
    pub fn new(fonte: impl Into<String>, url: impl Into<String>) -> Self {
        let url = url.into();
        Self {
            fonte: fonte.into(),
            thumb: url.clone(),
            url,
            largura: None,
            altura: None,
            animado: false,
            autor: String::new(),
        }
    }
}

/// Candidato de descrição: `{ fonte, texto }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextCandidate {
    pub fonte: String,
    pub texto: String,
}

impl TextCandidate {
    pub fn new(fonte: impl Into<String>, texto: impl Into<String>) -> Self {
        Self {
            fonte: fonte.into(),
            texto: texto.into(),
        }
    }
}
