"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const crypto = require("node:crypto")
const {
  upsertIniSection,
  encryptDuckstationToken,
  configurePcsx2,
  configureDuckstation,
  configurePpsspp,
  configureEmulatorCredentials,
  pcsx2ConfigPaths,
  ppsspIniPath,
  ppsspSecretPath,
} = require("../electron/retroachievements/emulator-config")

function fixtureHome() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-ra-home-"))
}

test("upsertIniSection cria seção nova preservando o resto do arquivo", () => {
  const out = upsertIniSection("[UI]\nfoo = bar\n", "Achievements", { Username: "joao", Token: "abc" })
  assert.match(out, /\[UI\]/)
  assert.match(out, /foo = bar/)
  assert.match(out, /\[Achievements\]/)
  assert.match(out, /Username = joao/)
  assert.match(out, /Token = abc/)
})

test("upsertIniSection atualiza chaves existentes sem tocar em outras seções ou chaves não listadas", () => {
  const existing =
    "[UI]\nfoo=bar\n\n[Achievements]\nEnabled = false\nUsername = old\nSomeOtherKey = keep_me\n\n[Folders]\nBios = /bios\n"
  const out = upsertIniSection(existing, "Achievements", { Enabled: "true", Username: "joao" })
  assert.match(out, /Enabled = true/)
  assert.match(out, /Username = joao/)
  assert.match(out, /SomeOtherKey = keep_me/)
  assert.match(out, /\[Folders\]/)
  assert.match(out, /Bios = \/bios/)
})

test("encryptDuckstationToken é determinístico e reversível com a mesma derivação de chave (modo portátil)", () => {
  const encrypted = encryptDuckstationToken("mytoken123", "joao", { portable: true })
  assert.equal(typeof encrypted, "string")
  assert.ok(encrypted.length > 0)

  // Reproduz a derivação de chave (SHA256(username) + 100 rounds, AES-128-CBC)
  // pra confirmar reversibilidade sem depender de um binário externo.
  let key = crypto.createHash("sha256").update(Buffer.from("joao", "utf8")).digest()
  for (let i = 0; i < 100; i++) key = crypto.createHash("sha256").update(key).digest()
  const decipher = crypto.createDecipheriv("aes-128-cbc", key.subarray(0, 16), key.subarray(16, 32))
  decipher.setAutoPadding(false)
  const decrypted = Buffer.concat([decipher.update(Buffer.from(encrypted, "base64")), decipher.final()])
  assert.equal(decrypted.toString("utf8").replace(/\0+$/, ""), "mytoken123")
})

test("encryptDuckstationToken produz valores diferentes para usuários diferentes", () => {
  const a = encryptDuckstationToken("tok", "userA", { portable: true })
  const b = encryptDuckstationToken("tok", "userB", { portable: true })
  assert.notEqual(a, b)
})

test("configurePcsx2 escreve Username/Token/Enabled em texto puro na seção Achievements", () => {
  const home = fixtureHome()
  try {
    const configFile = pcsx2ConfigPaths(home)[0]
    fs.mkdirSync(path.dirname(configFile), { recursive: true })
    fs.writeFileSync(configFile, "[UI]\nfoo = bar\n")

    const result = configurePcsx2({ username: "joao", token: "tok-abc", home })
    assert.equal(result.ok, true)
    const content = fs.readFileSync(configFile, "utf8")
    assert.match(content, /\[Achievements\]/)
    assert.match(content, /Username = joao/)
    assert.match(content, /Token = tok-abc/)
    assert.match(content, /Enabled = true/)
    // Config anterior fora da seção permanece intacta.
    assert.match(content, /\[UI\]/)
    assert.match(content, /foo = bar/)
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
  }
})

test("configurePcsx2 cria o arquivo mesmo sem instalação prévia detectada", () => {
  const home = fixtureHome()
  try {
    const result = configurePcsx2({ username: "joao", token: "tok-abc", home })
    assert.equal(result.ok, true)
    assert.ok(result.files.length >= 1)
    assert.ok(fs.existsSync(result.files[0]))
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
  }
})

test("configureDuckstation grava o token criptografado (nunca em texto puro)", () => {
  const home = fixtureHome()
  try {
    const result = configureDuckstation({ username: "joao", token: "segredo-claro", home, portable: true })
    assert.equal(result.ok, true)
    const content = fs.readFileSync(result.files[0], "utf8")
    assert.match(content, /\[Cheevos\]/)
    assert.match(content, /Username = joao/)
    assert.doesNotMatch(content, /segredo-claro/)
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
  }
})

test("configurePpsspp separa usuário (ini) do token (secret fora do ini)", () => {
  const home = fixtureHome()
  try {
    const env = { XDG_CONFIG_HOME: path.join(home, ".config") }
    const result = configurePpsspp({ username: "joao", token: "tok-secret", home, env })
    assert.equal(result.ok, true)
    const iniContent = fs.readFileSync(ppsspIniPath(home, env), "utf8")
    assert.match(iniContent, /AchievementsUserName = joao/)
    assert.doesNotMatch(iniContent, /tok-secret/)
    const secretContent = fs.readFileSync(ppsspSecretPath(home, env), "utf8")
    assert.equal(secretContent, "tok-secret")
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
  }
})

test("configureEmulatorCredentials rejeita emuladores sem client RA nativo", () => {
  const home = fixtureHome()
  try {
    const result = configureEmulatorCredentials("rpcs3", { username: "joao", token: "tok", home })
    assert.equal(result.ok, false)
    assert.equal(result.error, "emulador_sem_suporte_retroachievements")
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
  }
})

test("configureEmulatorCredentials rejeita credenciais vazias", () => {
  const result = configureEmulatorCredentials("pcsx2", { username: "", token: "" })
  assert.equal(result.ok, false)
  assert.equal(result.error, "credenciais_vazias")
})
