//! Stub de entrada por gamepad.
//!
//! A navegação por controle (e o modo Big Picture) será construída sobre este
//! módulo numa onda futura. Por enquanto ele expõe apenas um estado neutro para
//! que a UI já tenha o ponto de extensão.

/// Texto exibido no card "Controle" da página do jogo.
pub const GAMEPAD_PLACEHOLDER: &str =
    "Navegação por gamepad ainda não ligada. O stub fica pronto para a onda de input.";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GamepadState {
    pub connected: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub confirm: bool,
    pub back: bool,
}

#[derive(Debug, Default)]
pub struct GamepadInput {
    _private: (),
}

impl GamepadInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// Estado neutro; nenhum dispositivo é lido nesta fase.
    pub fn poll(&mut self) -> GamepadState {
        GamepadState::default()
    }
}
