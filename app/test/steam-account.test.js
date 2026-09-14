"use strict"

// Catraca do vínculo conta Steam x conta Arcadia (B1) e do interruptor (B2).
//
// O que precisa continuar verdadeiro:
//   - conta Steam igual à vinculada  -> captura liberada;
//   - conta Steam trocada            -> captura PAUSADA (nada é ingerido);
//   - captura automática desligada   -> pausada, e "Capturar agora" força UMA vez;
//   - sem Steam legível              -> liberada (não é troca de conta, é ausência);
//   - as horas vêm do localconfig da conta vinculada.
//
// Tudo em diretório temporário: `STEAM_DIR` e `ARCADIA_DATA_DIR` são respeitados.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const raiz = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-steam-"))
const steamDir = path.join(raiz, "Steam")
fs.mkdirSync(path.join(steamDir, "config"), { recursive: true })
fs.mkdirSync(path.join(steamDir, "userdata", "39738813", "config"), { recursive: true })

const CONTA_A = "76561198000004541" // 39738813
const CONTA_B = "76561198000010132" // 39750804

function escreverLoginUsers(ativa) {
  fs.writeFileSync(
    path.join(steamDir, "config", "loginusers.vdf"),
    `"users"
{
\t"${CONTA_A}"
\t{
\t\t"PersonaName"\t\t"Kk"
\t\t"MostRecent"\t\t"${ativa === CONTA_A ? 1 : 0}"
\t}
\t"${CONTA_B}"
\t{
\t\t"PersonaName"\t\t"Outra Conta"
\t\t"MostRecent"\t\t"${ativa === CONTA_B ? 1 : 0}"
\t}
}
`,
  )
}

fs.writeFileSync(
  path.join(steamDir, "userdata", "39738813", "config", "localconfig.vdf"),
  `"UserLocalConfigStore"
{
\t"apps"
\t{
\t\t"990080"
\t\t{
\t\t\t"Playtime"\t\t"4907"
\t\t}
\t}
}
`,
)

process.env.STEAM_DIR = steamDir
process.env.ARCADIA_DATA_DIR = path.join(raiz, "dados")
// HOME na pasta temporária: sem isto o módulo ainda acha a Steam de verdade do
// usuário (~/.steam/steam) e o teste deixa de ser isolado.
process.env.HOME = raiz
fs.mkdirSync(process.env.ARCADIA_DATA_DIR, { recursive: true })

const caminhoConta = (nome) => path.join(process.env.ARCADIA_DATA_DIR, "contas", "zes", nome)
const sa = require("../electron/steam-account")

function configCom(autoLigado) {
  fs.writeFileSync(
    path.join(process.env.ARCADIA_DATA_DIR, "config.json"),
    JSON.stringify({ achievements_auto_capture: autoLigado }),
  )
}

test("primeiro uso vincula sozinho à conta Steam logada", () => {
  escreverLoginUsers(CONTA_A)
  configCom(true)
  const s = sa.status(caminhoConta)
  assert.equal(s.contaAtual.persona, "Kk")
  assert.equal(s.vinculo.persona, "Kk", "sem vínculo, amarra na conta atual")
  assert.equal(s.permitido, true)
  assert.ok(fs.existsSync(caminhoConta(sa.VINCULO)), "o vínculo fica na pasta da conta")
})

test("conta da Steam trocada PAUSA a captura", () => {
  escreverLoginUsers(CONTA_B)
  configCom(true)
  const { permitido, status } = sa.capturaPermitida(caminhoConta)
  assert.equal(permitido, false, "trocar de conta não pode capturar")
  assert.equal(status.contaAtual.persona, "Outra Conta")
  assert.equal(status.vinculo.persona, "Kk")
})

test("captura automática desligada pausa, e 'Capturar agora' força uma vez", () => {
  escreverLoginUsers(CONTA_A)
  configCom(false)
  assert.equal(sa.capturaPermitida(caminhoConta).permitido, false, "interruptor manda")

  sa.forcarProximaCaptura()
  assert.equal(sa.capturaPermitida(caminhoConta).permitido, true, "força uma passada")
  assert.equal(sa.capturaPermitida(caminhoConta).permitido, false, "e só uma")
})

test("captura automática vem DESLIGADA por padrão (sem a chave no config)", () => {
  // Sem config.json nenhum: nada é ingerido sozinho — só pelo botão "Capturar agora".
  escreverLoginUsers(CONTA_A)
  fs.rmSync(path.join(process.env.ARCADIA_DATA_DIR, "config.json"), { force: true })
  assert.equal(sa.capturaPermitida(caminhoConta).permitido, false, "default é desligada")

  sa.forcarProximaCaptura()
  assert.equal(sa.capturaPermitida(caminhoConta).permitido, true, "e o botão continua mandando")

  // Só liga quando o usuário liga explicitamente.
  configCom(true)
  assert.equal(sa.capturaPermitida(caminhoConta).permitido, true, "ligada pelo usuário")
})

test("vincular de novo amarra na conta que está logada agora", () => {
  escreverLoginUsers(CONTA_B)
  configCom(true)
  const r = sa.vincularContaAtual(caminhoConta)
  assert.equal(r.ok, true)
  assert.equal(r.vinculo.persona, "Outra Conta")
  assert.equal(sa.capturaPermitida(caminhoConta).permitido, true, "agora bate")
  escreverLoginUsers(CONTA_A)
})

test("horas da Steam vêm da conta vinculada", () => {
  // Amarrado à conta certa: a leitura segue o vínculo, não o acaso do último teste.
  escreverLoginUsers(CONTA_A)
  const v = sa.vincularContaAtual(caminhoConta)
  assert.equal(v.ok, true)
  const { horas, steamid } = sa.lerHoras(caminhoConta)
  assert.equal(horas["990080"], 4907, "Hogwarts Legacy = 4907 min (81h47)")
  assert.equal(steamid, CONTA_A)
})

test("sem loginusers legível a captura não é bloqueada (ausência, não troca)", () => {
  configCom(true) // o teste anterior desligou o automático
  const backup = fs.readFileSync(path.join(steamDir, "config", "loginusers.vdf"))
  fs.rmSync(path.join(steamDir, "config", "loginusers.vdf"))
  const s = sa.status(caminhoConta)
  assert.equal(s.semSteam, true)
  assert.equal(s.permitido, true)
  fs.writeFileSync(path.join(steamDir, "config", "loginusers.vdf"), backup)
})

test.after(() => {
  try {
    fs.rmSync(raiz, { recursive: true, force: true })
  } catch {}
})
