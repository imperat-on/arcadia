//! Entrada de gamepad e navegação por foco para o frontend nativo do Arcadia.
//!
//! Este crate é o equivalente nativo de
//! `ui/src/components/ps5-launcher/useGamepadNav.ts`: lê o controle com
//! [`gilrs`], traduz botões e eixos para [`NavEvent`] e oferece modelos de foco
//! prontos ([`FocusGrid`] e [`FocusList`]). Não há iced nem webview — o
//! renderer só consome eventos e o índice de foco, e decide como desenhar.
//!
//! # Uso típico
//!
//! Chame [`NavInput::tick`] uma vez por quadro com o delta de tempo. O método
//! nunca bloqueia: drena o que o gilrs tiver e devolve os eventos daquele
//! quadro (na maioria deles, um vetor vazio).
//!
//! ```no_run
//! use std::time::Duration;
//! use arcadia_native_input::{FocusGrid, NavEvent, NavInput};
//!
//! let mut input = NavInput::new();
//! let mut grid = FocusGrid::new(7, 4);
//!
//! for event in input.tick(Duration::from_millis(16)) {
//!     match event {
//!         NavEvent::Confirm => println!("abrir o jogo {}", grid.index()),
//!         NavEvent::Back => println!("voltar"),
//!         NavEvent::PageLeft => println!("página anterior"),
//!         NavEvent::PageRight => println!("próxima página"),
//!         NavEvent::Menu => println!("menu"),
//!         NavEvent::Options => println!("opções"),
//!         NavEvent::Quit => break,
//!         direction => {
//!             grid.apply(direction);
//!         }
//!     }
//! }
//! ```
//!
//! # Semântica
//!
//! - Direções respondem no quadro do aperto e, se seguradas, repetem depois de
//!   [`RepeatConfig::initial_delay`] a cada [`RepeatConfig::repeat_interval`].
//!   Os padrões (500 ms / 260 ms) são os mesmos do `useGamepadNav.ts`.
//! - Analógico esquerdo usa zona morta de `0.6` por padrão, também igual à UI
//!   web (`DEADZONE_PADRAO`). D-pad tem prioridade sobre o analógico.
//! - Confirmar, voltar, L1/R1, Start, Select e Quit disparam uma vez por aperto;
//!   segurar não repete.
//! - Sem controle conectado, ou sem backend gilrs disponível, tudo devolve
//!   vetor vazio. Testes e CI não precisam de hardware.

#![deny(missing_docs)]

mod config;
mod driver;
mod event;
mod focus;
mod mapping;

pub use config::{InputConfig, RepeatConfig};
pub use driver::NavInput;
pub use event::NavEvent;
pub use focus::{EdgeBehavior, FocusGrid, FocusList};
pub use mapping::ButtonMap;
