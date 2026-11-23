use gilrs::Button;

use crate::NavEvent;

/// Associação entre botões físicos do gilrs e ações de navegação.
///
/// Os campos são públicos para remapeamento direto. O padrão é o mesmo da UI
/// web (`useGamepadNav.ts` / `PS5Launcher.tsx`):
///
/// | Ação | Botão gilrs | Padrão web |
/// |---|---|---|
/// | [`NavEvent::Confirm`] | [`Button::South`] | A / botão 0 |
/// | [`NavEvent::Back`] | [`Button::East`] | B / botão 1 |
/// | [`NavEvent::PageLeft`] | [`Button::LeftTrigger`] | L1 / LB / botão 4 |
/// | [`NavEvent::PageRight`] | [`Button::RightTrigger`] | R1 / RB / botão 5 |
/// | [`NavEvent::Menu`] | [`Button::Start`] | Start / botão 9 |
/// | [`NavEvent::Options`] | [`Button::Select`] | Select / botão 8 |
/// | [`NavEvent::Quit`] | [`Button::Mode`] | Guide / PS |
/// | [`NavEvent::Up`] | [`Button::DPadUp`] | D-pad ↑ |
/// | [`NavEvent::Down`] | [`Button::DPadDown`] | D-pad ↓ |
/// | [`NavEvent::Left`] | [`Button::DPadLeft`] | D-pad ← |
/// | [`NavEvent::Right`] | [`Button::DPadRight`] | D-pad → |
///
/// `Button::LeftTrigger` é o bumper (L1/LB); o gatilho analógico é
/// `Button::LeftTrigger2` e não é mapeado por padrão. O analógico esquerdo é
/// lido de `Axis::LeftStickX/Y` e não passa por este mapa.
///
/// Evite mapear o mesmo botão para duas ações: [`ButtonMap::action_for`]
/// devolve a primeira correspondência, e as direções são verificadas antes das
/// ações. `quit` é opcional para quem não quiser um botão de sair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ButtonMap {
    /// Botão que move o foco para cima.
    pub up: Button,
    /// Botão que move o foco para baixo.
    pub down: Button,
    /// Botão que move o foco para a esquerda.
    pub left: Button,
    /// Botão que move o foco para a direita.
    pub right: Button,
    /// Botão de confirmar/ativar.
    pub confirm: Button,
    /// Botão de voltar/fechar.
    pub back: Button,
    /// Botão de página/aba anterior.
    pub page_left: Button,
    /// Botão de próxima página/aba.
    pub page_right: Button,
    /// Botão de menu.
    pub menu: Button,
    /// Botão de opções.
    pub options: Button,
    /// Botão de sair, se houver.
    pub quit: Option<Button>,
}

impl ButtonMap {
    /// Mapa padrão (DualSense/Xbox e compatíveis).
    pub const fn standard() -> Self {
        Self {
            up: Button::DPadUp,
            down: Button::DPadDown,
            left: Button::DPadLeft,
            right: Button::DPadRight,
            confirm: Button::South,
            back: Button::East,
            page_left: Button::LeftTrigger,
            page_right: Button::RightTrigger,
            menu: Button::Start,
            options: Button::Select,
            quit: Some(Button::Mode),
        }
    }

    /// Ação associada a um botão, ou `None` se ele não estiver mapeado.
    ///
    /// Direções são verificadas primeiro; depois as ações, na ordem em que
    /// aparecem na documentação do tipo.
    pub fn action_for(&self, button: Button) -> Option<NavEvent> {
        if button == self.up {
            return Some(NavEvent::Up);
        }
        if button == self.down {
            return Some(NavEvent::Down);
        }
        if button == self.left {
            return Some(NavEvent::Left);
        }
        if button == self.right {
            return Some(NavEvent::Right);
        }
        if button == self.confirm {
            return Some(NavEvent::Confirm);
        }
        if button == self.back {
            return Some(NavEvent::Back);
        }
        if button == self.page_left {
            return Some(NavEvent::PageLeft);
        }
        if button == self.page_right {
            return Some(NavEvent::PageRight);
        }
        if button == self.menu {
            return Some(NavEvent::Menu);
        }
        if button == self.options {
            return Some(NavEvent::Options);
        }
        if self.quit == Some(button) {
            return Some(NavEvent::Quit);
        }
        None
    }
}

impl Default for ButtonMap {
    fn default() -> Self {
        Self::standard()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_mapping_matches_the_web_navigation() {
        let map = ButtonMap::standard();
        assert_eq!(map.action_for(Button::South), Some(NavEvent::Confirm));
        assert_eq!(map.action_for(Button::East), Some(NavEvent::Back));
        assert_eq!(
            map.action_for(Button::LeftTrigger),
            Some(NavEvent::PageLeft)
        );
        assert_eq!(
            map.action_for(Button::RightTrigger),
            Some(NavEvent::PageRight)
        );
        assert_eq!(map.action_for(Button::Start), Some(NavEvent::Menu));
        assert_eq!(map.action_for(Button::Select), Some(NavEvent::Options));
        assert_eq!(map.action_for(Button::Mode), Some(NavEvent::Quit));
        assert_eq!(map.action_for(Button::DPadUp), Some(NavEvent::Up));
        assert_eq!(map.action_for(Button::DPadDown), Some(NavEvent::Down));
        assert_eq!(map.action_for(Button::DPadLeft), Some(NavEvent::Left));
        assert_eq!(map.action_for(Button::DPadRight), Some(NavEvent::Right));
    }

    #[test]
    fn default_is_the_standard_mapping() {
        assert_eq!(ButtonMap::default(), ButtonMap::standard());
    }

    #[test]
    fn unmapped_buttons_return_none() {
        let map = ButtonMap::standard();
        assert_eq!(map.action_for(Button::North), None);
        assert_eq!(map.action_for(Button::West), None);
        assert_eq!(map.action_for(Button::LeftThumb), None);
        assert_eq!(map.action_for(Button::Unknown), None);
    }

    #[test]
    fn mapping_is_configurable() {
        let map = ButtonMap {
            confirm: Button::East,
            back: Button::West,
            menu: Button::Mode,
            quit: None,
            ..ButtonMap::standard()
        };

        assert_eq!(map.action_for(Button::East), Some(NavEvent::Confirm));
        assert_eq!(map.action_for(Button::West), Some(NavEvent::Back));
        assert_eq!(map.action_for(Button::Mode), Some(NavEvent::Menu));
        assert_eq!(map.action_for(Button::South), None);
        assert_eq!(map.action_for(Button::Start), None);
    }

    #[test]
    fn quit_can_be_disabled() {
        let map = ButtonMap {
            quit: None,
            ..ButtonMap::standard()
        };
        assert_eq!(map.action_for(Button::Mode), None);
    }
}
