"use strict"

// Catraca do controle: o emulador DualSense/DS4 -> XInput (uinput).
//
// Este é o comportamento que faz jogos Windows via Proton/Wine enxergarem o
// controle: o kernel expõe o DualSense por hid-playstation como joystick
// genérico, e o Wine só procura XInput. O wrapper re-publica um Xbox 360
// virtual. A navegação da interface funciona sem nada disso (Gamepad API), o
// que torna a regressão fácil de passar batido: só se descobre num jogo.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const { execFileSync, spawnSync } = require("node:child_process")

const APP = path.resolve(__dirname, "..")
const ler = (...p) => fs.readFileSync(path.join(APP, ...p), "utf8")

const launcher = require("../electron/gamepad-launch.js")
const cfg = require("../src/components/controllerConfig.ts")

// ---------------------------------------------------------------------------
// 1. Detecção no sysfs
// ---------------------------------------------------------------------------

test("parseUevent lê PRODUCT=bus/vendor/product em hexadecimal", () => {
  // uevent real de um DualSense (0x054c:0x0ce6).
  const uev = "PRODUCT=3/54c/ce6/110\nNAME=\"Sony Interactive Entertainment DualSense Wireless Controller\"\n"
  assert.deepEqual(launcher.parseUevent(uev), [0x3, 0x54c, 0xce6])
})

test("parseUevent lê o MODALIAS quando não há PRODUCT", () => {
  const uev = "MODALIAS=input:b0003v054Cp0CE6e0110-e0,1,4,etc\n"
  assert.deepEqual(launcher.parseUevent(uev), [0x3, 0x54c, 0xce6])
})

test("parseUevent devolve null sem PRODUCT nem MODALIAS aproveitável", () => {
  assert.equal(launcher.parseUevent("PRODUCT=\nMODALIAS=\n"), null)
  assert.equal(launcher.parseUevent(""), null)
})

test("isGamepadUevent aceita o nó de gamepad (BTN_SOUTH + ABS_X)", () => {
  const uev = "MODALIAS=input:b0003v054Cp0CE6e0110-e0,1,2,4,6,80,81,82,83,k130,k131,k13a,ra0,ra1,ra2,ra3\n"
  assert.equal(launcher.isGamepadUevent(uev), true)
})

test("isGamepadUevent recusa os nós de Motion Sensor e Touchpad do DualSense", () => {
  // O DualSense expõe três nós de evento: o gamepad, o sensor de movimento e
  // o touchpad. Só o primeiro tem k130 + ra0; filtrar é o que evita pegar o
  // device errado e o jogo nunca ver o controle.
  const sensor = "MODALIAS=input:b0003v054Cp0CE6e0110-e0,1,2,5,k100,k101,ra0,ra1\n"
  const touchpad = "MODALIAS=input:b0003v054Cp0CE6e0110-e0,1,2,a0,k100,k101,ra0\n"
  assert.equal(launcher.isGamepadUevent(sensor), false)
  assert.equal(launcher.isGamepadUevent(touchpad), false)
  assert.equal(launcher.isGamepadUevent(""), false)
})

// ---------------------------------------------------------------------------
// 2. O wrapper Python
// ---------------------------------------------------------------------------

test("o wrapper aponta para electron/scripts/gamepad-xinput.py e existe", () => {
  assert.equal(launcher.WRAPPER, path.join(APP, "electron", "scripts", "gamepad-xinput.py"))
  assert.ok(fs.existsSync(launcher.WRAPPER), "gamepad-xinput.py precisa acompanhar o app")
})

test("o wrapper compila e falha com mensagem clara num device inválido", (t) => {
  const python = launcher.pythonCmd()
  const temPython = spawnSync(python, ["--version"], { stdio: "ignore" }).status === 0
  if (!temPython) return t.skip(`sem ${python} no PATH`)

  // Compila em memória (sem deixar __pycache__ no repo).
  execFileSync(python, ["-c", "import py_compile,sys; py_compile.compile(sys.argv[1], cfile='/tmp/gamepad-xinput-check.pyc', doraise=True)", launcher.WRAPPER])

  const r = spawnSync(python, [launcher.WRAPPER, "--device", "/dev/input/eventZZZ"], { encoding: "utf8" })
  assert.notEqual(r.status, 0, "device inexistente deve sair com erro")
  assert.match(`${r.stdout}${r.stderr}`, /gamepad|joystick/i)
})

test("o wrapper sai do asar: o python é processo externo e não lê dentro do pacote", () => {
  // O app hoje empacota com asar DESLIGADO, então o script sai como arquivo
  // real e nada disso é necessário. A regra existe para o dia em que o asar for
  // ligado: lá dentro o código é um ARQUIVO, o Electron enxerga através dele e
  // o python3 não — o script tem que ser extraído para app.asar.unpacked.
  const pkg = JSON.parse(ler("package.json"))
  assert.deepEqual(pkg.build.asarUnpack, ["electron/scripts/**"])

  assert.equal(
    launcher.caminhoDesempacotado("/opt/app/resources/app.asar/electron/scripts/gamepad-xinput.py"),
    "/opt/app/resources/app.asar.unpacked/electron/scripts/gamepad-xinput.py",
  )

  const fonte = ler("electron", "gamepad-launch.js")
  assert.match(fonte, /const wrapper = caminhoWrapper\(\)/)
  assert.match(fonte, /return \[pythonCmd\(\), wrapper, "--device", event\]/)
  assert.doesNotMatch(fonte, /return \[pythonCmd\(\), WRAPPER, "--device", event\]/)
})

test("rodando do código-fonte (sem asar) o caminho continua sendo o do repo", () => {
  assert.equal(launcher.caminhoWrapper(), launcher.WRAPPER)
  assert.ok(fs.existsSync(launcher.caminhoWrapper()))
})

test("o wrapper converte os eixos para a faixa do XInput, de forma dinâmica", () => {
  const fonte = fs.readFileSync(launcher.WRAPPER, "utf8")
  // Sticks do DualSense: 0..255 com centro em 128. XInput: -32768..32767 com
  // centro em 0. Assumir 0..255 fixo causava drift em outros controles: o
  // wrapper lê os ranges REAIS do aparelho via EVIOCGABS.
  assert.match(fonte, /EVIOCGABS/)
  assert.match(fonte, /ABS_X/)
  assert.match(fonte, /UI_DEV_DESTROY/)
  assert.match(fonte, /uinput/i)
})

test("o wrapper cruza o diamante só para quem nomeia pela geometria (Sony/Nintendo)", (t) => {
  const python = launcher.pythonCmd()
  if (spawnSync(python, ["--version"], { stdio: "ignore" }).status !== 0) {
    return t.skip(`sem ${python} no PATH`)
  }
  // Importa o wrapper e chama o mapa de verdade (não lê o texto do arquivo).
  const script = [
    "import importlib.util, json, sys",
    "spec = importlib.util.spec_from_file_location('w', sys.argv[1])",
    "m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)",
    "print(json.dumps({str(v): {str(k): str(c) for k, c in m.mapa_botoes(v).items()}",
    "                  for v in (0x054C, 0x057E, 0x045E, 0x1234)}))",
  ].join("\n")
  const r = spawnSync(python, ["-c", script, launcher.WRAPPER], { encoding: "utf8" })
  assert.equal(r.status, 0, r.stderr)
  const mapas = JSON.parse(r.stdout)

  // hid-playstation: Square=BTN_WEST(308), Cross=BTN_SOUTH(304),
  // Circle=BTN_EAST(305), Triangle=BTN_NORTH(307).
  // xpad (Xbox 360): A=BTN_A(304), B=BTN_B(305), X=BTN_X=BTN_NORTH(307),
  // Y=BTN_Y=BTN_WEST(308) — o botão ESQUERDO do Xbox é o X.
  for (const vendor of ["1356", "1406"]) { // 0x054C Sony, 0x057E Nintendo
    const mapa = mapas[vendor]
    assert.equal(mapa["304"], "304", `${vendor}: Cross (baixo) -> A`)
    assert.equal(mapa["305"], "305", `${vendor}: Circle (direita) -> B`)
    assert.equal(mapa["308"], "307", `${vendor}: botão da esquerda -> X`)
    assert.equal(mapa["307"], "308", `${vendor}: botão de cima -> Y`)
  }

  // Vendedor desconhecido: repasse 1:1 (cruzar por conta própria quebraria
  // quem já numera no padrão Xbox).
  for (const vendor of ["1118", "4660"]) { // 0x045E Xbox, 0x1234 genérico
    const mapa = mapas[vendor]
    assert.equal(mapa["308"], "308", `${vendor}: sem cruzamento`)
    assert.equal(mapa["307"], "307", `${vendor}: sem cruzamento`)
  }

  // O resto atravessa inteiro em qualquer caso: ombros, sticks, start/select.
  for (const codigo of ["310", "311", "312", "313", "314", "315", "316", "317", "318"]) {
    assert.equal(mapas["1356"][codigo], codigo, `código ${codigo} deveria passar reto`)
  }
})

test("a lista de produtos Sony inclui o DualSense Edge (0x0df2)", () => {
  // O hid-playstation também cobre o Edge (alias v054Cp0DF2): sem ele, o
  // controle caía no ramo genérico do launcher e era recusado no wrapper.
  const wrapper = fs.readFileSync(launcher.WRAPPER, "utf8")
  assert.match(wrapper, /0x0DF2/)
  assert.match(ler("electron", "gamepad-launch.js"), /0x0df2/)
})

test("o loop re-emite pelo mapa do vendor, não repassa o código cru", () => {
  const fonte = fs.readFileSync(launcher.WRAPPER, "utf8")
  assert.match(fonte, /destino = mapa\.get\(ev\.code\)/)
  assert.match(fonte, /xin\._write_event\(EV_KEY, destino, ev\.value\)/)
  assert.doesNotMatch(fonte, /_write_event\(EV_KEY, code, ev\.value\)/)
})

// ---------------------------------------------------------------------------
// 3. Cache de configuração lido pelos laços de gamepad
// ---------------------------------------------------------------------------

test("o cache nasce com a navegação ligada e a zona morta padrão", () => {
  assert.equal(cfg.DEADZONE_MIN, 0.15)
  assert.equal(cfg.DEADZONE_MAX, 0.85)
  assert.equal(cfg.DEADZONE_PADRAO, 0.6)
  const c = cfg.getControllerConfig()
  assert.equal(c.enable_controller_navigation, true)
  assert.equal(c.controller_deadzone, cfg.DEADZONE_PADRAO)
})

test("clampDeadzone traz valores fora da faixa (e lixo) para o slider", () => {
  assert.equal(cfg.clampDeadzone(0), cfg.DEADZONE_MIN)
  assert.equal(cfg.clampDeadzone(9), cfg.DEADZONE_MAX)
  assert.equal(cfg.clampDeadzone(0.42), 0.42)
  assert.equal(cfg.clampDeadzone(Number.NaN), cfg.DEADZONE_PADRAO)
})

test("setControllerConfig aceita patch parcial e não deixa um config ausente desligar a navegação", () => {
  cfg.setControllerConfig({ controller_deadzone: 0.3 })
  assert.equal(cfg.deadzoneAtual(), 0.3)
  assert.equal(cfg.navegacaoControleAtiva(), true, "mexer na zona morta não desliga a navegação")

  cfg.setControllerConfig({ enable_controller_navigation: false })
  assert.equal(cfg.navegacaoControleAtiva(), false)
  assert.equal(cfg.deadzoneAtual(), 0.3, "desligar não reseta a zona morta")

  // Config ausente/legado (chave `undefined`) não mexe no que já está valendo:
  // é o que garante que um config salvo sem essas chaves não desligue a
  // navegação de quem nunca abriu a aba.
  cfg.setControllerConfig({ enable_controller_navigation: true })
  cfg.setControllerConfig({ enable_controller_navigation: undefined })
  assert.equal(cfg.navegacaoControleAtiva(), true, "chave ausente mantém o padrão ligado")
})

test("o laço de navegação lê a zona morta e o liga/desliga do cache", () => {
  const fonte = ler("src", "components", "ps5-launcher", "useGamepadNav.ts")
  assert.match(fonte, /import \{[^}]*deadzoneAtual[^}]*\} from "\.\.\/controllerConfig"/)
  assert.match(fonte, /const dz = deadzoneAtual\(\)/)
  assert.match(fonte, /navegacaoControleAtiva\(\)/)
  // Nada de limiar cravado no código: era 0.6 fixo.
  assert.doesNotMatch(fonte, /ax > 0\.6 \? 1/)
})

test("o launcher semeia o cache com a configuração salva", () => {
  const fonte = ler("src", "components", "useLibraryState.ts")
  assert.match(fonte, /setControllerConfig\(/)
  assert.match(fonte, /enable_controller_navigation: cfg\.enable_controller_navigation/)
})

// ---------------------------------------------------------------------------
// 4. Costura no processo principal: paralelo, teardown e status
// ---------------------------------------------------------------------------

test("o main lança o wrapper em paralelo, só em jogo não-Steam e fora do Windows", () => {
  const fonte = ler("electron", "main.js")
  assert.match(fonte, /require\("\.\/gamepad-launch"\)/)
  // Paralelo: processo detached, sem I/O compartilhado e desacoplado do ciclo
  // de vida do renderer. Como argv[0] do jogo (loop infinito) o jogo não abriria.
  assert.match(
    fonte,
    /gamepadWrapperChild = spawn\(wrapperCmd\[0\], wrapperCmd\.slice\(1\), \{\s*detached: true,\s*stdio: "ignore",/,
  )
  assert.match(fonte, /gamepadWrapperChild\.unref\?\.\(\)/)
  assert.match(fonte, /if \(!isSteamAlvo && process\.platform !== "win32"\)/)
  // O spawn do jogo continua sendo o do jogo, não o do wrapper.
  assert.match(fonte, /child = spawn\(c\[0\], c\.slice\(1\), \{/)
})

test("o wrapper é derrubado quando a sessão de jogo encerra", () => {
  const fonte = ler("electron", "main.js")
  assert.match(fonte, /function derrubarGamepadWrapper\(\)/)
  assert.match(fonte, /derrubarGamepadWrapper\(\)\n  launchLifecycle\.finish\(record\.token\)/)
  // SIGTERM no grupo: o finally do Python roda UI_DEV_DESTROY e o controle
  // virtual some junto com o jogo.
  assert.match(fonte, /process\.kill\(-child\.pid, "SIGTERM"\)/)
})

test("o status do emulador chega ao renderer pelo IPC", () => {
  const main = ler("electron", "main.js")
  assert.match(main, /ipcMain\.handle\("gamepadStatus"/)
  const preload = ler("electron", "preload.js")
  assert.match(preload, /gamepadStatus: \(\) => ipcRenderer\.invoke\("gamepadStatus"\)/)
  const tipos = ler("src", "global.d.ts")
  assert.match(tipos, /gamepadStatus: \(\) => Promise<\{ ok: boolean; emulator: string; running: boolean \}>/)
})

// ---------------------------------------------------------------------------
// 5. A aba "Controle"
// ---------------------------------------------------------------------------

test("a aba Controle está ligada na sidebar e renderiza o ControllerView", () => {
  const sidebar = ler("src", "components", "desktop", "Sidebar.tsx")
  assert.match(sidebar, /\| "controle"/)
  assert.match(sidebar, /\{ id: "controle", label: "Controle", labelKey: "controller\.titulo"/)
  const settings = ler("src", "components", "desktop", "SettingsView.tsx")
  assert.match(settings, /import \{ ControllerView \} from "\.\/ControllerView"/)
  assert.match(settings, /\{sub === "controle" && <ControllerView \/>\}/)
})

test("a aba Controle não promete emulador no Windows (uinput é Linux-only)", () => {
  // No Windows o pad já é XInput nativo: o wrapper nunca sobe. Dizer "inativo —
  // será ativado ao abrir um jogo" ali seria mentira.
  const fonte = ler("src", "components", "desktop", "ControllerView.tsx")
  assert.match(fonte, /\(window\.launcherPlatform \|\| "linux"\) === "linux"/)
  assert.match(fonte, /setStatusEmulador\("indisponivel"\)/)
})

test("toda chave i18n usada pelas telas de controle existe nos 3 catálogos", () => {
  const catalogos = ["pt-BR", "en-US", "es-ES"].map((n) =>
    JSON.parse(ler("src", "i18n", `${n}.json`)),
  )
  const chaves = new Set()
  for (const arq of [
    ["src", "components", "desktop", "ControllerView.tsx"],
    ["src", "components", "GamepadDiagnostics.tsx"],
  ]) {
    const fonte = ler(...arq)
    for (const m of fonte.matchAll(/t\(\s*[`"]([a-zA-Z0-9_.$}]+)[`"]/g)) {
      const k = m[1]
      if (k.includes("${")) {
        // `controller.tipo_${tipo}` — cobre as quatro famílias.
        for (const sufixo of ["xbox", "playstation", "switch", "generico"]) {
          chaves.add(k.split("${")[0] + sufixo)
        }
      } else {
        chaves.add(k)
      }
    }
  }
  assert.ok(chaves.size >= 10, `esperava várias chaves, achei ${chaves.size}`)
  for (const cat of catalogos) {
    for (const k of chaves) {
      assert.ok(k in cat, `chave ausente: ${k}`)
    }
  }
})
