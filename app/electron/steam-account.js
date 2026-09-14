"use strict"

// Vínculo entre a conta da Steam e a conta do Arcadia, mais as horas da Steam.
//
// Por que existe (relato do dono): o vigia de conquistas olha ARQUIVOS da máquina
// (saves de emulador, .bin da Steam) e creditava tudo na conta do Arcadia que
// estivesse ativa. A conquista pertence ao arquivo, não a quem está logado — e o
// dono tem duas contas Steam no mesmo PC, então trocar de conta sujava a outra.
//
// Regra: a captura automática só acontece quando a conta Steam que está logada
// agora é a MESMA que foi amarrada a esta conta do Arcadia. Trocou de conta, o
// app PAUSA a captura e diz por quê (nada é ingerido, nada é revogado). O vínculo
// é por conta do Arcadia, no mesmo diretório das outras coisas da conta.
//
// Tudo o que é leitura da Steam aqui é SOMENTE leitura: o app nunca escreve nos
// arquivos da Steam.

const fs = require("fs")
const os = require("os")
const path = require("path")

const { contasDoLoginUsers, contaAtivaDoLoginUsers, horasDoLocalConfig } = require("./vdf")

const VINCULO = "steam_account.json"

// Interruptor "capturar automaticamente" (B2). Lido direto do config do app para o
// vigia e o loader não precisarem de plumbing.
//
// Default DESLIGADO: a captura automática só roda quando o usuário LIGA o
// interruptor, ou numa passada sob demanda pelo botão "Capturar agora"
// (forcar=true). Sem isso, nada é ingerido sem o usuário pedir.
function autoLigado() {
  try {
    const raiz = process.env.ARCADIA_DATA_DIR
      ? process.env.ARCADIA_DATA_DIR
      : path.join(os.homedir(), ".local", "share", "arcadia")
    const cfg = JSON.parse(fs.readFileSync(path.join(raiz, "config.json"), "utf-8"))
    // Só liga com valor explícito: sem a chave (ou com qualquer outra coisa), fica off.
    return cfg.achievements_auto_capture === true
  } catch {
    return false
  }
}

// Mesmas raízes que o resto do app usa para achar a Steam. A lista (registro do
// Windows, discos, Program Files, Flatpak/.deb no Linux) mora em steam-path.js, para
// não existirem duas respostas diferentes para "onde está a Steam".
function raizesSteam() {
  const cands = require("./steam-path").candidatosSteam()
  return cands.filter((p) => fs.existsSync(p))
}

/** A chave das horas é o NÚMERO do appid: o id do jogo pode vir como `steam:990080`. */
function chaveAppid(appid) {
  const s = String(appid == null ? "" : appid).trim()
  const m = /(\d{2,})/.exec(s)
  return m ? m[1] : s
}

function lerArquivo(caminho) {
  try {
    return fs.readFileSync(caminho, "utf-8")
  } catch {
    return ""
  }
}

// Pasta do userdata cujo loginusers diz ser a conta ativa (MostRecent=1).
// O nome da pasta é o id de conta; o loginusers fala em steamid64 — o módulo vdf
// converte quando precisa.
function contaSteamAtiva() {
  for (const raiz of raizesSteam()) {
    const txt = lerArquivo(path.join(raiz, "config", "loginusers.vdf"))
    if (!txt) continue
    const ativa = contaAtivaDoLoginUsers(txt)
    if (ativa) return { ...ativa, raiz }
  }
  // Sem MostRecent (arquivo recém-mexido), cai na primeira conta conhecida.
  for (const raiz of raizesSteam()) {
    const txt = lerArquivo(path.join(raiz, "config", "loginusers.vdf"))
    if (!txt) continue
    const contas = contasDoLoginUsers(txt)
    if (contas.length) return { ...contas[0], raiz, semMostRecent: true }
  }
  return null
}

/** Contas Steam conhecidas nesta máquina (para a tela de configuração). */
function contasSteam() {
  for (const raiz of raizesSteam()) {
    const txt = lerArquivo(path.join(raiz, "config", "loginusers.vdf"))
    if (!txt) continue
    return contasDoLoginUsers(txt).map((c) => ({ steamid: c.steamid, persona: c.persona }))
  }
  return []
}

// --- vínculo por conta do Arcadia -------------------------------------------

function caminhoVinculo(caminhoConta) {
  try {
    return caminhoConta(VINCULO)
  } catch {
    return ""
  }
}

function lerVinculo(caminhoConta) {
  const p = caminhoVinculo(caminhoConta)
  if (!p) return null
  try {
    const d = JSON.parse(fs.readFileSync(p, "utf-8"))
    if (d && typeof d === "object" && d.steamid) return d
  } catch {}
  return null
}

function gravarVinculo(caminhoConta, dados) {
  const p = caminhoVinculo(caminhoConta)
  if (!p) return false
  try {
    const tmp = `${p}.tmp`
    fs.mkdirSync(path.dirname(p), { recursive: true })
    fs.writeFileSync(tmp, JSON.stringify(dados, null, 2))
    fs.renameSync(tmp, p)
    return true
  } catch {
    return false
  }
}

/**
 * Estado da captura para a conta ATIVA do Arcadia.
 * @param {function} caminhoConta helper do app (conta ativa -> caminho)
 * @param {function} log função de log do app
 */
function status(caminhoConta, log = () => {}, opts = {}) {
  const ativa = contaSteamAtiva()
  let vinculo = lerVinculo(caminhoConta)
  const forcar = Boolean(opts.forcar)
  const auto = opts.auto === undefined ? autoLigado() : Boolean(opts.auto)

  // Primeiro uso: sem vínculo ainda, amarra na conta Steam que está logada.
  // É o padrão sensato — quem nunca trocou de conta não sente diferença.
  if (!vinculo && ativa) {
    const novo = { steamid: ativa.steamid, persona: ativa.persona, vinculadoEm: Date.now() }
    if (gravarVinculo(caminhoConta, novo)) {
      vinculo = novo
      log("steam-account/vinculo-inicial", `${ativa.persona} (${ativa.steamid.slice(-6)})`)
    }
  }

  const contaAtual = ativa
    ? { steamid: ativa.steamid, persona: ativa.persona }
    : null
  const bate = Boolean(contaAtual && vinculo && contaAtual.steamid === vinculo.steamid)
  // Sem Steam instalada/legível não há como julgar: deixa capturar (não é uma
  // troca de conta, é ausência de dado).
  const semSteam = !contaAtual

  return {
    vinculo: vinculo ? { steamid: vinculo.steamid, persona: vinculo.persona } : null,
    contaAtual,
    // `forcar` = botão "Capturar agora": ignora a troca de conta e o interruptor.
    permitido: forcar || (auto && (semSteam || bate)),
    vinculoOk: bate,
    semSteam,
    auto,
    contasSteam: contasSteam(),
  }
}

/** Atalho para os pontos de captura: pode ingerir/revogar agora? */
function capturaPermitida(caminhoConta, log, opts) {
  const efetivo = Object.assign({}, opts)
  if (forcarProxima) {
    // "Capturar agora" força UMA passada e some: o próximo ciclo volta à regra.
    efetivo.forcar = true
    forcarProxima = false
  }
  const s = status(caminhoConta, log, efetivo)
  return { permitido: s.permitido, status: s }
}

// Marcado pelo IPC "Capturar agora" (uma passada, em memória).
let forcarProxima = false
function forcarProximaCaptura() {
  forcarProxima = true
}

/** Amarra esta conta do Arcadia à conta Steam que está logada agora. */
function vincularContaAtual(caminhoConta, log = () => {}) {
  const ativa = contaSteamAtiva()
  if (!ativa) return { ok: false, motivo: "sem_steam" }
  const dados = { steamid: ativa.steamid, persona: ativa.persona, vinculadoEm: Date.now() }
  if (!gravarVinculo(caminhoConta, dados)) return { ok: false, motivo: "nao_gravou" }
  log("steam-account/vinculado", `${ativa.persona} (${ativa.steamid.slice(-6)})`)
  return { ok: true, vinculo: { steamid: ativa.steamid, persona: ativa.persona } }
}

// --- horas da Steam ---------------------------------------------------------

function lerHoras(caminhoConta, log = () => {}) {
  const s = status(caminhoConta, log, { forcar: true })
  const alvo = s.vinculo ? s.vinculo.steamid : s.contaAtual ? s.contaAtual.steamid : ""
  if (!alvo) return { horas: {}, persona: "", steamid: "" }
  const contaId = String(BigInt(alvo) - 76561197960265728n)
  for (const raiz of raizesSteam()) {
    const txt = lerArquivo(path.join(raiz, "userdata", contaId, "config", "localconfig.vdf"))
    if (!txt) continue
    const horas = horasDoLocalConfig(txt)
    if (Object.keys(horas).length) {
      return {
        horas,
        persona: s.vinculo ? s.vinculo.persona : s.contaAtual.persona,
        steamid: alvo,
      }
    }
  }
  return { horas: {}, persona: s.vinculo ? s.vinculo.persona : "", steamid: alvo }
}

/** Horas de UMA conta Steam, pelo steamid (não depende de vínculo nenhum). */
function lerHorasDaConta(steamid, persona = "") {
  const alvo = String(steamid || "")
  if (!alvo) return { horas: {}, persona, steamid: "" }
  const contaId = String(BigInt(alvo) - 76561197960265728n)
  for (const raiz of raizesSteam()) {
    const txt = lerArquivo(path.join(raiz, "userdata", contaId, "config", "localconfig.vdf"))
    if (!txt) continue
    const horas = horasDoLocalConfig(txt)
    if (Object.keys(horas).length) return { horas, persona, steamid: alvo }
  }
  return { horas: {}, persona, steamid: alvo }
}

/**
 * Horas de TODAS as contas Steam conhecidas nesta máquina.
 *
 * Existe para uma coisa só: quem troca de conta não pode PERDER as horas da conta
 * anterior. Cada conta tem a sua pasta em `userdata/`, então dá para ler todas — e o
 * servidor guarda o maior valor por (jogo, conta), de modo que o que já subiu de
 * outra máquina também entra na soma.
 */
function lerHorasDeTodasAsContas(log = () => {}) {
  const leituras = []
  for (const c of contasSteam()) {
    const leitura = lerHorasDaConta(c.steamid, c.persona)
    if (Object.keys(leitura.horas).length) leituras.push(leitura)
  }
  if (!leituras.length) {
    // Sem lista de contas (loginusers recém-mexido): cai na conta que está ativa.
    const ativa = contaSteamAtiva()
    if (ativa) {
      const leitura = lerHorasDaConta(ativa.steamid, ativa.persona)
      if (Object.keys(leitura.horas).length) leituras.push(leitura)
    }
  }
  log("steam-account/horas", `${leituras.length} conta(s) com horas locais`)
  return leituras
}

module.exports = {
  raizesSteam,
  contaSteamAtiva,
  contasSteam,
  status,
  capturaPermitida,
  forcarProximaCaptura,
  vincularContaAtual,
  chaveAppid,
  lerVinculo,
  gravarVinculo,
  lerHoras,
  lerHorasDaConta,
  lerHorasDeTodasAsContas,
  VINCULO,
}
