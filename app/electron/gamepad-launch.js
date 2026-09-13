"use strict"

/**
 * gamepad-launch.js — lança o emulador DualSense/DS4 -> XInput (uinput) junto
 * com jogos não-Steam. A cueção: jogos Windows via Proton/Wine/UMU usam a API
 * XInput, que NÃO enxerga o DualSense nativo (hid-playstation, que só expõe
 * /dev/input/js* como joystick genérico). A Steam resolve criando um controle
 * virtual XInput com Steam Input. Este módulo faz o mesmo para jogos lançados
 * cru (non-Steam) sem depender da Steam.
 *
 * Estratégia: se um joystick Sony (DualSense/DS4) está conectado, e o jogo não
 * é lançado via `steam://`, injeta o script Python como um wrapper externo no
 * comando de lançamento. O wrapper cria um device Xbox 360 (/dev/uinput) e o
 * jogo o detecta como um controle XInput.
 *
 * Roda em userspace: /dev/uinput tem tag uaccess, então o usuário da sessão
 * ativa pode escrever sem root. Não exige instalar nada (só stdlib + ctypes).
 */

const fs = require("node:fs")
const path = require("node:path")

// Constants da arquitetura Sony DualSense / DS4. Também reconhecemos qualquer
// joystick que NÃO seja XInput nativo (para jogos Windows via Proton/Wine).
const SONY_VENDOR = 0x054c
const SONY_PRODUCTS = new Set([0x09cc, 0x0df2, 0x0ce6, 0x05c4, 0x05ce])
// Vendor do Xbox (XInput nativo) — NÃO precisa do wrapper: o kernel já o expõe
// como XInput e o Wine o enxerga direto.
const XBOX_VENDOR = 0x045e

// Path do script Python em relação a este módulo (electron/).
const WRAPPER = path.join(__dirname, "scripts", "gamepad-xinput.py")

/**
 * Traduz um caminho dentro do pacote para a cópia extraída.
 *
 * Empacotado, o código vive dentro de `app.asar` — que é um ARQUIVO, não uma
 * pasta. O Electron enxerga através dele, mas o python3 é um processo externo:
 * para ele, `/…/app.asar/electron/scripts/gamepad-xinput.py` não existe. Por
 * isso o electron-builder extrai esse script para `app.asar.unpacked`
 * (ver build.asarUnpack no package.json) e é esse caminho que vai no spawn.
 * Sem isso o emulador funciona só rodando do código-fonte.
 */
function caminhoDesempacotado(caminho) {
  return caminho
    .split(`${path.sep}app.asar${path.sep}`)
    .join(`${path.sep}app.asar.unpacked${path.sep}`)
}

/** Caminho utilizável do wrapper: a cópia extraída no pacote, o próprio no dev. */
function caminhoWrapper() {
  const desempacotado = caminhoDesempacotado(WRAPPER)
  if (desempacotado !== WRAPPER && fs.existsSync(desempacotado)) return desempacotado
  return WRAPPER
}

/**
 * Detecta no sysfs o joystick de um controle que precisa do wrapper XInput.
 * Retorna o /dev/input/eventXX, ou "".
 *
 * Regras:
 *  - Só aceita eventos que são gamepad de verdade (BTN_A=0x130 + ABS_X=ra0).
 *  - Sony (DualSense/DS4) sempre entra — é o caso resolve.
 *  - Qualquer outro joystick NÃO-XInput (não-vendor Xbox) também entra, para o
 *    emulador converter genérico -> XInput. Xbox nativo fica de fora (já é
 *    XInput e o Wine o vê; o wrapper não precisa nem interferir).
 */
function detectGamepadEvent() {
  const base = "/sys/class/input"
  let entries
  try {
    entries = fs.readdirSync(base)
  } catch {
    return ""
  }
  for (const name of entries) {
    if (!name.startsWith("event")) continue
    const ueventPath = path.join(base, name, "device", "uevent")
    let uev
    try {
      uev = fs.readFileSync(ueventPath, "utf8")
    } catch {
      continue
    }
    const parsed = parseUevent(uev)
    if (!parsed) continue
    const [bus, vendor, product] = parsed
    // Só gamepad de verdade (tem BTN_A=0x130 e ABS_X=ra0 no MODALIAS).
    if (!isGamepadUevent(uev)) continue
    const isSony = vendor === SONY_VENDOR && SONY_PRODUCTS.has(product)
    const isXboxNative = vendor === XBOX_VENDOR
    // Xbox nativo (XInput) — o Wine já enxerga, não precisa do wrapper.
    if (isXboxNative) continue
    if (isSony) return `/dev/input/${name}`
    // Outros gamepads não-Sony e não-XInput: qualquer joystick real entra.
    return `/dev/input/${name}`
  }
  return ""
}

/**
 * Extrai (bus, vendor, product) do uevent. Suporta PRODUCT=a/bbbb/cccc/dddd
 * (hex) e MODALIAS=input:bXXXXvYYYYpZZZZ...
 */
function parseUevent(uev) {
  const result = {}
  for (const line of uev.split("\n")) {
    const eq = line.indexOf("=")
    if (eq > 0) result[line.slice(0, eq).trim()] = line.slice(eq + 1).trim()
  }
  // PRODUCT=a/bbbb/cccc/dddd
  const prod = result.PRODUCT || ""
  if (prod) {
    const parts = prod.split("/")
    if (parts.length >= 3) {
      const p0 = parseInt(parts[0], 16)
      const p1 = parseInt(parts[1], 16)
      const p2 = parseInt(parts[2], 16)
      if (!Number.isNaN(p0) && !Number.isNaN(p1) && !Number.isNaN(p2)) {
        return [p0, p1, p2]
      }
    }
  }
  // MODALIAS=input:bXXXXvYYYYpZZZZ... — o produto tem 4 dígitos hex; sem
  // limitar, o grupo guloso engolia o sufixo e devolvia um produto inventado
  // (ex.: 0x0CE6 + "e0110" virava 0xCE6E0110), o que fazia um DualSense deixar
  // de casar com SONY_PRODUCTS quando o uevent viesse sem PRODUCT.
  const modalias = result.MODALIAS || ""
  const m = /b([0-9a-fA-F]+)v([0-9a-fA-F]+)p([0-9a-fA-F]{1,4})/.exec(modalias)
  if (m) return [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)]
  return null
}

/**
 * True se o uevent descreve um gamepad (tem BTN_A=BTN_SOUTH=0x130 e um stick
 * ABS_X=ra0 no MODALIAS). Exclui os event de Motion Sensor e Touchpad que o
 * DualSense também expõe.
 */
function isGamepadUevent(uev) {
  let modalias = ""
  for (const line of uev.split("\n")) {
    if (line.startsWith("MODALIAS=")) modalias = line.slice("MODALIAS=".length).trim()
  }
  if (!modalias) return false
  return modalias.includes("k130") && modalias.includes("ra0")
}

/**
 * Retorna o comando do wrapper para lançar junto do jogo, ou null se não se
 * aplica. `isSteamLaunch` indica se o alvo é a Steam (que já faz Steam Input).
 * O wrapper Python roda como um processo de fundo em paralelo com o jogo —
 * NÃO pode ser o `c[0]` do spawn principal (é um loop infinito).
 */
function buildGamepadWrapperCommand() {
  // Sem joystick Sony conectado, não faz nada.
  const event = detectGamepadEvent()
  if (!event) return null
  const wrapper = caminhoWrapper()
  if (!fs.existsSync(wrapper)) return null
  return [pythonCmd(), wrapper, "--device", event]
}

// Python 3 no PATH (é o intérprete que tem ctypes/libudev disponíveis).
function pythonCmd() {
  return process.env.PYTHON || "python3"
}

// Detection result cached? No — detect on each launch (cheap sysfs scan).

module.exports = {
  detectGamepadEvent,
  parseUevent,
  isGamepadUevent,
  buildGamepadWrapperCommand,
  pythonCmd,
  caminhoWrapper,
  caminhoDesempacotado,
  WRAPPER,
}
