/// Evento de navegação já traduzido do gamepad.
///
/// A UI não vê botões físicos: vê intenções. O mesmo enum serve para eventos
/// vindos do controle e para teclado ou controles virtuais que a UI queira
/// injetar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NavEvent {
    /// Move o foco uma célula para cima.
    Up,
    /// Move o foco uma célula para baixo.
    Down,
    /// Move o foco uma célula para a esquerda.
    Left,
    /// Move o foco uma célula para a direita.
    Right,
    /// Ativa o item focado (A / Cross / botão 0 no padrão web).
    Confirm,
    /// Volta ou fecha o overlay atual (B / Circle / botão 1).
    Back,
    /// Aba ou página anterior (L1 / LB / botão 4).
    PageLeft,
    /// Próxima aba ou página (R1 / RB / botão 5).
    PageRight,
    /// Abre o menu (Start / botão 9).
    Menu,
    /// Abre as opções (Select / botão 8).
    Options,
    /// Encerra a aplicação ou o modo atual (Guide / Mode por padrão).
    Quit,
}

impl NavEvent {
    /// Passo em coordenadas de tela para eventos direcionais.
    ///
    /// `x` cresce para a direita e `y` cresce para baixo, como no modelo de
    /// foco. Eventos que não são direção devolvem `None`.
    pub const fn delta(self) -> Option<(i32, i32)> {
        match self {
            Self::Up => Some((0, -1)),
            Self::Down => Some((0, 1)),
            Self::Left => Some((-1, 0)),
            Self::Right => Some((1, 0)),
            _ => None,
        }
    }

    /// `true` para [`NavEvent::Up`], [`NavEvent::Down`], [`NavEvent::Left`] e
    /// [`NavEvent::Right`].
    pub const fn is_direction(self) -> bool {
        matches!(self, Self::Up | Self::Down | Self::Left | Self::Right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_deltas_follow_screen_coordinates() {
        assert_eq!(NavEvent::Up.delta(), Some((0, -1)));
        assert_eq!(NavEvent::Down.delta(), Some((0, 1)));
        assert_eq!(NavEvent::Left.delta(), Some((-1, 0)));
        assert_eq!(NavEvent::Right.delta(), Some((1, 0)));
    }

    #[test]
    fn actions_have_no_delta() {
        for event in [
            NavEvent::Confirm,
            NavEvent::Back,
            NavEvent::PageLeft,
            NavEvent::PageRight,
            NavEvent::Menu,
            NavEvent::Options,
            NavEvent::Quit,
        ] {
            assert_eq!(event.delta(), None);
            assert!(!event.is_direction());
        }
    }

    #[test]
    fn direction_detection_matches_variants() {
        assert!(NavEvent::Up.is_direction());
        assert!(NavEvent::Down.is_direction());
        assert!(NavEvent::Left.is_direction());
        assert!(NavEvent::Right.is_direction());
    }
}
