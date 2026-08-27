"use strict"

// Sync de BIBLIOTECA (jogos custom) + HORAS jogadas — MAIN PROCESS.
// Cada conta tem a própria coleção no servidor (user_library/user_playtime).
//
// PUSH (2s após mudanças locais, debounce):
//   - jogos custom: diff local vs watermark (libPush) → upsert/removidos
//   - horas: delta = total local - último enviado → servidor ACUMULA
// PULL (no login): traz os jogos que faltam (exe vazio — usuário configura)
//   e sobe o display de horas pro total da conta (maior vence localmente).
//
// Watermarks ficam no sync_state.json (por conta, via conta.js) sob as chaves
// libPush / playtimePush — mesmas chaves não usadas pelo sync de conquistas.

const fs = require("fs")
const path = require("path")
const { getClient } = require("./client")
const { caminhoArquivoConta, DATA_DIR } = require("./conta")
const { ownedSet, readOwned } = require("../owned")
const { conta } = require("./conta")
const steamstore = require("../steamstore")
const { catalogGet } = require("../catalog")
const { readLibraryFile } = require("../library-store")
const { resolveLibraryConflict, resolvePlaytimeConflict } = require("../../../contracts")

const CUSTOM = () => caminhoArquivoConta("custom_games.json")
const PENDING = () => caminhoArquivoConta("pending_games.json")
const OVERRIDES = () => caminhoArquivoConta("overrides.json")
const STATE = () => caminhoArquivoConta("sync_state.json")
const OWNED = () => caminhoArquivoConta("owned_games.json")

let listeners = []
function onChanged(cb) {
  listeners.push(cb)
}
function avisar(mudou) {
  if (mudou) for (const cb of listeners) cb("library:changed")
}

// ---------- util ----------
function readJson(file, def) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"))
  } catch {
    return def
  }
}
function writeJson(file, obj) {
  const tmp = file + ".tmp"
  fs.writeFileSync(tmp, JSON.stringify(obj, null, 2))
  fs.renameSync(tmp, file)
}
function loadState() {
  return readJson(STATE(), {})
}
function saveState(st) {
  writeJson(STATE(), st)
}

async function usuarioAtual() {
  const { data } = await getClient().auth.getUser()
  return data?.user ?? null
}

// Paths de conta são funções (não constantes): um logout/login pode trocar o
// escopo enquanto o RPC está em voo. Capturamos o username antes da rede e
// nunca aplicamos uma resposta antiga no diretório da conta nova.
function contaAindaAtiva(contexto) {
  return conta() === contexto
}

function erroContaTrocada() {
  return { ok: false, error: "conta_trocada", retryable: false }
}

const retroMetadataCache = new Map()

function isRetroGame(gameOrId) {
  const id = typeof gameOrId === "string" ? gameOrId : gameOrId?.id
  return String(id || "").startsWith("retro:") || gameOrId?.retro === true || gameOrId?.launcher === "retro"
}

function retroMetadataFrom(value) {
  if (!value || typeof value !== "object") return {}
  const artwork = value.artwork && typeof value.artwork === "object" ? value.artwork : {}
  const cover = value.cover || value.capa || value.fallbackCover || artwork.cover || ""
  const hero = value.hero || value.heroi || artwork.hero || artwork.background || cover || ""
  // Retrôs usam a capa como arte portátil principal. Um ícone legado diferente
  // não pode fazer a sidebar variar entre máquinas; só cai para ele quando não
  // existe capa disponível.
  const icon = cover || value.icon || value.logo || artwork.icon || ""
  const genres = Array.isArray(value.genres)
    ? value.genres.filter((item) => typeof item === "string").map((item) => item.trim()).filter(Boolean).slice(0, 32)
    : undefined
  return {
    ...(cover ? { cover: String(cover).slice(0, 2000) } : {}),
    ...(hero ? { hero: String(hero).slice(0, 2000) } : {}),
    ...(icon ? { icon: String(icon).slice(0, 2000) } : {}),
    ...(value.description || value.summary ? { description: String(value.description || value.summary).slice(0, 10000) } : {}),
    ...(genres?.length ? { genres } : {}),
    ...(value.systemId ? { systemId: String(value.systemId).slice(0, 120) } : {}),
    ...(Number.isInteger(value.releaseYear) ? { releaseYear: value.releaseYear } : {}),
    ...(value.developer ? { developer: Array.isArray(value.developer) ? String(value.developer[0] || "") : String(value.developer) } : {}),
    ...(value.publisher ? { publisher: Array.isArray(value.publisher) ? String(value.publisher[0] || "") : String(value.publisher) } : {}),
  }
}

async function carregarMetadataRetro(id) {
  const chave = String(id || "")
  if (!isRetroGame(chave)) return {}
  if (retroMetadataCache.has(chave)) return retroMetadataCache.get(chave)
  const emVoo = (async () => {
    try {
      const encoded = encodeURIComponent(chave.slice(0, 240))
      const response = await catalogGet(`/catalog/v1/retro/games/${encoded}`, { timeoutMs: 10000 })
      return retroMetadataFrom(response?.data?.game)
    } catch {
      return {}
    }
  })()
  retroMetadataCache.set(chave, emVoo)
  const metadata = await emVoo
  retroMetadataCache.set(chave, metadata)
  return metadata
}

function payloadBiblioteca(game) {
  const retro = isRetroGame(game)
  const metadata = retroMetadataFrom(game)
  return {
    appid: game.id,
    title: game.title || game.id,
    platform: retro ? "emulator" : game.platform || "windows",
    ...(retro ? { retro: true, ...metadata } : {}),
  }
}

// ---------- PUSH ----------
async function push() {
  const contexto = conta()
  const user = await usuarioAtual()
  if (!user) return
  if (!contaAindaAtiva(contexto)) return erroContaTrocada()

  const st = loadState()
  const enviados = st.libPush || {}
  const wp = st.playtimePush || {}

  // Jogos custom: diff local vs watermark
  const lib = readJson(CUSTOM(), [])
  const ids = new Set(lib.map((g) => g.id))
  const p_lib = []
  for (const g of lib) {
    const prev = enviados[g.id]
    const payload = payloadBiblioteca(g)
    if (
      !prev ||
      prev.title !== payload.title ||
      prev.platform !== payload.platform ||
      prev.cover !== payload.cover ||
      prev.hero !== payload.hero ||
      prev.icon !== payload.icon
    ) {
      p_lib.push(payload)
    }
  }

  // Jogos possuídos (owned.js) que não são custom: sobem com o título real
  // (do pending_games ou do id) pra não virar nome feio (ex: "steam:3240220")
  // na outra máquina. Reenvia quando o título mudou em relação ao watermark —
  // sem isto, um jogo que subiu com título feio ("Steam <appid>") nunca era
  // corrigido nas outras máquinas.
  const owned = ownedSet()
  const pendentes = readJson(PENDING(), [])
  // Título real do jogo: primeiro o pending (adicionado pela loja), depois o
  // snapshot global local, quando existir. Sem esse fallback, um jogo antigo
  // sem stub pending poderia subir com o id feio ("steam:1091500").
  let libGlobal = null
  try {
    libGlobal = readLibraryFile(path.join(DATA_DIR, "library.json")).games
  } catch {
    libGlobal = null
  }
  const tituloDe = (id) => {
    const p = pendentes.find((x) => x.id === id)
    if (p?.title && p.title !== id && !String(p.title).startsWith("Steam ")) return p.title
    if (Array.isArray(libGlobal)) {
      const g = libGlobal.find((x) => x && x.id === id)
      if (g?.title) return g.title
    }
    return p?.title || id
  }
  for (const id of owned) {
    if (ids.has(id)) continue
    const prev = enviados[id]
    const titulo = tituloDe(id)
    if (!prev || prev.title !== titulo) {
      p_lib.push({ appid: id, title: titulo, platform: "windows" })
    }
  }

  // Removidos localmente (sumiram do arquivo, mas já tinham sido enviados).
  // Cobre custom (fora do custom_games.json) E steam (fora do owned_games):
  // sem isto, remover um jogo steam numa maquina nao sumia na outra — o
  // watermark ficava e o push nunca marcava removed.
  for (const id of Object.keys(enviados)) {
    if (!ids.has(id) && !owned.has(id)) p_lib.push({ appid: id, removed: true })
  }

  // Horas: delta acumulado desde o último push
  const overrides = readJson(OVERRIDES(), {})
  const p_playtime = []
  for (const [gid, data] of Object.entries(overrides)) {
    const total = Number(data?.playtime_added_minutes) || 0
    const base = Number(wp[gid]) || 0
    if (total > base) p_playtime.push({ appid: gid, minutes: total - base })
  }

  if (!p_lib.length && !p_playtime.length) return
  if (!contaAindaAtiva(contexto)) return erroContaTrocada()

  const { error } = await getClient().rpc("push_library", { p_lib, p_playtime })
  if (!contaAindaAtiva(contexto)) return erroContaTrocada()
  if (error) {
    console.error("[biblioteca] push falhou:", error.message)
    return
  }

  // Atualiza watermarks
  for (const g of p_lib) {
    if (g.removed) delete enviados[g.appid]
    else enviados[g.appid] = {
      title: g.title,
      platform: g.platform || "windows",
      ...(g.cover ? { cover: g.cover } : {}),
      ...(g.hero ? { hero: g.hero } : {}),
      ...(g.icon ? { icon: g.icon } : {}),
    }
  }
  for (const p of p_playtime) wp[p.appid] = (Number(wp[p.appid]) || 0) + p.minutes
  st.libPush = enviados
  st.playtimePush = wp
  if (!contaAindaAtiva(contexto)) return erroContaTrocada()
  saveState(st)
}

// ---------- PULL ----------
async function pull() {
  const contexto = conta()
  const user = await usuarioAtual()
  if (!user) return false
  if (!contaAindaAtiva(contexto)) return false

  const { data, error } = await getClient().rpc("pull_library")
  if (!contaAindaAtiva(contexto)) return false
  if (error || !Array.isArray(data)) {
    console.error("[biblioteca] pull falhou:", error?.message || "sem dados")
    return false
  }

  let mudou = false
  const st = loadState()
  const enviados = st.libPush || {}
  const wp = st.playtimePush || {}

  // Posse: owned_games.json ausente (null) significa "possui tudo" (ainda
  // nao migrou, ver constraint 7). Nesse caso o pull nao mexe na posse, so
  // quando ja existe arquivo real e que o appid novo entra.
  const rawOwned = readOwned()
  const owned = rawOwned === null ? null : new Set(rawOwned)
  let ownedMudou = false

  // Jogos que faltam localmente entram como custom (exe vazio — usuário
  // configura na máquina nova; título/plataforma vêm do servidor)
  const lib = readJson(CUSTOM(), [])
  const ids = new Set(lib.map((g) => g.id))
  // Stubs pending existentes (para nao duplicar jogos steam: vindos do servidor)
  const pendentes = readJson(PENDING(), [])
  const pendentesIds = new Set(pendentes.map((p) => p && p.id))
  let pendentesMudou = false
  const idsDoServidor = new Set(data.map((row) => row && row.appid))

  // Retrôs que já foram sincronizados precisam sair também do snapshot local
  // quando forem removidos em outra máquina. Jogos retrô ainda não enviados
  // permanecem locais para não perder uma adição que está aguardando o push.
  const retroRemovidos = lib.filter((game) => isRetroGame(game) && enviados[game.id] && !idsDoServidor.has(game.id))
  if (retroRemovidos.length) {
    const removerIds = new Set(retroRemovidos.map((game) => game.id))
    const restantes = lib.filter((game) => !removerIds.has(game.id))
    lib.length = 0
    lib.push(...restantes)
    mudou = true
    for (const id of removerIds) {
      if (owned !== null && owned.delete(id)) ownedMudou = true
      // O jogo já foi removido no servidor. Limpar o watermark permite que
      // uma futura adição local do mesmo retrô seja enviada novamente.
      delete enviados[id]
    }
  }

  // Retrôs antigos foram sincronizados apenas com id/título. Busca a ficha
  // canônica para que o notebook recupere capa/hero mesmo quando o servidor
  // ainda devolve o formato reduzido do RPC legado.
  const metadataRetro = new Map()
  const retroRows = data.filter((row) => isRetroGame(row?.appid))
  const localGamesById = new Map(lib.map((game) => [game.id, game]))
  await Promise.all(
    retroRows.map(async (row) => {
      const local = retroMetadataFrom({ ...localGamesById.get(row.appid), ...row })
      if (local.cover || local.hero || local.icon) {
        metadataRetro.set(row.appid, local)
        return
      }
      metadataRetro.set(row.appid, await carregarMetadataRetro(row.appid))
    }),
  )
  if (!contaAindaAtiva(contexto)) return false

  // Busca capa/hero/icone REAIS antes de criar os stubs pending: sem isto o
  // stub nascia so com URLs de capa "chutadas" (podem nao existir) e sem
  // icone nenhum, e a sidebar so corrigia isso depois, ao abrir library:get
  // (curarCapasSteam) — dava um "flash" de carregamento visivel. Buscando
  // aqui, o stub ja nasce com a arte certa.
  const novosSteamIds = data
    .filter((row) => row && String(row.appid).startsWith("steam:") && !ids.has(row.appid) && !pendentesIds.has(row.appid))
    .map((row) => String(row.appid).replace(/^steam:/, ""))
  let itensMapa = new Map()
  if (novosSteamIds.length) {
    // Pre-popula o cache local ANTES do pull buscar: o itensDaLoja abaixo
    // acha no cache (0 rede) e o stub nasce com arte real desde a primeira
    // montagem — sem flash de capa cinza na sidebar. Se a Steam falhar aqui,
    // segue o fluxo normal (stub com arte chutada, curada em background).
    steamstore.popularItens(novosSteamIds).catch(() => {})
    try {
      const r = await steamstore.itensDaLoja(novosSteamIds)
      itensMapa = r.mapa
    } catch {
      /* loja indisponivel: stub nasce com a arte chutada, curada depois */
    }
    if (!contaAindaAtiva(contexto)) return false
  }

  for (const row of data) {
    if (owned !== null && !owned.has(row.appid)) {
      owned.add(row.appid)
      ownedMudou = true
    }
    const ehSteam = String(row.appid).startsWith("steam:")
    if (ehSteam) {
      // Jogos steam:* do servidor entram na POSSE e ganham um stub PENDING se
      // nao existirem localmente. Sem o stub, o jogo adicionado na loja de
      // OUTRA maquina nao aparecia na biblioteca aqui sem um stub local.
      // O stub permanece até o jogo ser instalado ou removido localmente.
      if (!ids.has(row.appid) && !pendentesIds.has(row.appid)) {
        const appid = String(row.appid).replace(/^steam:/, "")
        const base = "https://cdn.cloudflare.steamstatic.com/steam/apps/" + appid
        const it = itensMapa.get(appid)
        pendentes.push({
          id: row.appid,
          title: (row.title && row.title !== row.appid && String(row.title).trim()) || `Steam ${appid}`,
          launcher: "steam",
          launch_cmd: ["steam", `steam://rungameid/${appid}`],
          installed: false,
          cover: it?.capa || `${base}/library_600x900.jpg`,
          hero: it?.heroi || `${base}/library_hero.jpg`,
          logo: `${base}/logo.png`,
          icon: it?.icon || "",
          pendente: true,
        })
        pendentesIds.add(row.appid)
        pendentesMudou = true
      } else if (pendentesIds.has(row.appid)) {
        // Stub pending já existe mas com título feio (ex: "Steam 12345" ou
        // "steam:12345"): atualiza quando o servidor manda um nome melhor.
        // O pull anterior podia ter criado o stub antes de o servidor ter o
        // título real (o push da outra máquina ainda não tinha reenviado).
        const p = pendentes.find((x) => x && x.id === row.appid)
        if (p) {
          const atual = String(p.title || "")
          const feio = atual === `Steam ${String(row.appid).replace(/^steam:/, "")}` || atual === row.appid
          const novo = row.title && row.title !== row.appid ? String(row.title).trim() : ""
          if (feio && novo && novo !== atual) {
            p.title = novo
            pendentesMudou = true
          }
        }
      }
    } else if (!ids.has(row.appid)) {
      const retro = isRetroGame(row.appid)
      const metadata = retro ? metadataRetro.get(row.appid) || {} : {}
      lib.push({
        id: row.appid,
        title: row.title || row.appid,
        launcher: retro ? "retro" : "custom",
        platform: retro ? metadata.systemId || row.systemId || "emulator" : row.platform || "windows",
        exe: "",
        installed: false,
        ...(retro ? { retro: true, ...metadata } : {}),
      })
      mudou = true
    } else {
      const g = lib.find((x) => x.id === row.appid)
      const retro = isRetroGame(row.appid) || isRetroGame(g)
      const metadata = retro ? metadataRetro.get(row.appid) || {} : {}
      const merged = resolveLibraryConflict(
        { appid: g.id, title: g.title, platform: g.platform },
        { appid: row.appid, title: row.title || row.appid, platform: retro ? "emulator" : row.platform || "windows" },
      )
      if (merged && !merged.removed) {
        if (g.title !== merged.title) {
          g.title = merged.title
          mudou = true
        }
        if (merged.platform && g.platform !== merged.platform) {
          g.platform = merged.platform
          mudou = true
        }
        if (retro) {
          const preenchido = {
            retro: true,
            launcher: "retro",
            ...(metadata.systemId ? { systemId: metadata.systemId, platform: metadata.systemId } : {}),
            ...metadata,
          }
          for (const [key, value] of Object.entries(preenchido)) {
            if (value !== undefined && value !== "" && g[key] !== value) {
              g[key] = value
              mudou = true
            }
          }
        }
      }
    }
  }
  // Remocao no pull: jogos que sumiram do servidor (removidos em outra
  // maquina) saem da posse local. Sem isto, remover na conta em um dispositivo
  // nunca propagava — o owned_games.json local ficava com o jogo para sempre.
  // So remove da POSSE (owned): o jogo continua no snapshot local se já
  // instalado — ele só deixa de ser "possuído pela conta".
  // Stub PENDING nao bloqueia mais a remocao: o stub e criado pelo proprio
  // pull (nao e dado do usuario) — antes o pendentesIds.has(id) impedia a
  // remocao pra sempre, entao um jogo so puxado (nunca instalado) na outra
  // maquina nunca podia ser removido por sync.
  if (owned !== null) {
    for (const id of [...owned]) {
      const customRetroSincronizado = isRetroGame(id) && enviados[id]
      if (!idsDoServidor.has(id) && (!ids.has(id) || customRetroSincronizado)) {
        owned.delete(id)
        ownedMudou = true
      }
    }
  }
  // Stubs pendentes cujo jogo sumiu do servidor tambem saem — senao a entrada
  // fantasma (instalado:false) fica pra sempre em pending_games.json mesmo
  // apos o owned ja ter sido limpo acima.
  const pendentesRestantes = pendentes.filter((p) => !p || idsDoServidor.has(p.id))
  if (pendentesRestantes.length !== pendentes.length) {
    pendentes.length = 0
    pendentes.push(...pendentesRestantes)
    pendentesMudou = true
  }
  if (!contaAindaAtiva(contexto)) return false
  if (pendentesMudou) writeJson(PENDING(), pendentes)
  if (mudou) writeJson(CUSTOM(), lib)
  if (ownedMudou) {
    writeJson(OWNED(), [...owned])
    mudou = true
  }

  // Horas: total da conta > local → display local sobe + watermark acompanha
  const overrides = readJson(OVERRIDES(), {})
  for (const row of data) {
    const total = Number(row.minutes) || 0
    const local = Number(overrides[row.appid]?.playtime_added_minutes) || 0
    const merged = resolvePlaytimeConflict(local, total)
    if (merged > local) {
      overrides[row.appid] = { ...(overrides[row.appid] || {}), playtime_added_minutes: merged }
      wp[row.appid] = merged
      mudou = true
    }
  }
  if (mudou) writeJson(OVERRIDES(), overrides)

  // Watermark dos jogos que vieram do servidor: sem isto, um jogo adicionado
  // em OUTRA maquina e apenas puxado aqui NAO tem libPush local — remover
  // neste dispositivo nao sobe removed:true (o push so itera watermarks) e o
  // jogo volta no proximo pull. Criar o watermark no pull faz a remocao
  // local propagar para o servidor e para as outras maquinas.
  let libPushMudou = false
  for (const row of data) {
    const id = row && row.appid
    if (!id) continue
    const titulo = row.title && row.title !== id ? String(row.title).trim() : undefined
    if (!enviados[id]) {
      enviados[id] = { title: titulo || id, platform: row.platform || "windows" }
      libPushMudou = true
    } else if (titulo && enviados[id].title !== titulo && enviados[id].title === id) {
      // Watermark com titulo feio (appid) ganha o titulo real do servidor
      enviados[id] = { ...enviados[id], title: titulo }
      libPushMudou = true
    }
  }
  if (libPushMudou) {
    st.libPush = enviados
    mudou = true
  }

  st.playtimePush = wp
  if (!contaAindaAtiva(contexto)) return false
  saveState(st)
  return mudou
}

// ---------- RECONCILE (login / boot) ----------
async function reconcile() {
  // PULL antes do PUSH: o pull traz as remocoes feitas em OUTRAS maquinas e
  // limpa o owned_games.json local ANTES do push reenviar. Com push-primeiro,
  // uma maquina que ainda tinha o jogo no owned local (nunca puxou a remocao)
  // reenviava o jogo ao servidor, ressuscitando-o apos o DELETE da outra
  // maquina, evitando que um push com estado antigo ressuscite a remoção.
  const mudou = await pull()
  await push()
  avisar(mudou)
}

// ---------- PUSH imediato (sem debounce) ----------
// Sincroniza na hora após mudanças locais. A proteção de troca de conta
// fica no push() (que usa a sessão atual via requireUserId).
function agendarPush() {
  push().catch((e) => console.error("[biblioteca] push falhou:", e?.message))
}

// ---------- REALTIME (canal library-<me>) ----------
// Sem isto, o jogo adicionado numa maquina só chegava na outra no próximo
// boot/login (reconcile roda uma vez só) — podia demorar horas. O servidor
// avisa via WebSocket (canal library-<me>) assim que um push_library com
// jogos de fato acontece, e aqui a gente puxa na hora. Mesmo padrão de
// friends.watchRequests().
function watchChanges() {
  let channel = null
  let iniciando = null // serializa starts concorrentes (SIGNED_IN duplo do boot)

  const stop = async () => {
    if (channel) {
      try {
        await getClient().removeChannel(channel)
      } catch {
        /* ignore */
      }
      channel = null
    }
  }

  const start = () => {
    if (iniciando) return iniciando
    iniciando = (async () => {
      await stop()
      const user = await usuarioAtual()
      if (!user) return
      channel = getClient().channel(`library-${user.id}`)
      channel.on(
        "postgres_changes",
        { event: "*", schema: "public", table: "user_library" },
        () => {
          pull()
            .then((mudou) => avisar(mudou))
            .catch((e) => console.error("[biblioteca] pull via realtime falhou:", e?.message))
        },
      )
      // Playtime mudou em outra máquina: o servidor emite postgres_changes na
      // tabela user_playtime — puxa na hora pra atualizar o display sem
      // reiniciar. Antes so atualizava no proximo login/boot.
      channel.on(
        "postgres_changes",
        { event: "*", schema: "public", table: "user_playtime" },
        () => {
          pull()
            .then((mudou) => avisar(mudou))
            .catch((e) => console.error("[biblioteca] pull playtime via realtime falhou:", e?.message))
        },
      )
      channel.subscribe()
    })().finally(() => {
      iniciando = null
    })
    return iniciando
  }

  return { start, stop }
}

module.exports = { push, pull, reconcile, agendarPush, onChanged, watchChanges }
