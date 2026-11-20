//! arcadia-gamepad — emulador DualSense/DS4 -> Xbox 360 (XInput) via uinput.
//!
//! Porte de `scripts/gamepad-xinput.py`: lê o joystick Sony por evdev
//! (`/dev/input/event*`) e re-emite como um controle Xbox 360 virtual por
//! `/dev/uinput`, para jogos Windows rodando em Proton/Wine enxergarem um
//! XInput de verdade (o DualSense nativo só aparece como js* genérico).
//!
//! Zero dependências — std + libc. O Python original precisava de ctypes para
//! montar as structs do kernel; aqui elas são `[u8; N]` serializados na mão,
//! no layout exato de `linux/uinput.h` (sem packed, alinhamento natural).
//!
//! Uso:
//!     arcadia-gamepad                      # autodetecta o primeiro DualSense/DS4
//!     arcadia-gamepad --device eventXX     # força um /dev/input/eventXX
//!     arcadia-gamepad --list               # lista dispositivos Sony
//!
//! Encerra com SIGINT/SIGTERM; remove o device virtual via UI_DEV_DESTROY.

// uinput e evdev são interfaces do kernel Linux. Compilar em outro alvo não
// falha por falta de libc (a dependência é condicional no Cargo.toml); este
// stub mantém `cargo build --workspace` verde em qualquer host.
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("arcadia-gamepad: disponível apenas no Linux (uinput/evdev).");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
fn main() {
    linux::run();
}
