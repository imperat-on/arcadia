use std::time::Duration;

use crate::ButtonMap;

/// Como uma direção segurada se repete.
///
/// O primeiro evento sai no quadro do aperto; se o botão continuar pressionado,
/// o próximo sai depois de [`RepeatConfig::initial_delay`] e os seguintes a
/// cada [`RepeatConfig::repeat_interval`]. Os padrões são os mesmos do
/// `useGamepadNav.ts` (500 ms e 260 ms).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepeatConfig {
    /// Liga a repetição. Com `false`, segurar a direção gera só o primeiro
    /// evento.
    pub enabled: bool,
    /// Tempo mínimo com a direção segurada antes do primeiro repeat.
    pub initial_delay: Duration,
    /// Intervalo entre repeats depois do primeiro.
    pub repeat_interval: Duration,
}

impl RepeatConfig {
    /// Configuração ligada com os tempos informados.
    pub const fn new(initial_delay: Duration, repeat_interval: Duration) -> Self {
        Self {
            enabled: true,
            initial_delay,
            repeat_interval,
        }
    }

    /// Repetição desligada. A direção só emite o evento do aperto.
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            initial_delay: Duration::from_millis(500),
            repeat_interval: Duration::from_millis(260),
        }
    }
}

impl Default for RepeatConfig {
    fn default() -> Self {
        Self::new(Duration::from_millis(500), Duration::from_millis(260))
    }
}

/// Configuração completa do leitor de entrada.
///
/// Pode ser trocada a qualquer momento por [`crate::NavInput::config_mut`] ou
/// pelos setters de conveniência ([`crate::NavInput::set_mapping`],
/// [`crate::NavInput::set_repeat`] e [`crate::NavInput::set_deadzone`]).
#[derive(Clone, Debug, PartialEq)]
pub struct InputConfig {
    /// Botões físicos e as ações que eles disparam.
    pub buttons: ButtonMap,
    /// Repetição das direções seguradas.
    pub repeat: RepeatConfig,
    /// Zona morta do analógico esquerdo, de `0.0` a `0.95`.
    ///
    /// O padrão `0.6` é o mesmo `DEADZONE_PADRAO` da UI web. Valores fora da
    /// faixa são limitados internamente; `NaN` volta ao padrão. Um valor
    /// menor que o repouso de um analógico gasto faz o foco andar sozinho.
    pub deadzone: f32,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            buttons: ButtonMap::default(),
            repeat: RepeatConfig::default(),
            deadzone: 0.6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_web_timings() {
        let config = InputConfig::default();
        assert_eq!(config.deadzone, 0.6);
        assert!(config.repeat.enabled);
        assert_eq!(config.repeat.initial_delay, Duration::from_millis(500));
        assert_eq!(config.repeat.repeat_interval, Duration::from_millis(260));
        assert_eq!(config.buttons, ButtonMap::standard());
    }

    #[test]
    fn repeat_can_be_disabled() {
        let repeat = RepeatConfig::disabled();
        assert!(!repeat.enabled);
    }
}
