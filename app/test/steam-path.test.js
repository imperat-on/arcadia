"use strict"

// Onde a Steam pode estar.
//
// No Windows, procurar só em "Program Files (x86)\Steam" e "Program Files\Steam"
// deixava de fora a Steam instalada em outro disco (D:\Steam, C:\Games\Steam) ou
// registrada em outro lugar. O sintoma que apareceu: conquistas sincronizavam (vêm do
// servidor) e o playtime não (depende de achar o localconfig.vdf na máquina).

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const sp = require(path.join(__dirname, "..", "electron", "steam-path.js"))

test("candidatos do Windows incluem o registro e os outros discos", () => {
  const fonte = fs.readFileSync(path.join(__dirname, "..", "electron", "steam-path.js"), "utf8")
  const cru = fonte.replace(/\\\\/g, "\\") // normaliza as barras pra comparar o nome da chave
  assert.ok(cru.includes("HKCU\\Software\\Valve\\Steam"), "consulta o registro do usuário")
  assert.ok(cru.includes("WOW6432Node\\Valve\\Steam"), "e a chave de 32 bits")
  assert.ok(fonte.includes("SteamPath") && fonte.includes("InstallPath"), "lê SteamPath e InstallPath")
  assert.match(fonte, /LETRAS_DISCO = "C/, "varre as letras de disco")
  assert.ok(fonte.includes('"Games"'), "inclui pasta fora do Program Files")
  assert.ok(fonte.includes('"SteamLibrary"'), "e a pasta de biblioteca")
  assert.ok(fonte.includes('"steam.exe"'), "reconhece a raiz pelo executável, não só por steamapps")
  assert.ok(fonte.includes("timeout: 3000"), "a consulta ao registro tem timeout (não trava o app)")
  assert.ok(
    /process\.platform !== "win32"\) return \[\]/.test(fonte),
    "a consulta não roda fora do Windows",
  )
})

test("no Linux os candidatos são os de sempre", () => {
  const c = sp.candidatosSteam()
  assert.ok(Array.isArray(c) && c.length > 0)
  const home = os.homedir()
  assert.ok(c.includes(path.join(home, ".steam", "steam")), "Steam nativa (.deb)")
  assert.ok(c.includes(path.join(home, ".local", "share", "Steam")), "Steam do pacote/flatpak")
  assert.ok(
    c.includes(path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam")),
    "Steam Flatpak",
  )
  assert.ok(!c.some((x) => /^[A-Z]:\\/.test(x)), "sem caminhos de Windows nesta plataforma")
  assert.strictEqual(sp.raizesDoRegistro().length, 0, "consulta de registro não roda fora do Windows")
})

test("findSteamDir acha a Steam desta máquina", () => {
  const dir = sp.findSteamDir()
  assert.ok(dir, "devolveu um caminho")
  assert.ok(fs.existsSync(dir), `caminho existe: ${dir}`)
})
