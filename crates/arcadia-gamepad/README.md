# arcadia-gamepad

Emulador DualSense/DS4 → Xbox 360 (XInput) via `uinput`.

Porte de `scripts/gamepad-xinput.py` (638 linhas de ctypes). O launcher
detecta um controle Sony em `/sys/class/input`, e este binário lê o evdev do
controle e re-emite como um gamepad Xbox 360 virtual em `/dev/uinput` — o que
jogos Windows rodando em Proton/Wine enxergam como XInput de verdade, já que
o DualSense nativo só aparece como `js*` genérico para eles.

## Por que existe

Sem o wrapper, um DualSense no Linux aparece para o Proton como joystick
genérico: sem rumble, sem gatilhos analógicos, e vários jogos não detectam.
O `uinput` expõe um device com o ID de um Xbox 360 (`045e:028e`), que todo
jogo Windows já sabe ler.

## Uso

```sh
arcadia-gamepad                      # autodetecta o primeiro DualSense/DS4
arcadia-gamepad --device event5      # força um /dev/input/eventXX
arcadia-gamepad --list               # lista dispositivos Sony
```

Roda em userspace: `/dev/uinput` tem tag `uaccess`, então o usuário da sessão
ativa escreve sem root. Encerra com SIGINT/SIGTERM e remove o device virtual
via `UI_DEV_DESTROY`.

## A pegadinha do diamante

O kernel nomeia os quatro botões de duas formas. `hid-playstation` (Sony) e
`hid-nintendo` nomeiam pela **geometria**: Square = `BTN_WEST` (0x134,
esquerda), Triangle = `BTN_NORTH` (0x133, topo). O `xpad` (driver do Xbox)
usa os apelidos legados do header, onde `BTN_X` = `BTN_NORTH` e `BTN_Y` =
`BTN_WEST`.

Repassar os códigos 1:1 punha o Square no slot do Y e o Triangle no do X —
a troca de X com Y sentida no jogo. `mapa_botoes` cruza os dois para
vendedores geométricos (Sony 0x054C, Nintendo 0x057E) e mantém 1:1 para os
demais, porque sem saber a convenção do driver cruzar quebraria quem já
numera no padrão Xbox.

Cross e Circle caem certos sozinhos: sul e leste nas duas convenções.

## Escala dos sticks

O DualSense emite `0..255` (centro 127, por `(0+255)/2` com divisão inteira) e
o XInput espera `-32768..32767` (centro 0). `ler_ranges_abs` lê o range real
via `EVIOCGABS` para converter a escala de **qualquer** joystick, não só a do
DualSense.

O centro é 127 e não 128 — comportamento herdado do original, mantido de
propósito: o valor 128 já cai do lado positivo e vira 256.

## Diferenças em relação ao Python

- Zero dependências além de `libc`. As structs do kernel são serializadas na
  mão, no layout de `linux/uinput.h`.
- Timeout de leitura sai do `O_NONBLOCK` + sleep de 2 ms, como no original.
- Sinais tratados por handler em Rust com `AtomicBool`; o Python usava
  `signal` + dicionário mutável.

## Testes

```sh
cargo test -p arcadia-gamepad
```

Cobrem o mapa de botões (1:1, Sony, Nintendo), a escala de sticks (centro,
extremos, range degenerado), o parser de `uevent`/`MODALIAS` e o layout do
buffer de `uinput_abs_setup`.
