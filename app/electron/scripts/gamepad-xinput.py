#!/usr/bin/env python3
"""
gamepad-xinput.py — emulador DualSense/DS4 -> Xbox 360 (XInput) via uinput.

Lê o joystick Sony por evdev (/dev/input/event*) e re-emite como um controle
Xbox 360 virtual por /dev/uinput. É o mesmo comportamento do Steam Input x no
sentido de expor um controlador XInput para jogos Windows rodando em
Proton/Wine, que NÃO enxergam DualSense nativo (só /dev/input/js* genérico).

Sem dependências (apenas stdlib + ctypes + libudev/linux/uinput.h do sistema).
Roda em userspace: /dev/uinput tem tag uaccess -> o usuário na sessão ativa
pode escrever sem root. Precisa ler /dev/input/event* (grupo input).

Uso:
    gamepad-xinput.py                     # autodetecta o primeiro DualSense/DS4
    gamepad-xinput.py --device <eventXX>  # força um /dev/input/eventXX
    gamepad-xinput.py --list              # lista dispositivos joystick Sony

Encerra com Ctrl-C / SIGTERM; remove o device virtual via UI_DEV_DESTROY.
"""

import ctypes
import errno
import os
import signal
import struct
import sys
import time

# ----------------------------------------------------------------------------
# Constantes de kernel (linux/input-event-codes.h + linux/uinput.h)
# ----------------------------------------------------------------------------
UINPUT_IOCTL_BASE = ord("U")

# EV_* tipos de evento
EV_SYN = 0x00
EV_KEY = 0x01
EV_ABS = 0x03

# BTN_* (códigos de botão, evdev)
BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST = 0x130, 0x131, 0x133, 0x134
BTN_TL, BTN_TR, BTN_TL2, BTN_TR2 = 0x136, 0x137, 0x138, 0x139
BTN_SELECT, BTN_START, BTN_MODE = 0x13A, 0x13B, 0x13C
BTN_THUMBL, BTN_THUMBR = 0x13D, 0x13E
BTN_DPAD_UP, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT = 0x220, 0x221, 0x222, 0x223

# ABS_* eixos
ABS_X, ABS_Y, ABS_Z = 0x00, 0x01, 0x02
ABS_RX, ABS_RY, ABS_RZ = 0x03, 0x04, 0x05
ABS_HAT0X, ABS_HAT0Y = 0x10, 0x11

# ID de um Xbox 360 (o que o Wine/Proton espera como XInput)
ID_BUS_USB = 0x03
ID_VENDOR_XBOX = 0x045E
ID_PRODUCT_X360 = 0x028E
ID_VERSION = 0x0001

# Bitmaps de capacidades (bits são só capacidade de descrição p/ sysfs)
# (não estritamente necessários p/ escrever, mas o kernel quer saber o que
# vamos usar — declaramos só o que enviamos.)

# Sony DualSense / DS4 (usado para autodetecção via uevent)
SONY_VENDOR = 0x054C
NINTENDO_VENDOR = 0x057E
SONY_PRODUCTS = {
    0x0CE6: "DualSense",
    0x0DF2: "DualSense Edge",
    0x05C4: "DualShock 4 (v1)",
    0x09CC: "DualShock 4 (v2)",
    0x05CE: "DualShock 4 (v2, BT)",
}

# Vendedores cujos drivers nomeiam o diamante pela GEOMETRIA (esquerda = WEST,
# topo = NORTH): Sony (hid-playstation) e Nintendo (hid-nintendo). Ver
# mapa_botoes() para o porquê de isso importar na conversão para XInput.
VENDEDORES_GEOMETRICOS = {SONY_VENDOR, NINTENDO_VENDOR}


# ----------------------------------------------------------------------------
# Structs do kernel
# ----------------------------------------------------------------------------
class input_event(ctypes.Structure):
    _fields_ = [
        ("sec", ctypes.c_long),
        ("usec", ctypes.c_long),
        ("type", ctypes.c_ushort),
        ("code", ctypes.c_ushort),
        ("value", ctypes.c_int),
    ]


class input_id(ctypes.Structure):
    _fields_ = [
        ("bustype", ctypes.c_ushort),
        ("vendor", ctypes.c_ushort),
        ("product", ctypes.c_ushort),
        ("version", ctypes.c_ushort),
    ]


class input_absinfo(ctypes.Structure):
    _fields_ = [
        ("value", ctypes.c_uint),
        ("minimum", ctypes.c_uint),
        ("maximum", ctypes.c_uint),
        ("fuzz", ctypes.c_uint),
        ("flat", ctypes.c_uint),
        ("resolution", ctypes.c_int),
    ]


class uinput_setup(ctypes.Structure):
    _fields_ = [
        ("id", input_id),
        ("name", ctypes.c_char * 80),
        ("ff_effects_max", ctypes.c_uint),
    ]


class uinput_abs_setup(ctypes.Structure):
    _fields_ = [
        ("code", ctypes.c_ushort),
        ("absinfo", input_absinfo),
    ]


def _ioc(dir_, type_, nr, size):
    """linux/ioctl.h _IOC: (dir<<30)|(size<<16)|(type<<8)|nr."""
    return int(
        (dir_ << 30) | (size << 16) | (type_ << 8) | nr
    )


# _IOC_WRITE = 1 ; _IOC_READ = 2
def _IOW(type_, nr, struct_type):
    return _ioc(1, type_, nr, ctypes.sizeof(struct_type))


def _IOR(type_, nr, struct_type):
    return _ioc(2, type_, nr, ctypes.sizeof(struct_type))


def _IO(type_, nr):
    """_IO(type_, nr) — ioctl que não carrega argumento (apenas o nr)."""
    return _ioc(0, type_, nr, 0)


UI_DEV_SETUP = _IOW(UINPUT_IOCTL_BASE, 3, uinput_setup)
UI_DEV_CREATE = _IO(UINPUT_IOCTL_BASE, 1)
UI_DEV_DESTROY = _IO(UINPUT_IOCTL_BASE, 2)
UI_ABS_SETUP = _IOW(UINPUT_IOCTL_BASE, 4, uinput_abs_setup)
UI_SET_EVBIT = _IOW(UINPUT_IOCTL_BASE, 100, ctypes.c_int)
UI_SET_KEYBIT = _IOW(UINPUT_IOCTL_BASE, 101, ctypes.c_int)
UI_SET_RELBIT = _IOW(UINPUT_IOCTL_BASE, 102, ctypes.c_int)
UI_SET_ABSBIT = _IOW(UINPUT_IOCTL_BASE, 103, ctypes.c_int)

# ioctl para ler o ID do dispositivo joystick (EVIOCGID)
EVIOCGID = _ioc(2, ord("E"), 2, ctypes.sizeof(input_id))


def _ler_ranges_abs(fd):
    """Lê min/max de cada eixo ABS do dispositivo joystick via EVIOCGABS.
    Retorna dict {axis: (min, max, valor_atual)}. Permite converter a escala
    real de QUALQUER joystick para o range XInput (-32768..32767)."""
    import fcntl
    ranges = {}
    for axis in range(0x00, 0x11):  # ABS_X(0) .. ABS_HAT1Y(0x11)
        # EVIOCGABS(axis) = _IOR('E', 0x40+axis, struct input_absinfo)
        req = _IOR(ord("E"), 0x40 + axis, input_absinfo)
        buf = bytearray(ctypes.sizeof(input_absinfo))
        try:
            fcntl.ioctl(fd, req, buf, True)
        except OSError:
            continue
        vals = struct.unpack("6i", bytes(buf))
        ranges[axis] = (vals[1], vals[2])  # min, max
    return ranges


def _escala_stick(valor, mn, mx):
    """Converte um valor de stick do range real [mn..mx] (centro (mn+mx)//2)
    para o range XInput [-32768..32767] (centro 0), proporcional."""
    if mx == mn:
        return 0
    # centro do range real
    centro = (mn + mx) // 2
    # normaliza para [-1..1] e multiplica pelo maximo signed
    if valor >= centro:
        span = max(1, mx - centro)
        return int(round(((valor - centro) / span) * 32767))
    span = max(1, centro - mn)
    return int(round(((valor - centro) / span) * 32768))


def styles_dump(ranges):
    """Resume os ranges para log (apenas sticks/triggers)."""
    nomes = {0: "ABS_X", 1: "ABS_Y", 2: "ABS_Z", 3: "ABS_RX", 4: "ABS_RY", 5: "ABS_RZ"}
    return ", ".join(
        f"{nomes.get(a, hex(a))}={mn}..{mx}" for a, (mn, mx) in ranges.items() if mn < mx
    )


def evdev_codes():
    """Códigos que o wrapper lê do DualSense e re-emite como XInput."""
    return {
        BTN_SOUTH: 0x130,  # A  | Cross (baixo)
        BTN_EAST: 0x131,   # B  | Circle (direita)
        BTN_NORTH: 0x133,  # X  | Triangle (topo)
        BTN_WEST: 0x134,   # Y  | Square (esquerda)
        BTN_TL: 0x136,     # LB
        BTN_TR: 0x137,     # RB
        BTN_TL2: 0x138,    # LT (trigger)
        BTN_TR2: 0x139,    # RT (trigger)
        BTN_SELECT: 0x13A,
        BTN_START: 0x13B,
        BTN_MODE: 0x13C,
        BTN_THUMBL: 0x13D,
        BTN_THUMBR: 0x13E,
        BTN_DPAD_UP: 0x220,
        BTN_DPAD_DOWN: 0x221,
        BTN_DPAD_LEFT: 0x222,
        BTN_DPAD_RIGHT: 0x223,
    }


# Mapa de botões: código lido do controle -> código emitido no device XInput.
#
# A pegadinha do diamante. O kernel nomeia os quatro botões de duas formas.
# hid-playstation (Sony) e hid-nintendo nomeiam pela GEOMETRIA: hid-playstation
# reporta Square = BTN_WEST (0x134, esquerda) e Triangle = BTN_NORTH (0x133,
# topo); no Switch Pro é igual (Y = oeste, X = norte). Já o xpad, driver do
# Xbox 360, usa os apelidos legados do header — no Xbox o botão ESQUERDO é
# BTN_X = BTN_NORTH (0x133) e o de CIMA é BTN_Y = BTN_WEST (0x134)
# (input-event-codes.h define BTN_X = BTN_NORTH e BTN_Y = BTN_WEST). Repassar
# os códigos 1:1 punha o Square no slot do Y e o Triangle no do X: era a troca
# de X com Y sentida no jogo. Cross e Circle caem certos sozinhos (sul/leste
# nas duas convenções).
#
# Para os demais vendedores o repasse fica 1:1: sem saber a convenção do
# driver, cruzar por conta própria quebraria quem já numera no padrão Xbox.
def mapa_botoes(vendor):
    """Mapa de botões do controle -> device XInput, conforme a convenção do vendor."""
    mapa = {code: code for code in evdev_codes()}
    if vendor in VENDEDORES_GEOMETRICOS:
        mapa[BTN_WEST] = BTN_NORTH  # esquerda -> X
        mapa[BTN_NORTH] = BTN_WEST  # topo     -> Y
    return mapa


# Mapa ABS: eixo DualSense (evdev) -> (eixo XInput, estilo de escala).
# O DualSense emite sticks como 0..255 (centro 128) e o device XInput espera
# -32768..32767 (centro 0) — por isso escala. d-pad vem como HAT0X/Y (±1),
# que o loop converte em botões BTN_DPAD_* (XInput espera d-pad como botões).
ABS_MAP = {
    ABS_X: (ABS_X, "stick"),    # esquerdo X  (0..255 -> -32768..32767)
    ABS_Y: (ABS_Y, "stick"),    # esquerdo Y
    ABS_RX: (ABS_RX, "stick"),  # direito X
    ABS_RY: (ABS_RY, "stick"),  # direito Y
    ABS_Z: (ABS_Z, "trigger"),  # trigger esquerdo (0..255, direto)
    ABS_RZ: (ABS_RZ, "trigger"),# trigger direito (0..255, direto)
    ABS_HAT0X: (None, "hatx"),  # d-pad horizontal (±1) -> BTN_DPAD_*
    ABS_HAT0Y: (None, "haty"),  # d-pad vertical (±1) -> BTN_DPAD_*
}

# Tabela de eixos configurados no device virtual (ranges XInput reais).
# Só sticks e triggers; hat é convertido em botões, não em eixo.
DEVICE_ABS = {
    ABS_X: (-32768, 32767),
    ABS_Y: (-32768, 32767),
    ABS_RX: (-32768, 32767),
    ABS_RY: (-32768, 32767),
    ABS_Z: (0, 255),
    ABS_RZ: (0, 255),
}


# ----------------------------------------------------------------------------
# Detecção do dispositivo Sony via /sys
# ----------------------------------------------------------------------------
def _read_uevent(event_dir):
    try:
        with open(os.path.join(event_dir, "device", "uevent"), "r") as f:
            return f.read()
    except OSError:
        return ""


def _parse_uevent(uev):
    """Extrai (bus, vendor, product) do uevent. Suporta PRODUCT= e MODALIAS=."""
    result = {}
    for line in uev.splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            result[k.strip()] = v.strip()
    # PRODUCT=a/bbbb/cccc/dddd  (bus/vendor/product/version, hex)
    prod = result.get("PRODUCT", "")
    if prod:
        parts = prod.split("/")
        if len(parts) >= 3:
            try:
                return (
                    int(parts[0], 16),
                    int(parts[1], 16),
                    int(parts[2], 16),
                )
            except ValueError:
                pass
    # MODALIAS=input:bXXXXvYYYYpZZZZ...
    modalias = result.get("MODALIAS", "")
    if modalias:
        import re
        m = re.search(r"b([0-9a-fA-F]+)v([0-9a-fA-F]+)p([0-9a-fA-F]+)", modalias)
        if m:
            return (int(m.group(1), 16), int(m.group(2), 16), int(m.group(3), 16))
    return None


def _is_gamepad_uevent(uev):
    """True se o uevent descreve um gamepad (tem botões 0x130+ e sticks 0x00/0x01)."""
    # Botão BTN_SOUTH (0x130) deve estar presente no bitmap KEY. No MODALIAS,
    # aparece como k130. Também pedimos ABS_X/ABS_Y (ra0,1) no MODALIAS.
    import re
    modalias = ""
    for line in uev.splitlines():
        if line.startswith("MODALIAS="):
            modalias = line.split("=", 1)[1].strip()
    if not modalias:
        return False
    # Tem que ter pelo menos BTN_A(=BTN_SOUTH)=k130 e um stick (ABS_X=ra0).
    has_btn = "k130" in modalias
    has_stick = "ra0" in modalias
    return has_btn and has_stick


def detect_sony_devices():
    """Retorna lista de /dev/input/eventXX que são joysticks Sony."""
    found = []
    base = "/sys/class/input"
    try:
        entries = os.listdir(base)
    except OSError:
        return found
    for name in entries:
        if not name.startswith("event"):
            continue
        real = os.path.realpath(os.path.join(base, name))
        uev = _read_uevent(real)
        parsed = _parse_uevent(uev)
        if not parsed:
            continue
        bus, vendor, product = parsed
        if vendor == SONY_VENDOR and product in SONY_PRODUCTS and _is_gamepad_uevent(uev):
            found.append({
                "dev": f"/dev/input/{name}",
                "bus": bus,
                "vendor": vendor,
                "product": product,
                "name": SONY_PRODUCTS.get(product, "Sony joystick"),
            })
    return found


# ----------------------------------------------------------------------------
# Configuração e criação do device virtual
# ----------------------------------------------------------------------------
class XInputVirtual:
    def __init__(self):
        self.fd = None
        self.abs_enabled = set()

    def open(self):
        self.fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
        # EV_KEY e EV_ABS
        fcntl_ioctl(self.fd, UI_SET_EVBIT, EV_KEY)
        fcntl_ioctl(self.fd, UI_SET_EVBIT, EV_ABS)
        # Botões
        for _code in evdev_codes():
            fcntl_ioctl(self.fd, UI_SET_KEYBIT, _code)
        # Eixos: declara UI_SET_ABSBIT e os ranges via UI_ABS_SETUP ANTES do
        # UI_DEV_SETUP — a ordem do kernel que o /dev/uinput espera.
        for src, (mn, mx) in DEVICE_ABS.items():
            fcntl_ioctl(self.fd, UI_SET_ABSBIT, src)
            self.abs_enabled.add(src)
            a = uinput_abs_setup()
            a.code = src
            a.absinfo.minimum = mn
            a.absinfo.maximum = mx
            a.absinfo.fuzz = 0
            a.absinfo.flat = 0
            fcntl_ioctl(self.fd, UI_ABS_SETUP, a)
        # Setup do device
        setup = uinput_setup()
        setup.id.bustype = ID_BUS_USB
        setup.id.vendor = ID_VENDOR_XBOX
        setup.id.product = ID_PRODUCT_X360
        setup.id.version = ID_VERSION
        setup.name = b"Arcadia Virtual Gamepad (XInput)"
        setup.ff_effects_max = 16
        fcntl_ioctl(self.fd, UI_DEV_SETUP, setup)
        # Cria o device
        fcntl_ioctl(self.fd, UI_DEV_CREATE, 0)
        return self.fd

    def write(self, ev):
        self._write_event(ev["type"], ev["code"], ev["value"])

    def writable(self):
        return self.fd is not None

    def _write_event(self, type_, code, value):
        if self.fd is None:
            return
        ev = input_event()
        ev.type = type_
        ev.code = code
        # valor limitado com deadzone/filtro? mantém cru
        ev.value = int(value)
        buf = ctypes.string_at(ctypes.byref(ev), ctypes.sizeof(ev))
        try:
            os.write(self.fd, buf)
        except OSError as e:
            if e.errno not in (errno.EAGAIN, errno.EWOULDBLOCK):
                raise

    def sync(self):
        if self.fd is None:
            return
        ev = input_event()
        ev.type = EV_SYN
        ev.code = 0
        ev.value = 0
        buf = ctypes.string_at(ctypes.byref(ev), ctypes.sizeof(ev))
        try:
            os.write(self.fd, buf)
        except OSError as e:
            if e.errno not in (errno.EAGAIN, errno.EWOULDBLOCK):
                raise

    def close(self):
        if self.fd is not None:
            try:
                fcntl_ioctl(self.fd, UI_DEV_DESTROY, 0)
            except OSError:
                pass
            try:
                os.close(self.fd)
            except OSError:
                pass
            self.fd = None


def fcntl_ioctl(fd, request, arg):
    """Envolve fcntl.ioctl (Python 3.14 removeu os.ioctl).

    Aceita int (passa direto), ou struct ctypes (serializa para bytes, uso IOW),
    ou bytearray (mutável, para ioctls de leitura).
    """
    import fcntl
    if isinstance(arg, int):
        return fcntl.ioctl(fd, request, arg)
    if isinstance(arg, bytearray):
        return fcntl.ioctl(fd, request, arg, True)
    # struct ctypes -> bytes
    buf = ctypes.string_at(ctypes.byref(arg), ctypes.sizeof(arg))
    return fcntl.ioctl(fd, request, buf)


# ----------------------------------------------------------------------------
# Loop principal
# ----------------------------------------------------------------------------
def _resolver_device(target):
    """Resolve /dev/input/eventXX para um dict de dispositivo.

    Primeiro procura entre os Sony conhecidos. Se não for, aceita como gamepad
    genérico quando o uevent confirma botões (k130) e stick (ra0) — é o caso de
    um controle que o launcher escolheu e mandou por --device.
    """
    alvo = target if target.startswith("/dev/") else f"/dev/input/{target}"
    for d in detect_sony_devices():
        if d["dev"] == alvo:
            return d
    uev = _read_uevent(f"/sys/class/input/{os.path.basename(alvo)}")
    if not _is_gamepad_uevent(uev):
        return None
    bus, vendor, product = _parse_uevent(uev) or (0, 0, 0)
    print(
        f"{alvo} não é Sony; seguindo como gamepad genérico "
        f"(0x{vendor:04x}:0x{product:04x})",
        file=sys.stderr,
    )
    return {
        "dev": alvo,
        "bus": bus,
        "vendor": vendor,
        "product": product,
        "name": f"gamepad 0x{vendor:04x}:0x{product:04x}",
    }


def main():
    import argparse
    parser = argparse.ArgumentParser(description="DualSense/DS4 -> XInput via uinput")
    parser.add_argument("--list", action="store_true", help="lista dispositivos Sony")
    parser.add_argument("--device", help="força /dev/input/eventXX específico")
    args = parser.parse_args()

    if args.list:
        for d in detect_sony_devices():
            print(f"{d['dev']}  {d['name']}  (0x{d['vendor']:04x}:0x{d['product']:04x})"
                  f"  bus=0x{d['bus']:04x}")
        if not detect_sony_devices():
            print("Nenhum joystick Sony (DualSense/DS4) encontrado.")
        return 0

    if args.device:
        dev = _resolver_device(args.device)
        if dev is None:
            print(f"Dispositivo {args.device} não é um gamepad.", file=sys.stderr)
            return 1
    else:
        devices = detect_sony_devices()
        if not devices:
            print("Nenhum DualSense/DS4 detectado. Conecte por USB ou Bluetooth.", file=sys.stderr)
            return 1
        dev = devices[0]
    print(f"Lendo {dev['dev']} ({dev['name']})", file=sys.stderr)
    # O diamante muda de nome conforme o driver do vendedor: ver mapa_botoes().
    mapa = mapa_botoes(dev.get("vendor"))

    # Abre o joystick evdev em modo de leitura
    try:
        joy_fd = os.open(dev["dev"], os.O_RDONLY | os.O_NONBLOCK)
    except OSError as e:
        print(f"Não foi possível abrir {dev['dev']}: {e}", file=sys.stderr)
        return 1

    # Ranges reais dos eixos (permite converter qualquer escala de stick).
    ranges_abs = _ler_ranges_abs(joy_fd)
    print(f"Ranges de sticks detectados: {styles_dump(ranges_abs)}", file=sys.stderr)

    xin = XInputVirtual()
    try:
        xin.open()
    except (OSError, PermissionError) as e:
        print(f"Não foi possível criar device uinput: {e}. "
              f"Confirme /dev/uinput com uaccess.", file=sys.stderr)
        os.close(joy_fd)
        return 1
    print("Device virtual Xbox 360 criado (uinput).", file=sys.stderr)

    running = {"stop": False}

    def handler(sig, frame):
        running["stop"] = True

    signal.signal(signal.SIGTERM, handler)
    signal.signal(signal.SIGINT, handler)

    sz = ctypes.sizeof(input_event)
    # Mapa inverso: evdev ABS -> destino (para re-emissão)
    try:
        while not running["stop"]:
            try:
                data = os.read(joy_fd, sz * 16)
            except BlockingIOError:
                time.sleep(0.002)
                continue
            except OSError as e:
                if e.errno in (errno.EAGAIN, errno.EWOULDBLOCK):
                    time.sleep(0.002)
                    continue
                break
            if not data:
                time.sleep(0.002)
                continue
            for i in range(0, len(data) - (len(data) % sz), sz):
                ev = input_event.from_buffer_copy(data[i:i + sz])
                if ev.type == EV_KEY:
                    # Re-emite como botão XInput pelo mapa do vendor (o d-pad
                    # via HAT é tratado em EV_ABS). O mapa cruza o diamante
                    # para quem nomeia pela geometria: ver mapa_botoes().
                    destino = mapa.get(ev.code)
                    if destino is not None:
                        xin._write_event(EV_KEY, destino, ev.value)
                        xin.sync()
                elif ev.type == EV_ABS:
                    code = ev.code
                    if code not in ABS_MAP:
                        continue
                    dst, style = ABS_MAP[code]
                    if style == "stick":
                        # Converte a escala REAL do joystick para XInput.
                        # DualSense usa 0..255; outros podem variar. Usa os
                        # ranges lidos do dispositivo no boot.
                        mn, mx = ranges_abs.get(code, (0, 255))
                        val = _escala_stick(ev.value, mn, mx)
                        val = max(-32768, min(32767, val))
                        xin._write_event(EV_ABS, dst, val)
                        xin.sync()
                    elif style == "trigger":
                        # trigger: 0..255 direto (já é o range XInput)
                        xin._write_event(EV_ABS, dst, ev.value)
                        xin.sync()
                    elif style == "hatx":
                        # d-pad horizontal (±1) -> BTN_DPAD_LEFT/RIGHT
                        if ev.value == -1:
                            xin._write_event(EV_KEY, BTN_DPAD_LEFT, 1); xin.sync()
                            xin._write_event(EV_KEY, BTN_DPAD_RIGHT, 0); xin.sync()
                        elif ev.value == 1:
                            xin._write_event(EV_KEY, BTN_DPAD_RIGHT, 1); xin.sync()
                            xin._write_event(EV_KEY, BTN_DPAD_LEFT, 0); xin.sync()
                        elif ev.value == 0:
                            xin._write_event(EV_KEY, BTN_DPAD_LEFT, 0); xin.sync()
                            xin._write_event(EV_KEY, BTN_DPAD_RIGHT, 0); xin.sync()
                    elif style == "haty":
                        # d-pad vertical (±1) -> BTN_DPAD_UP/DOWN
                        if ev.value == -1:
                            xin._write_event(EV_KEY, BTN_DPAD_UP, 1); xin.sync()
                            xin._write_event(EV_KEY, BTN_DPAD_DOWN, 0); xin.sync()
                        elif ev.value == 1:
                            xin._write_event(EV_KEY, BTN_DPAD_DOWN, 1); xin.sync()
                            xin._write_event(EV_KEY, BTN_DPAD_UP, 0); xin.sync()
                        elif ev.value == 0:
                            xin._write_event(EV_KEY, BTN_DPAD_UP, 0); xin.sync()
                            xin._write_event(EV_KEY, BTN_DPAD_DOWN, 0); xin.sync()
    finally:
        xin.close()
        try:
            os.close(joy_fd)
        except OSError:
            pass
        print("Wrapper encerrado.", file=sys.stderr)

    return 0


if __name__ == "__main__":
    sys.exit(main())
