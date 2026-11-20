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

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::process::exit;

// ---------------------------------------------------------------------------
// Constantes de kernel (linux/input-event-codes.h + linux/uinput.h)
// ---------------------------------------------------------------------------

/// `input_event` do kernel: timeval(16) + type(2) + code(2) + value(4) = 24 bytes
/// em LP64. O Python montava via ctypes; aqui é serialização manual.
const INPUT_EVENT_SIZE: usize = 24;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;

const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
const BTN_NORTH: u16 = 0x133;
const BTN_WEST: u16 = 0x134;
const BTN_TL: u16 = 0x136;
const BTN_TR: u16 = 0x137;
const BTN_TL2: u16 = 0x138;
const BTN_TR2: u16 = 0x139;
const BTN_SELECT: u16 = 0x13A;
const BTN_START: u16 = 0x13B;
const BTN_MODE: u16 = 0x13C;
const BTN_THUMBL: u16 = 0x13D;
const BTN_THUMBR: u16 = 0x13E;
const BTN_DPAD_UP: u16 = 0x220;
const BTN_DPAD_DOWN: u16 = 0x221;
const BTN_DPAD_LEFT: u16 = 0x222;
const BTN_DPAD_RIGHT: u16 = 0x223;

const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_Z: u16 = 0x02;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;
const ABS_RZ: u16 = 0x05;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;

// ID de um Xbox 360 — o que o Wine/Proton espera como XInput.
const ID_BUS_USB: u16 = 0x03;
const ID_VENDOR_XBOX: u16 = 0x045E;
const ID_PRODUCT_X360: u16 = 0x028E;
const ID_VERSION: u16 = 0x0001;

const SONY_VENDOR: u32 = 0x054C;
const NINTENDO_VENDOR: u32 = 0x057E;

/// Produtos Sony que o autodetect reconhece.
const SONY_PRODUCTS: &[(u32, &str)] = &[
    (0x0CE6, "DualSense"),
    (0x0DF2, "DualSense Edge"),
    (0x05C4, "DualShock 4 (v1)"),
    (0x09CC, "DualShock 4 (v2)"),
    (0x05CE, "DualShock 4 (v2, BT)"),
];

// ---------------------------------------------------------------------------
// ioctl helpers (kernel ABI: _IOC(dir,type,nr,size))
// ---------------------------------------------------------------------------

const UINPUT_IOCTL_BASE: u8 = b'U';
const EVIOCTL_BASE: u8 = b'E';

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut list = false;
    let mut device: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--list" => list = true,
            "--device" => {
                i += 1;
                device = args.get(i).cloned();
            }
            other => {
                eprintln!("Argumento desconhecido: {other}");
                eprintln!("Uso: arcadia-gamepad [--list] [--device eventXX]");
                exit(2);
            }
        }
        i += 1;
    }

    if list {
        let devs = detect_sony_devices();
        if devs.is_empty() {
            println!("Nenhum joystick Sony (DualSense/DS4) encontrado.");
        }
        for d in &devs {
            println!(
                "{}  {}  (0x{:04x}:0x{:04x})  bus=0x{:04x}",
                d.dev, d.name, d.vendor, d.product, d.bus
            );
        }
        exit(0);
    }

    let dev = match device {
        Some(target) => match resolver_device(&target) {
            Some(d) => d,
            None => {
                eprintln!("Dispositivo {target} não é um gamepad.");
                exit(1);
            }
        },
        None => match detect_sony_devices().into_iter().next() {
            Some(d) => d,
            None => {
                eprintln!("Nenhum DualSense/DS4 detectado. Conecte por USB ou Bluetooth.");
                exit(1);
            }
        },
    };

    eprintln!("Lendo {} ({})", dev.dev, dev.name);
    let mapa = mapa_botoes(dev.vendor);

    let mut joy = match open_event_read(&dev.dev) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Não foi possível abrir {}: {}", dev.dev, e);
            exit(1);
        }
    };

    let ranges = ler_ranges_abs(joy.as_raw_fd());
    eprintln!("Ranges de sticks detectados: {}", styles_dump(&ranges));

    let mut xin = match XInputVirtual::open() {
        Ok(x) => x,
        Err(e) => {
            eprintln!(
                "Não foi possível criar device uinput: {e}. Confirme /dev/uinput com uaccess."
            );
            exit(1);
        }
    };
    eprintln!("Device virtual Xbox 360 criado (uinput).");

    // SIGINT/SIGTERM encerram o loop pelo EINTR de read.
    unsafe {
        libc::signal(libc::SIGTERM, libc::SIG_IGN);
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    install_signal_handlers(&stop);

    let mut buf = vec![0u8; INPUT_EVENT_SIZE * 16];
    while !stop.load(std::sync::atomic::Ordering::SeqCst) {
        let n = match joy.read(&mut buf) {
            Ok(0) => {
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }
            Err(_) => break,
        };
        let usable = n - (n % INPUT_EVENT_SIZE);
        for chunk in buf[..usable].as_chunks::<INPUT_EVENT_SIZE>().0 {
            // input_event LP64: sec(i64) usec(i64) type(u16) code(u16) value(i32)
            let typ = u16::from_ne_bytes([chunk[16], chunk[17]]);
            let code = u16::from_ne_bytes([chunk[18], chunk[19]]);
            let value = i32::from_ne_bytes([chunk[20], chunk[21], chunk[22], chunk[23]]);
            if typ == EV_KEY {
                if let Some(destino) = mapa.get(&code) {
                    xin.write_key(*destino, value);
                }
            } else if typ == EV_ABS {
                match code {
                    ABS_X | ABS_Y | ABS_RX | ABS_RY => {
                        let (mn, mx) = ranges.get(&code).copied().unwrap_or((0, 255));
                        let mut val = escala_stick(value, mn, mx);
                        val = val.clamp(-32768, 32767);
                        xin.write_abs(code, val);
                    }
                    ABS_Z | ABS_RZ => {
                        xin.write_abs(code, value);
                    }
                    ABS_HAT0X => {
                        if value == -1 {
                            xin.write_key(BTN_DPAD_LEFT, 1);
                            xin.write_key(BTN_DPAD_RIGHT, 0);
                        } else if value == 1 {
                            xin.write_key(BTN_DPAD_RIGHT, 1);
                            xin.write_key(BTN_DPAD_LEFT, 0);
                        } else if value == 0 {
                            xin.write_key(BTN_DPAD_LEFT, 0);
                            xin.write_key(BTN_DPAD_RIGHT, 0);
                        }
                    }
                    ABS_HAT0Y => {
                        if value == -1 {
                            xin.write_key(BTN_DPAD_UP, 1);
                            xin.write_key(BTN_DPAD_DOWN, 0);
                        } else if value == 1 {
                            xin.write_key(BTN_DPAD_DOWN, 1);
                            xin.write_key(BTN_DPAD_UP, 0);
                        } else if value == 0 {
                            xin.write_key(BTN_DPAD_UP, 0);
                            xin.write_key(BTN_DPAD_DOWN, 0);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    xin.close();
    eprintln!("Wrapper encerrado.");
    exit(0);
}

// ---------------------------------------------------------------------------
// Dispositivo virtual uinput
// ---------------------------------------------------------------------------

struct XInputVirtual {
    file: File,
}

impl XInputVirtual {
    /// Monta o device virtual com o ID de Xbox 360 e os ranges XInput.
    fn open() -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open("/dev/uinput")?;

        let fd = file.as_raw_fd();
        unsafe { BorrowedFd::borrow_raw(fd) };

        // EV_KEY e EV_ABS
        ioctl_int(
            fd,
            uinput_iow_int(UINPUT_IOCTL_BASE, 100),
            EV_KEY as libc::c_int,
        )?;
        ioctl_int(
            fd,
            uinput_iow_int(UINPUT_IOCTL_BASE, 100),
            EV_ABS as libc::c_int,
        )?;

        // Botões
        for code in evdev_codes() {
            let req = uinput_iow_int(UINPUT_IOCTL_BASE, 101);
            ioctl_int(fd, req, code as libc::c_int)?;
        }

        // Eixos: UI_SET_ABSBIT + UI_ABS_SETUP antes do UI_DEV_SETUP
        for &(axis, mn, mx) in DEVICE_ABS {
            let req = uinput_iow_int(UINPUT_IOCTL_BASE, 103);
            ioctl_int(fd, req, axis as libc::c_int)?;
            let setup = uinput_abs_setup_buf(axis, mn, mx);
            ioctl_buf(fd, uinput_iow(UINPUT_IOCTL_BASE, 4, 12), &setup)?;
        }

        // Setup do device: ID Xbox 360
        let mut setup = [0u8; 108]; // struct uinput_setup: id(8) name(80) ff_effects_max(4) + pad
        setup[0..2].copy_from_slice(&ID_BUS_USB.to_ne_bytes());
        setup[2..4].copy_from_slice(&ID_VENDOR_XBOX.to_ne_bytes());
        setup[4..6].copy_from_slice(&ID_PRODUCT_X360.to_ne_bytes());
        setup[6..8].copy_from_slice(&ID_VERSION.to_ne_bytes());
        let name = b"Arcadia Virtual Gamepad (XInput)";
        setup[8..8 + name.len()].copy_from_slice(name);
        setup[88..92].copy_from_slice(&16u32.to_ne_bytes()); // ff_effects_max
        ioctl_buf(fd, uinput_iow(UINPUT_IOCTL_BASE, 3, setup.len()), &setup)?;

        // UI_DEV_CREATE
        ioctl_int_raw(fd, uinput_io(UINPUT_IOCTL_BASE, 1))?;
        Ok(Self { file })
    }

    fn write_event_raw(&mut self, typ: u16, code: u16, value: i32) {
        let mut ev = [0u8; INPUT_EVENT_SIZE];
        ev[0..8].copy_from_slice(&0i64.to_ne_bytes()); // sec
        ev[8..16].copy_from_slice(&0i64.to_ne_bytes()); // usec
        ev[16..18].copy_from_slice(&typ.to_ne_bytes());
        ev[18..20].copy_from_slice(&code.to_ne_bytes());
        ev[20..24].copy_from_slice(&value.to_ne_bytes());
        // EAGAIN é ignorado como no original (fd não-bloqueante)
        let _ = self.file.write(&ev);
    }

    fn write_key(&mut self, code: u16, value: i32) {
        self.write_event_raw(EV_KEY, code, value);
        self.sync();
    }

    fn write_abs(&mut self, code: u16, value: i32) {
        self.write_event_raw(EV_ABS, code, value);
        self.sync();
    }

    fn sync(&mut self) {
        self.write_event_raw(EV_SYN, 0, 0);
    }

    fn close(&mut self) {
        let fd = self.file.as_raw_fd();
        let req = uinput_io(UINPUT_IOCTL_BASE, 2); // UI_DEV_DESTROY
        unsafe {
            libc::ioctl(fd, req, 0);
        }
        // File fecha no Drop
    }
}

// ---------------------------------------------------------------------------
// Serialização das structs do kernel
// ---------------------------------------------------------------------------

/// `struct uinput_abs_setup { struct input_absinfo absinfo; __u16 code; }`
/// não é o layout real — o kernel espera `code` primeiro (ver uinput.h):
/// `struct uinput_abs_setup { __u16 code; struct input_absinfo absinfo; }`.
fn uinput_abs_setup_buf(code: u16, mn: u32, mx: u32) -> [u8; 28] {
    let mut buf = [0u8; 28];
    buf[0..2].copy_from_slice(&code.to_ne_bytes());
    // input_absinfo: value, minimum, maximum, fuzz, flat, resolution (6x u32)
    buf[8..12].copy_from_slice(&mn.to_ne_bytes());
    buf[12..16].copy_from_slice(&mx.to_ne_bytes());
    // fuzz(16..20), flat(20..24), resolution(24..28) ficam 0
    buf
}

const DEVICE_ABS: &[(u16, u32, u32)] = &[
    (ABS_X, 32768, 32767),
    (ABS_Y, 32768, 32767),
    (ABS_RX, 32768, 32767),
    (ABS_RY, 32768, 32767),
    (ABS_Z, 0, 255),
    (ABS_RZ, 0, 255),
];

/// Códigos de botão lidos do controle e re-emitidos.
const fn evdev_codes() -> [u16; 17] {
    [
        BTN_SOUTH,
        BTN_EAST,
        BTN_NORTH,
        BTN_WEST,
        BTN_TL,
        BTN_TR,
        BTN_TL2,
        BTN_TR2,
        BTN_SELECT,
        BTN_START,
        BTN_MODE,
        BTN_THUMBL,
        BTN_THUMBR,
        BTN_DPAD_UP,
        BTN_DPAD_DOWN,
        BTN_DPAD_LEFT,
        BTN_DPAD_RIGHT,
    ]
}

// ---------------------------------------------------------------------------
// Mapa de botões (a pegadinha do diamante)
// ---------------------------------------------------------------------------

/// Sony (hid-playstation) e Nintendo (hid-nintendo) nomeiam o diamante pela
/// GEOMETRIA: Square = BTN_WEST, Triangle = BTN_NORTH. O xpad (Xbox 360) usa os
/// apelidos legados: BTN_X = BTN_NORTH, BTN_Y = BTN_WEST. Repassar 1:1 punha o
/// Square no slot do Y — a troca de X/Y sentida no jogo. Para vendedores
/// geométricos o mapa cruza; para os demais fica 1:1.
fn mapa_botoes(vendor: u32) -> std::collections::BTreeMap<u16, u16> {
    let mut mapa = std::collections::BTreeMap::new();
    for code in evdev_codes() {
        if code == 0 {
            continue;
        }
        mapa.insert(code, code);
    }
    if vendor == SONY_VENDOR || vendor == NINTENDO_VENDOR {
        mapa.insert(BTN_WEST, BTN_NORTH); // esquerda -> X
        mapa.insert(BTN_NORTH, BTN_WEST); // topo -> Y
    }
    mapa
}

// ---------------------------------------------------------------------------
// Escala de stick
// ---------------------------------------------------------------------------

/// Converte o range real [mn..mx] (centro (mn+mx)/2) para o range XInput
/// [-32768..32767]. O Python usava `//` (floor div centrado em negativo);
/// aqui o centro coincide, então `/2` direto é equivalente para os ranges
/// reais de joystick (mn+mx sempre par em controladores Sony: 0..255).
fn escala_stick(valor: i32, mn: i32, mx: i32) -> i32 {
    if mx == mn {
        return 0;
    }
    let centro = (mn + mx) / 2;
    if valor >= centro {
        let span = std::cmp::max(1, mx - centro);
        ((((valor - centro) as f64) / span as f64) * 32767.0).round() as i32
    } else {
        let span = std::cmp::max(1, centro - mn);
        ((((valor - centro) as f64) / span as f64) * 32768.0).round() as i32
    }
}

fn styles_dump(ranges: &std::collections::BTreeMap<u16, (i32, i32)>) -> String {
    const NOMES: [(u16, &str); 6] = [
        (ABS_X, "ABS_X"),
        (ABS_Y, "ABS_Y"),
        (ABS_Z, "ABS_Z"),
        (ABS_RX, "ABS_RX"),
        (ABS_RY, "ABS_RY"),
        (ABS_RZ, "ABS_RZ"),
    ];
    let partes: Vec<String> = ranges
        .iter()
        .filter(|&(_, &(mn, mx))| mn < mx)
        .map(|(&axis, &(mn, mx))| {
            let nome = NOMES
                .iter()
                .find(|&&(a, _)| a == axis)
                .map(|&(_, n)| n)
                .unwrap_or_else(|| Box::leak(format!("{axis:#x}").into_boxed_str()));
            format!("{nome}={mn}..{mx}")
        })
        .collect();
    partes.join(", ")
}

// ---------------------------------------------------------------------------
// Ranges ABS do joystick (EVIOCGABS)
// ---------------------------------------------------------------------------

/// Lê min/max de cada eixo ABS via EVIOCGABS para converter a escala real de
/// QUALQUER joystick para o range XInput.
fn ler_ranges_abs(fd: RawFd) -> std::collections::BTreeMap<u16, (i32, i32)> {
    let mut ranges = std::collections::BTreeMap::new();
    for axis in 0u16..0x12 {
        // EVIOCGABS(axis) = _IOR('E', 0x40+axis, struct input_absinfo)
        let req = evio_iord(EVIOCTL_BASE, (0x40 + axis) as u8, 24);
        let mut buf = [0u8; 24];
        let r = unsafe { libc::ioctl(fd, req, buf.as_mut_ptr()) };
        if r < 0 {
            continue;
        }
        let mn = i32::from_ne_bytes(buf[4..8].try_into().unwrap());
        let mx = i32::from_ne_bytes(buf[8..12].try_into().unwrap());
        ranges.insert(axis, (mn, mx));
    }
    ranges
}

// ---------------------------------------------------------------------------
// Detecção Sony via /sys
// ---------------------------------------------------------------------------

struct JoyDevice {
    dev: String,
    bus: u32,
    vendor: u32,
    product: u32,
    name: String,
}

fn read_uevent(event: &str) -> String {
    let base = std::path::Path::new("/sys/class/input").join(event);
    let real = fs::read_link(&base).unwrap_or(base);
    // real aponta para ../../devices/...; o uevent fica no diretório device/
    let mut dir = real;
    dir = fs::canonicalize(&dir).unwrap_or(dir);
    let uev = dir.join("device").join("uevent");
    // Se o link não resolveu (canônico falhou), tenta pelo caminho direto.
    if let Ok(text) = fs::read_to_string(&uev) {
        return text;
    }
    // Fallback: caminho bruto do /sys/class/input/<event>/device/uevent
    let direct = format!("/sys/class/input/{event}/device/uevent");
    fs::read_to_string(direct).unwrap_or_default()
}

/// Extrai (bus, vendor, product) do uevent: PRODUCT=a/bbbb/cccc/dddd ou
/// MODALIAS=input:bXXXXvYYYYpZZZZ.
fn parse_uevent(uev: &str) -> Option<(u32, u32, u32)> {
    for line in uev.lines() {
        if let Some(rest) = line.strip_prefix("PRODUCT=") {
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() >= 3 {
                return Some((
                    u32::from_str_radix(parts[0], 16).ok()?,
                    u32::from_str_radix(parts[1], 16).ok()?,
                    u32::from_str_radix(parts[2], 16).ok()?,
                ));
            }
        }
    }
    for line in uev.lines() {
        if let Some(rest) = line.strip_prefix("MODALIAS=input:") {
            // O kernel emite bXXXXvYYYYpZZZZ com EXATAMENTE 4 dígitos hex por
            // campo; o que vem depois (e0110-...) é a versão e a lista de
            // capacidades. Consumir "todos os hexdigits" engolia o e0110 no
            // product — mesma regra do parser de `arcadia-launch`.
            let mut bus = None;
            let mut vendor = None;
            let mut product = None;
            let bytes = rest.as_bytes();
            let mut i = 0;
            while i + 4 < bytes.len() {
                let tag = bytes[i];
                let hex = &rest[i + 1..i + 5];
                if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    i += 1;
                    continue;
                }
                match tag {
                    b'b' => bus = u32::from_str_radix(hex, 16).ok(),
                    b'v' => vendor = u32::from_str_radix(hex, 16).ok(),
                    b'p' => product = u32::from_str_radix(hex, 16).ok(),
                    _ => {}
                }
                i += 1;
            }
            return Some((bus?, vendor?, product?));
        }
    }
    None
}

/// Gamepad se o MODALIAS tem BTN_SOUTH (k130) e ABS_X (ra0) — o mesmo critério
/// do Python original.
fn is_gamepad_uevent(uev: &str) -> bool {
    let modalias = uev
        .lines()
        .find_map(|l| l.strip_prefix("MODALIAS="))
        .unwrap_or("");
    modalias.contains("k130") && modalias.contains("ra0")
}

fn detect_sony_devices() -> Vec<JoyDevice> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/input") else {
        return found;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("event") {
            continue;
        }
        let uev = read_uevent(name);
        let Some((bus, vendor, product)) = parse_uevent(&uev) else {
            continue;
        };
        if vendor == SONY_VENDOR
            && SONY_PRODUCTS.iter().any(|&(p, _)| p == product)
            && is_gamepad_uevent(&uev)
        {
            found.push(JoyDevice {
                dev: format!("/dev/input/{name}"),
                bus,
                vendor,
                product,
                name: SONY_PRODUCTS
                    .iter()
                    .find(|&&(p, _)| p == product)
                    .map(|&(_, n)| n)
                    .unwrap_or("Sony joystick")
                    .to_string(),
            });
        }
    }
    found
}

fn resolver_device(target: &str) -> Option<JoyDevice> {
    let alvo = if target.starts_with("/dev/") {
        target.to_string()
    } else {
        format!("/dev/input/{target}")
    };
    for d in detect_sony_devices() {
        if d.dev == alvo {
            return Some(d);
        }
    }
    let event = std::path::Path::new(&alvo)
        .file_name()?
        .to_str()?
        .to_string();
    let uev = read_uevent(&event);
    if !is_gamepad_uevent(&uev) {
        return None;
    }
    let (bus, vendor, product) = parse_uevent(&uev).unwrap_or((0, 0, 0));
    eprintln!("{alvo} não é Sony; seguindo como gamepad genérico (0x{vendor:04x}:0x{product:04x})");
    Some(JoyDevice {
        dev: alvo,
        bus,
        vendor,
        product,
        name: format!("gamepad 0x{vendor:04x}:0x{product:04x}"),
    })
}

// ---------------------------------------------------------------------------
// ioctl plumbing bruto
// ---------------------------------------------------------------------------

// _IOC(dir,type,nr,size): (dir<<30)|(size<<16)|(type<<8)|nr
const fn uinput_iow(typ: u8, nr: u8, size: usize) -> libc::c_ulong {
    ((1u64 << 30) | ((size as u64) << 16) | ((typ as u64) << 8) | nr as u64) as libc::c_ulong
}
const fn uinput_io(typ: u8, nr: u8) -> libc::c_ulong {
    (((typ as u64) << 8) | nr as u64) as libc::c_ulong
}
const fn uinput_iow_int(typ: u8, nr: u8) -> libc::c_ulong {
    uinput_iow(typ, nr, 4)
}
const fn evio_iord(typ: u8, nr: u8, size: usize) -> libc::c_ulong {
    ((2u64 << 30) | ((size as u64) << 16) | ((typ as u64) << 8) | nr as u64) as libc::c_ulong
}

fn ioctl_int(fd: RawFd, req: libc::c_ulong, arg: libc::c_int) -> std::io::Result<()> {
    let r = unsafe { libc::ioctl(fd, req, arg) };
    if r < 0 {
        Err(std::io::Error::last_os_error())?;
    }
    Ok(())
}

fn ioctl_int_raw(fd: RawFd, req: libc::c_ulong) -> std::io::Result<()> {
    let r = unsafe { libc::ioctl(fd, req, 0) };
    if r < 0 {
        Err(std::io::Error::last_os_error())?;
    }
    Ok(())
}

fn ioctl_buf(fd: RawFd, req: libc::c_ulong, buf: &[u8]) -> std::io::Result<()> {
    let r = unsafe { libc::ioctl(fd, req, buf.as_ptr()) };
    if r < 0 {
        Err(std::io::Error::last_os_error())?;
    }
    Ok(())
}

fn open_event_read(path: &str) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

fn install_signal_handlers(stop: &std::sync::Arc<std::sync::atomic::AtomicBool>) {
    static STOP: std::sync::OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> =
        std::sync::OnceLock::new();
    let _ = STOP.set(stop.clone());
    unsafe extern "C" fn on_signal(_sig: libc::c_int) {
        if let Some(flag) = STOP.get() {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    unsafe {
        // `sighandler_t` é usize; converter o fn item por `as usize` dispara
        // `clippy::fn_to_numeric_cast`. Um transmute explícito diz a intenção
        // e mantém a assinatura que o libc espera.
        let handler: libc::sighandler_t = std::mem::transmute(on_signal as *const ());
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
}

// ---------------------------------------------------------------------------
// Testes unitários — os casos do rust-anything + os limites de escala
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapa_1_para_1_para_vendor_desconhecido() {
        let m = mapa_botoes(0x0000);
        assert_eq!(m.get(&BTN_SOUTH), Some(&BTN_SOUTH));
        assert_eq!(m.get(&BTN_WEST), Some(&BTN_WEST));
        assert_eq!(m.get(&BTN_NORTH), Some(&BTN_NORTH));
    }

    #[test]
    fn mapa_ignora_vendor_nao_numerico() {
        // Python: vendor fora de VENDEDORES_GEOMETRICOS -> 1:1. O porte usa
        // u32; valores absurdos continuam 1:1.
        let m = mapa_botoes(u32::MAX);
        assert_eq!(m.get(&BTN_WEST), Some(&BTN_WEST));
    }

    #[test]
    fn sony_cruza_o_diamante() {
        let m = mapa_botoes(SONY_VENDOR);
        assert_eq!(m.get(&BTN_WEST), Some(&BTN_NORTH)); // Square -> X
        assert_eq!(m.get(&BTN_NORTH), Some(&BTN_WEST)); // Triangle -> Y
                                                        // Cross e Circle caem certos
        assert_eq!(m.get(&BTN_SOUTH), Some(&BTN_SOUTH));
        assert_eq!(m.get(&BTN_EAST), Some(&BTN_EAST));
    }

    #[test]
    fn nintendo_tambem_cruza() {
        let m = mapa_botoes(NINTENDO_VENDOR);
        assert_eq!(m.get(&BTN_WEST), Some(&BTN_NORTH));
    }

    #[test]
    fn escala_centro_128() {
        // DualSense: 0..255. O centro do Python é (0+255)//2 = 127, então o
        // valor 128 já cai do lado positivo e devolve 256 — não 0. É o
        // comportamento do original; o porte o mantém.
        assert_eq!(escala_stick(128, 0, 255), 256);
        assert_eq!(escala_stick(127, 0, 255), 0);
        assert_eq!(escala_stick(255, 0, 255), 32767);
        assert_eq!(escala_stick(0, 0, 255), -32768);
    }

    #[test]
    fn escala_simetrica() {
        // eixo com range simétrico: -32768..32767
        assert_eq!(escala_stick(0, -32768, 32767), 0);
        assert_eq!(escala_stick(32767, -32768, 32767), 32767);
        assert_eq!(escala_stick(-32768, -32768, 32767), -32768);
    }

    #[test]
    fn escala_range_degenerado() {
        assert_eq!(escala_stick(42, 7, 7), 0);
    }

    #[test]
    fn parse_uevent_product() {
        let uev = "PRODUCT=3/54c/ce6/110\nNAME=\"Sony Interactive Entertainment DualSense\"\n";
        assert_eq!(parse_uevent(uev), Some((0x3, 0x54c, 0xce6)));
    }

    #[test]
    fn parse_uevent_modalias() {
        let uev = "MODALIAS=input:b0003v054Cp0CE6e0110-e0,1,2,4,6,k130,ra0,ra1\n";
        assert_eq!(parse_uevent(uev), Some((0x0003, 0x054c, 0x0ce6)));
    }

    #[test]
    fn gamepad_uevent_precisa_de_botao_e_stick() {
        assert!(is_gamepad_uevent(
            "MODALIAS=input:b0003v054Cp0CE6-e0,1,k130,ra0\n"
        ));
        assert!(!is_gamepad_uevent(
            "MODALIAS=input:b0003v054Cp0CE6-e0,1,k130\n"
        ));
        assert!(!is_gamepad_uevent(
            "MODALIAS=input:b0003v054Cp0CE6-e0,1,ra0\n"
        ));
        assert!(!is_gamepad_uevent(""));
    }

    #[test]
    fn styles_dump_sozinho_eixos_validos() {
        let mut r = std::collections::BTreeMap::new();
        r.insert(ABS_X, (0, 255));
        r.insert(ABS_RY, (-32768, 32767));
        r.insert(ABS_HAT0X, (0, 0)); // degenerado: fora
        let s = styles_dump(&r);
        assert_eq!(s, "ABS_X=0..255, ABS_RY=-32768..32767");
    }

    #[test]
    fn buffer_abs_setup_tem_layout_do_kernel() {
        // uinput_abs_setup: code(u16) + pad(6) + input_absinfo(24) = 32? Não:
        // o kernel empacota __u16 code e depois struct input_absinfo.
        // __attribute__((packed)) não existe em uinput.h — o campo code vem
        // primeiro e absinfo segue com padding de alinhamento: offset 4 não 2.
        let buf = uinput_abs_setup_buf(ABS_X, 32768, 32767);
        assert_eq!(u16::from_ne_bytes(buf[0..2].try_into().unwrap()), ABS_X);
        // minimum no offset 4 (após code+pad), maximum no 8
        assert_eq!(u32::from_ne_bytes(buf[8..12].try_into().unwrap()), 32768);
        assert_eq!(u32::from_ne_bytes(buf[12..16].try_into().unwrap()), 32767);
    }
}
