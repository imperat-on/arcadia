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
const path = require("path")

const { contasDoLoginUsers, contaAtivaDoLoginUsers, horasDoLocalConfig } = require("./vdf")
const { getDataDir } = require("./runtime-paths")

const VINCULO = "steam_account.json"

// Interruptor "capturar automaticamente" (B2). Lido direto do config do app para o
// vigia e o loader não precisarem de plumbing.
//
// LIGADO POR PADRÃO: capturar é o comportamento normal do app, e desligar é uma
// escolha explícita do usuário (fica gravada no config). O portão de conta (B1)
// continua valendo por cima: trocou a conta da Steam, a captura pausa de qualquer
// jeito.
//
// NÃO montar este caminho na mão: no Windows a raiz é %LOCALAPPDATA%\arcadia e a
// versão anterior apontava para ~/.local/share/arcadia (raiz do Linux). Resultado:
// no Windows o arquivo nunca era encontrado, o catch devolvia `false` e a captura
// ficava permanentemente desligada — o interruptor da UI gravava no config CERTO e
// o leitor procurava em OUTRO lugar. `getDataDir()` é a única fonte da raiz.
function autoLigado() {
  try {
    const cfg = JSON.parse(fs.readFileSync(path.join(getDataDir(), "config.json"), "utf-8"))
    // Só DESLIGA com valor explícito: sem a chave, vale o padrão (ligada).
    return cfg.achievements_auto_capture !== false
  } catch {
    // Config ausente/ilegível não é escolha do usuário: vale o padrão (ligada).
    return true
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
    // Por que passou (ou não) — o chamador registra isto no log.
    forcado: forcar,
    vinculoOk: bate,
    semSteam,
    auto,
    contasSteam: contasSteam(),
  }
}

/** Atalho para os pontos de captura: pode ingerir/revogar agora? */
function capturaPermitida(caminhoConta, log, opts) {
  const efetivo = Object.assign({}, opts)
  if (efetivo.forcar === undefined) {
    // O botão "Capturar agora" libera UMA passada de cada consumidor — o vigia de
    // crack e o loader de schemas liberam o deles e o ciclo seguinte volta à regra.
    if (consumirForca(String(efetivo.componente || "__default__"))) efetivo.forcar = true
  }
  delete efetivo.componente
  const s = status(caminhoConta, log, efetivo)
  return { permitido: s.permitido, status: s }
}

// Marcado pelo IPC "Capturar agora" (em memória).
//
// Era um booleano consumido pela PRIMEIRA chamada que chegasse. Como o loader
// chama capturaPermitida UMA VEZ POR APPID, o flag era gasto no primeiro appid e
// os outros 41 continuavam bloqueados — o botão parecia não fazer nada para os
// jogos sem bin da Steam. Agora cada `componente` consome a sua passada; a
// liberação também expira sozinha, para nunca sobrar força pendente na sessão.
const JANELA_FORCA_MS = 120000
const forcar = { pendente: false, expira: 0, consumidoPor: new Set() }

function consumirForca(componente) {
  if (!forcar.pendente || Date.now() > forcar.expira) {
    forcar.pendente = false
    return false
  }
  if (forcar.consumidoPor.has(componente)) return false
  forcar.consumidoPor.add(componente)
  return true
}

function forcarProximaCaptura() {
  forcar.pendente = true
  forcar.expira = Date.now() + JANELA_FORCA_MS
  forcar.consumidoPor.clear()
}

/** Motivo legível do bloqueio. O log precisa dizer POR QUE nada foi ingerido —
 *  "progresso do bin ignorado" sem motivo já custou uma investigação inteira. */
function motivoPausa(s) {
  if (!s) return "sem-status"
  if (s.forcado) return "liberado-pelo-botao"
  if (!s.auto) return "captura-automatica-desligada"
  if (s.contaAtual && !s.vinculo) return "sem-vinculo-de-conta"
  if (s.contaAtual && s.vinculo && !s.vinculoOk) return "conta-steam-trocada"
  return "regra-de-captura"
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
  motivoPausa,
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
