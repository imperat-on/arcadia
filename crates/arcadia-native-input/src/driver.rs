use std::time::Duration;

use gilrs::{Axis, Button, EventType, Gilrs};

use crate::{ButtonMap, InputConfig, NavEvent, RepeatConfig};

/// Leitor de gamepads com polling não bloqueante.
///
/// [`NavInput::tick`] drena os eventos pendentes do gilrs e devolve só o que a
/// UI precisa naquele quadro. Sem controle conectado — ou quando o gilrs não
/// consegue iniciar, como em um ambiente headless — os eventos são um vetor
/// vazio: nunca há bloqueio nem erro fatal.
///
/// O leitor deve viver na mesma thread do loop de UI; ele não é sincronizado
/// internamente.
///
/// ```no_run
/// use std::time::Duration;
/// use arcadia_native_input::{FocusList, NavEvent, NavInput};
///
/// let mut input = NavInput::new();
/// let mut menu = FocusList::with_edge(5, arcadia_native_input::EdgeBehavior::Wrap);
///
/// loop {
///     for event in input.tick(Duration::from_millis(16)) {
///         match event {
///             NavEvent::Confirm => { /* ativar menu.index() */ }
///             NavEvent::Back => { /* fechar */ }
///             other => { menu.apply(other); }
///         }
///     }
///     break;
/// }
/// ```
pub struct NavInput {
    backend: Option<Gilrs>,
    engine: Engine,
}

impl NavInput {
    /// Cria o leitor e tenta iniciar o backend gilrs.
    ///
    /// Se o backend não puder iniciar, o leitor fica em modo headless e
    /// [`NavInput::has_backend`] devolve `false`. Use
    /// [`NavInput::is_connected`] para saber se há controle agora.
    pub fn new() -> Self {
        Self::with_config(InputConfig::default())
    }

    /// Igual a [`NavInput::new`], com configuração explícita.
    pub fn with_config(config: InputConfig) -> Self {
        match Gilrs::new() {
            Ok(backend) => Self {
                backend: Some(backend),
                engine: Engine::new(config),
            },
            Err(_) => Self {
                backend: None,
                engine: Engine::new(config),
            },
        }
    }

    /// Cria um leitor sem backend gilrs, para testes e ambientes headless.
    ///
    /// Todo [`NavInput::tick`] devolve vetor vazio; a configuração e os
    /// modelos de foco continuam utilizáveis.
    pub fn headless() -> Self {
        Self::headless_with_config(InputConfig::default())
    }

    /// Igual a [`NavInput::headless`], com configuração explícita.
    pub fn headless_with_config(config: InputConfig) -> Self {
        Self {
            backend: None,
            engine: Engine::new(config),
        }
    }

    /// Processa os eventos pendentes e devolve os eventos de navegação do
    /// quadro.
    ///
    /// `dt` é o tempo desde o chamado anterior. Ele alimenta a repetição das
    /// direções; usar um delta fixo (por exemplo 16 ms) também funciona. A
    /// chamada não bloqueia nem dorme.
    pub fn tick(&mut self, dt: Duration) -> Vec<NavEvent> {
        if let Some(backend) = &mut self.backend {
            while let Some(event) = backend.next_event() {
                match event.event {
                    EventType::ButtonPressed(button, _) => self.engine.press(button),
                    EventType::ButtonReleased(button, _) => self.engine.release(button),
                    EventType::AxisChanged(axis, value, _) => self.engine.axis(axis, value),
                    EventType::Disconnected => self.engine.reset(),
                    _ => {}
                }
            }
        }
        self.engine.advance(dt)
    }

    /// Descarta direções seguradas e eventos pendentes.
    ///
    /// Chame quando a janela perder o foco ou um modal assumir a entrada; sem
    /// isso, uma direção que ficou pressionada pode continuar repetindo ao
    /// voltar o foco. Desconexão de controle também limpa o estado
    /// automaticamente.
    pub fn reset(&mut self) {
        self.engine.reset();
    }

    /// `true` se o backend gilrs iniciou (mesmo sem controle conectado).
    pub fn has_backend(&self) -> bool {
        self.backend.is_some()
    }

    /// `true` se há pelo menos um controle conectado.
    pub fn is_connected(&self) -> bool {
        self.backend
            .as_ref()
            .is_some_and(|backend| backend.gamepads().next().is_some())
    }

    /// Nome do primeiro controle conectado, se houver.
    ///
    /// Devolve `String` porque o nome pertence ao `Gamepad` temporário do
    /// gilrs; guardar o valor evita manter o iterator vivo.
    pub fn gamepad_name(&self) -> Option<String> {
        self.backend
            .as_ref()?
            .gamepads()
            .next()
            .map(|(_, gamepad)| gamepad.name().to_owned())
    }

    /// Configuração atual.
    pub fn config(&self) -> &InputConfig {
        self.engine.config()
    }

    /// Configuração mutável (mapeamento, repetição e zona morta).
    pub fn config_mut(&mut self) -> &mut InputConfig {
        self.engine.config_mut()
    }

    /// Troca o mapa de botões.
    pub fn set_mapping(&mut self, mapping: ButtonMap) {
        self.engine.config_mut().buttons = mapping;
    }

    /// Troca a configuração de repetição.
    pub fn set_repeat(&mut self, repeat: RepeatConfig) {
        self.engine.config_mut().repeat = repeat;
    }

    /// Troca a zona morta do analógico esquerdo.
    pub fn set_deadzone(&mut self, deadzone: f32) {
        self.engine.config_mut().deadzone = deadzone;
    }
}

impl Default for NavInput {
    fn default() -> Self {
        Self::new()
    }
}

/// Núcleo independente de hardware: recebe botões/eixos já traduzidos e
/// mantém o estado de direção segurada e repetição.
struct Engine {
    config: InputConfig,
    stick: (f32, f32),
    dpad: Vec<NavEvent>,
    held: Option<NavEvent>,
    held_for: Duration,
    next_repeat: Duration,
    pending: Vec<NavEvent>,
}

impl Engine {
    fn new(config: InputConfig) -> Self {
        let next_repeat = config.repeat.initial_delay;
        Self {
            config,
            stick: (0.0, 0.0),
            dpad: Vec::new(),
            held: None,
            held_for: Duration::ZERO,
            next_repeat,
            pending: Vec::new(),
        }
    }

    fn config(&self) -> &InputConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut InputConfig {
        &mut self.config
    }

    fn press(&mut self, button: Button) {
        match self.config.buttons.action_for(button) {
            Some(action) if action.is_direction() => {
                if !self.dpad.contains(&action) {
                    self.dpad.push(action);
                }
            }
            Some(action) => self.pending.push(action),
            None => {}
        }
    }

    fn release(&mut self, button: Button) {
        if let Some(action) = self.config.buttons.action_for(button) {
            if action.is_direction() {
                self.dpad.retain(|pressed| *pressed != action);
            }
        }
    }

    fn axis(&mut self, axis: Axis, value: f32) {
        let value = if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        match axis {
            Axis::LeftStickX => self.stick.0 = value,
            Axis::LeftStickY => self.stick.1 = value,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.stick = (0.0, 0.0);
        self.dpad.clear();
        self.held = None;
        self.held_for = Duration::ZERO;
        self.next_repeat = self.config.repeat.initial_delay;
        self.pending.clear();
    }

    fn advance(&mut self, dt: Duration) -> Vec<NavEvent> {
        let mut out = std::mem::take(&mut self.pending);
        let desired = self.desired_direction();

        if desired != self.held {
            self.held = desired;
            self.held_for = Duration::ZERO;
            self.next_repeat = self.config.repeat.initial_delay;
            if let Some(direction) = desired {
                out.push(direction);
            }
        } else if let Some(direction) = desired {
            self.held_for += dt;
            if self.config.repeat.enabled && self.held_for >= self.next_repeat {
                out.push(direction);
                // No máximo um repeat por quadro: um quadro longo não deve
                // atravessar vários itens de uma vez.
                self.next_repeat = self.held_for + self.config.repeat.repeat_interval;
            }
        }

        out
    }

    fn desired_direction(&self) -> Option<NavEvent> {
        if let Some(direction) = self.dpad.last().copied() {
            return Some(direction);
        }

        // Histerese: depois que uma direção está ativa, o analógico precisa
        // cair mais para soltar do que precisou para ativar. Sem isso, um
        // analógico gasto pisca entre direção e repouso.
        let deadzone = self.deadzone();
        let threshold = if self.held.is_some() {
            deadzone * 0.75
        } else {
            deadzone
        };

        let (x, y) = self.stick;
        let (ax, ay) = (x.abs(), y.abs());
        if ax <= threshold && ay <= threshold {
            return None;
        }
        // Eixo dominante. Empate fica com o vertical, que é a direção mais
        // comum em listas e grades de capas.
        if ay >= ax {
            Some(if y < 0.0 {
                NavEvent::Up
            } else {
                NavEvent::Down
            })
        } else {
            Some(if x < 0.0 {
                NavEvent::Left
            } else {
                NavEvent::Right
            })
        }
    }

    fn deadzone(&self) -> f32 {
        if self.config.deadzone.is_finite() {
            self.config.deadzone.clamp(0.0, 0.95)
        } else {
            0.6
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_engine() -> Engine {
        Engine::new(InputConfig::default())
    }

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn confirm_fires_once_per_press() {
        let mut engine = new_engine();
        engine.press(Button::South);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Confirm]);
        assert!(engine.advance(ms(1_000)).is_empty());

        engine.release(Button::South);
        assert!(engine.advance(ms(1)).is_empty());

        engine.press(Button::South);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Confirm]);
    }

    #[test]
    fn action_buttons_map_to_their_events() {
        for (button, expected) in [
            (Button::South, NavEvent::Confirm),
            (Button::East, NavEvent::Back),
            (Button::LeftTrigger, NavEvent::PageLeft),
            (Button::RightTrigger, NavEvent::PageRight),
            (Button::Start, NavEvent::Menu),
            (Button::Select, NavEvent::Options),
            (Button::Mode, NavEvent::Quit),
        ] {
            let mut engine = new_engine();
            engine.press(button);
            assert_eq!(engine.advance(Duration::ZERO), vec![expected]);
        }
    }

    #[test]
    fn unknown_button_is_ignored() {
        let mut engine = new_engine();
        engine.press(Button::North);
        engine.press(Button::Unknown);
        assert!(engine.advance(Duration::ZERO).is_empty());
    }

    #[test]
    fn direction_repeats_after_initial_delay() {
        let mut engine = new_engine();
        engine.press(Button::DPadUp);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Up]);

        assert!(engine.advance(ms(499)).is_empty());
        assert_eq!(engine.advance(ms(1)), vec![NavEvent::Up]);
        assert!(engine.advance(ms(259)).is_empty());
        assert_eq!(engine.advance(ms(1)), vec![NavEvent::Up]);
    }

    #[test]
    fn release_stops_repetition() {
        let mut engine = new_engine();
        engine.press(Button::DPadDown);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Down]);
        engine.release(Button::DPadDown);
        assert!(engine.advance(ms(2_000)).is_empty());
    }

    #[test]
    fn direction_change_restarts_the_repeat_timer() {
        let mut engine = new_engine();
        engine.press(Button::DPadUp);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Up]);
        assert!(engine.advance(ms(300)).is_empty());

        engine.release(Button::DPadUp);
        engine.press(Button::DPadRight);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Right]);
        assert!(engine.advance(ms(499)).is_empty());
        assert_eq!(engine.advance(ms(1)), vec![NavEvent::Right]);
    }

    #[test]
    fn repeat_can_be_disabled() {
        let config = InputConfig {
            repeat: RepeatConfig::disabled(),
            ..InputConfig::default()
        };
        let mut engine = Engine::new(config);

        engine.press(Button::DPadLeft);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Left]);
        assert!(engine.advance(ms(10_000)).is_empty());
    }

    #[test]
    fn dpad_takes_precedence_over_the_stick() {
        let mut engine = new_engine();
        engine.axis(Axis::LeftStickX, 0.9);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Right]);

        engine.press(Button::DPadUp);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Up]);

        engine.release(Button::DPadUp);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Right]);

        engine.axis(Axis::LeftStickX, 0.0);
        assert!(engine.advance(Duration::ZERO).is_empty());
    }

    #[test]
    fn stick_uses_deadzone_and_hysteresis() {
        let mut engine = new_engine();
        engine.axis(Axis::LeftStickX, 0.59);
        assert!(engine.advance(Duration::ZERO).is_empty());

        engine.axis(Axis::LeftStickX, 0.61);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Right]);

        // Abaixo da zona morta, mas acima do limiar de soltura: mantém.
        engine.axis(Axis::LeftStickX, 0.5);
        assert!(engine.advance(Duration::ZERO).is_empty());
        assert_eq!(engine.held, Some(NavEvent::Right));

        engine.axis(Axis::LeftStickX, 0.4);
        assert!(engine.advance(Duration::ZERO).is_empty());
        assert_eq!(engine.held, None);
    }

    #[test]
    fn stick_hold_repeats() {
        let mut engine = new_engine();
        engine.axis(Axis::LeftStickY, 0.9);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Down]);
        assert_eq!(engine.advance(ms(500)), vec![NavEvent::Down]);
        assert!(engine.advance(ms(259)).is_empty());
        assert_eq!(engine.advance(ms(1)), vec![NavEvent::Down]);
    }

    #[test]
    fn diagonal_picks_the_dominant_axis() {
        let mut engine = new_engine();
        engine.axis(Axis::LeftStickX, 0.8);
        engine.axis(Axis::LeftStickY, 0.7);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Right]);

        let mut engine_vertical = new_engine();
        engine_vertical.axis(Axis::LeftStickX, 0.7);
        engine_vertical.axis(Axis::LeftStickY, 0.8);
        assert_eq!(
            engine_vertical.advance(Duration::ZERO),
            vec![NavEvent::Down]
        );

        // Empate resolve para o vertical.
        let mut engine_tie = new_engine();
        engine_tie.axis(Axis::LeftStickX, 0.8);
        engine_tie.axis(Axis::LeftStickY, 0.8);
        assert_eq!(engine_tie.advance(Duration::ZERO), vec![NavEvent::Down]);
    }

    #[test]
    fn diagonal_dpad_uses_the_last_pressed_direction() {
        let mut engine = new_engine();
        engine.press(Button::DPadUp);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Up]);

        engine.press(Button::DPadRight);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Right]);

        engine.release(Button::DPadRight);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Up]);
    }

    #[test]
    fn non_finite_axis_values_are_ignored() {
        let mut engine = new_engine();
        engine.axis(Axis::LeftStickX, f32::NAN);
        engine.axis(Axis::LeftStickY, f32::INFINITY);
        assert!(engine.advance(Duration::ZERO).is_empty());
    }

    #[test]
    fn mapping_changes_are_honoured() {
        let mut config = InputConfig::default();
        config.buttons.confirm = Button::East;
        config.buttons.back = Button::West;
        let mut engine = Engine::new(config);

        engine.press(Button::East);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Confirm]);

        engine.press(Button::West);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Back]);

        engine.press(Button::South);
        assert!(engine.advance(Duration::ZERO).is_empty());
    }

    #[test]
    fn deadzone_is_clamped_and_nan_falls_back() {
        let config = InputConfig {
            deadzone: f32::NAN,
            ..InputConfig::default()
        };
        let engine = Engine::new(config);
        assert_eq!(engine.deadzone(), 0.6);

        let config = InputConfig {
            deadzone: 2.0,
            ..InputConfig::default()
        };
        let engine = Engine::new(config);
        assert_eq!(engine.deadzone(), 0.95);
    }

    #[test]
    fn reset_clears_held_state_and_pending_events() {
        let mut engine = new_engine();
        engine.press(Button::DPadRight);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Right]);

        engine.press(Button::South);
        engine.reset();
        assert_eq!(engine.held, None);
        assert!(engine.advance(ms(2_000)).is_empty());

        engine.press(Button::DPadRight);
        assert_eq!(engine.advance(Duration::ZERO), vec![NavEvent::Right]);
    }

    #[test]
    fn headless_reader_reports_no_hardware() {
        let input = NavInput::headless();
        assert!(!input.has_backend());
        assert!(!input.is_connected());
        assert_eq!(input.gamepad_name(), None);
    }
}
