"use strict"

const { catalogGet } = require("./catalog")
const { createRetroCatalogV2 } = require("./retro-catalog-v2")
const { getDataDir } = require("./runtime-paths")
const { normalizeRetroTitle } = require("./retro-title-parser")
const { collapseVariants, displayKey, groupVariants } = require("./retro-variants")

const PERSONAL_OFFERS_TIMEOUT_MS = 400
// Acima disto o total de jogos vira estimativa: varrer o catálogo inteiro a cada
// abertura da Loja custaria centenas de requisições.
const VARREDURA_MAX_REGISTROS = 1000
const TOTAIS_TTL_MS = 10 * 60 * 1000

function serverError(result) {
  return result?.error ? new Error(result.error.message || String(result.error)) : new Error("retro_server_unavailable")
}

function createRetroServerCatalog(options = {}) {
  const fallback = options.fallback || createRetroCatalogV2({ dataDir: options.dataDir || getDataDir() })
  const get = options.catalogGet || catalogGet
  const personalSources = options.sources || require("./sources")
  const personalOffers = new Map()
  // Total de jogos por consulta (sem offset), para o cabeçalho não mudar ao
  // paginar. Ver contarFichas.
  const totais = new Map()

  async function withFallback(remote, local) {
    try {
      const result = await remote()
      // `remote` pode devolver o envelope do servidor (`{data}`) ou o payload já
      // transformado (`{ok, games}`). `ok === false` e envelope sem `ok`
      // continuam caindo no catálogo local, como antes.
      const payload = result?.data ?? result
      if (payload?.ok) return payload
      throw serverError(result)
    } catch {
      return local()
    }
  }

  /**
   * Colapsa as variantes de uma página do servidor em fichas de jogo.
   *
   * O servidor pagina REGISTROS, não jogos: um grupo de variantes pode começar
   * no meio do lote e continuar no próximo. Por isso a varredura para no grupo
   * que não cabe inteiro e devolve `consumed` = quantos registros ela de fato
   * absorveu. O chamador avança `nextOffset` por `consumed`, então o grupo
   * partido é relido do começo na página seguinte em vez de sumir.
   *
   * (Antes: `break` sem devolver o resto — os registros já lidos mas não
   * emitidos eram pulados por `nextOffset = offset + consumed`, e o grupo
   * desaparecia da grade.)
   *
   * @param {object[]} batch - Jogos da página do servidor
   * @param {number} limit - Tamanho pedido da página
   * @returns {{games: object[], consumed: number, total: number}} -
   *   Fichas colapsadas, registros consumidos e grupos na página
   */
  function colapsarLote(batch, limit) {
    const groups = [...groupVariants(batch).values()]
    const games = []
    let consumed = 0
    for (const members of groups) {
      // Sempre emitir ao menos um grupo, senão uma página inteira dominada por
      // um único grupo grande nunca avançaria (loop infinito no cliente).
      if (consumed > 0 && consumed + members.length > limit) break
      const collapsed = collapseVariants(members)
      games.push(somarOfertas(collapsed[0], members))
      consumed += members.length
    }
    return { games, consumed, total: groups.length }
  }

  /**
   * Total de JOGOS (fichas colapsadas) da consulta.
   *
   * Só a varredura sabe o número exato, porque o colapso é global: dois registros
   * que caem em páginas diferentes formam uma ficha só. Por isso:
   *
   *  - catálogo até `VARREDURA_MAX_REGISTROS`: varre paginando e conta exato,
   *    cacheado por consulta (o resultado não muda entre páginas do cliente);
   *  - catálogo maior: estima pela razão fichas/registros da primeira página e
   *    para de tentar — varrer 28k registros a cada abertura da Loja não paga.
   *
   * A estimativa erra para cima ou para baixo conforme a densidade de variantes
   * do lote, e é por isso que ela é o último recurso, não o padrão.
   */
  async function contarFichas(consulta, params, registros) {
    const cacheado = totais.get(consulta)
    if (cacheado && Date.now() - cacheado.em < TOTAIS_TTL_MS) return cacheado.jogos

    if (registros > VARREDURA_MAX_REGISTROS) {
      // Amostra da própria página: mesma razão para toda a consulta.
      const amostra = new URLSearchParams(params)
      amostra.set("offset", "0")
      amostra.set("limit", "100")
      const result = await get(`/catalog/v1/retro/games?${amostra}`)
      const batch = result?.data?.games
      const razao = Array.isArray(batch) && batch.length ? collapseVariants(batch).length / batch.length : 1
      const estimado = Math.round(registros * razao)
      totais.set(consulta, { jogos: estimado, em: Date.now() })
      return estimado
    }

    // Varredura exata: mesma consulta, sem offset, páginas de 100.
    const vistos = new Set()
    let cursor = 0
    for (let pagina = 0; pagina < 64; pagina++) {
      const q = new URLSearchParams(params)
      q.set("offset", String(cursor))
      q.set("limit", "100")
      const result = await get(`/catalog/v1/retro/games?${q}`)
      const batch = result?.data?.games
      if (!Array.isArray(batch) || !batch.length) break
      for (const jogo of collapseVariants(batch)) vistos.add(displayKey(jogo))
      cursor += batch.length
      if (cursor >= registros || batch.length < 100) break
    }
    const jogos = vistos.size || registros
    totais.set(consulta, { jogos, em: Date.now() })
    return jogos
  }

  async function listFromServer(params) {
    const url = `/catalog/v1/retro/games?${params}`
    const result = await get(url)
    const data = result?.data
    if (!data || data.ok === false) throw serverError(result)

    // O backend pode responder sem `games`. Nesse caso a resposta passa
    // intacta: nada aqui inventa página nem clampa o total do servidor.
    if (!Array.isArray(data.games)) return { ...data, path: data.path ?? url }

    const limit = Math.min(100, Math.max(1, Number(params.get("limit")) || 24))
    const offset = Math.max(0, Number(params.get("offset")) || 0)
    const { games, consumed } = colapsarLote(data.games, limit)

    // `data.total` conta REGISTROS (LaunchBox + uma linha por release de ISO),
    // não jogos. Repassar esse número fazia o cabeçalho dizer "121 jogos" numa
    // grade de 97 fichas. Número de fichas só se sabe varrendo, então ver
    // contarFichas.
    const registros = Number(data.total) || Number(data.totalGames) || data.games.length
    const consulta = params.toString().replace(/(^|&)(offset|limit)=\d+&?/g, "$1").replace(/&$/, "")
    const totalJogos = await contarFichas(consulta, params, registros)

    return {
      ...data,
      path: data.path ?? url,
      games,
      totalGames: totalJogos,
      // Nome legado: o cabeçalho da Loja lê `total ?? totalGames`.
      total: totalJogos,
      registros,
      offset,
      limit,
      hasMore: data.games.length >= limit && offset + consumed < registros,
      nextOffset: offset + (consumed || data.games.length),
    }
  }

  // O servidor já devolve `offerCount` por jogo. O representante do grupo soma
  // as ofertas de cada variante, senão a ficha de `Agent Under Fire` mostraria
  // 4 downloads e a coreana 1, como se fossem jogos diferentes.
  function somarOfertas(representative, members) {
    const offerCount = members.reduce((total, game) => total + (Number(game.offerCount) || 0), 0)
    return offerCount > 0 ? { ...representative, offerCount } : representative
  }

  /**
   * Ofertas do jogo aberto + das variantes que o servidor separou como jogos.
   *
   * Só roda quando há o que buscar: se o detalhe já trouxe tantas ofertas quanto
   * o `offerCount` agregado da lista, nenhuma variante ficou de fora e a busca é
   * pulada — o detalhe abre sem nenhuma requisição extra na maioria dos jogos.
   *
   * A varredura usa uma busca por título (limit 100) e depois os detalhes das
   * variantes em paralelo, no máximo 8: um grupo de releases regionais não passa
   * disso, e o custo total é uma rodada de requisições, não N em fila.
   */
  async function completarOfertasDosIrmaos(game, offers) {
    const atual = Array.isArray(offers) ? offers : []
    const esperado = Number(game?.offerCount) || 0
    if (atual.length >= esperado) return atual
    const key = displayKey(game)
    try {
      const query = new URLSearchParams({ query: String(game.title || "").slice(0, 120), limit: "100" })
      const result = await get(`/catalog/v1/retro/games?${query}`)
      const siblings = (result?.data?.games || []).filter(
        (candidate) => candidate.id !== game.id && displayKey(candidate) === key,
      )
      if (!siblings.length) return atual

      const detalhes = await Promise.all(
        siblings.slice(0, 8).map((candidate) =>
          get(`/catalog/v1/retro/games/${encodeURIComponent(String(candidate.id))}`)
            .then((detail) => detail?.data)
            .catch(() => null),
        ),
      )
      const vistos = new Set(atual.map((offer) => offer.id))
      const extras = []
      for (const detail of detalhes) {
        if (!detail?.ok) continue
        for (const offer of Array.isArray(detail.offers) ? detail.offers : []) {
          if (vistos.has(offer.id)) continue
          vistos.add(offer.id)
          extras.push(offer)
        }
      }
      return [...atual, ...extras]
    } catch {
      return atual
    }
  }

  async function list(params = {}) {
    const query = new URLSearchParams()
    query.set("offset", String(Math.max(0, Number(params.offset) || 0)))
    query.set("limit", String(Math.min(100, Math.max(1, Number(params.limit) || 24))))
    if (params.query) query.set("query", String(params.query).slice(0, 120))
    if (params.variants === "all") query.set("variants", "all")
    if (params.mode === "all") query.set("mode", "all")
    const system = Array.isArray(params.systems) ? params.systems[0] : params.system
    if (system) query.set("system", String(system).slice(0, 80))
    return withFallback(
      async () => listFromServer(query),
      async () => fallback.list(params),
    )
  }

  async function buscarOfertasPessoais(game) {
    const canonical = normalizeRetroTitle(game.title).toLowerCase()
    const candidates = await personalSources.search(game.title, 50)
    const local = candidates.filter((candidate) => normalizeRetroTitle(candidate.title).toLowerCase() === canonical)
    return local.map((candidate) => {
      const id = `personal:${candidate.ref}`
      personalOffers.set(id, candidate.ref)
      return {
        id,
        sourceId: String(candidate.ref).split(":")[0],
        sourceTitle: candidate.src || "Personal source",
        originalTitle: candidate.title,
        normalizedTitle: game.title,
        systemId: game.systemId,
        fileSize: candidate.fileSize || "",
        uploadDate: candidate.uploadDate || "",
        hasUris: true,
        uriCount: 1,
        personal: true,
      }
    })
  }

  async function getGame(gameId) {
    const id = encodeURIComponent(String(gameId || "").slice(0, 240))
    const response = await withFallback(
      async () => get(`/catalog/v1/retro/games/${id}`),
      async () => fallback.getGame(gameId),
    )
    if (!response?.ok || !response.game?.title) return response

    // O detalhe do servidor traz só as ofertas do registro aberto. Quando um
    // grupo de variantes foi colapsado, a ficha mostra a soma (`5 downloads`) e
    // precisa listar as ofertas das variantes; senão o número não bate com a
    // lista e a variante fica inalcançável.
    response.offers = await completarOfertasDosIrmaos(response.game, response.offers)

    if (!personalSources?.search) return response

    // As ofertas de fontes pessoais enriquecem o detalhe, mas o primeiro clique
    // paga a construção do índice (parse de 10-15 MB por fonte). A página abre com
    // o catálogo canônico e as fontes entram se chegarem a tempo; senão o índice
    // segue aquecendo em background para o próximo clique.
    const busca = buscarOfertasPessoais(response.game).catch(() => [])
    const prazo = new Promise((resolve) => {
      const timer = setTimeout(() => resolve(null), PERSONAL_OFFERS_TIMEOUT_MS)
      if (process.versions?.electron) timer.unref?.()
    })
    const summaries = await Promise.race([busca, prazo])
    if (summaries?.length) {
      response.offers = [...(response.offers || []), ...summaries]
      response.game.offerCount = Number(response.game.offerCount || 0) + summaries.length
    }
    return response
  }

  async function getOffer(offerId) {
    const personalRef = personalOffers.get(String(offerId || ""))
    if (personalRef) {
      const resolved = await personalSources.getGame(personalRef)
      if (!resolved?.ok || !resolved.game) return { ok: false, error: resolved?.error || "personal_offer_unavailable" }
      const uris = Array.isArray(resolved.game.uris) ? resolved.game.uris : resolved.game.uri ? [resolved.game.uri] : []
      return {
        ok: true,
        offer: {
          id: offerId,
          sourceId: String(personalRef).split(":")[0],
          sourceTitle: resolved.source || "Personal source",
          originalTitle: resolved.game.title || "",
          normalizedTitle: normalizeRetroTitle(resolved.game.title || ""),
          fileSize: resolved.game.fileSize || "",
          uploadDate: resolved.game.uploadDate || "",
          hasUris: uris.length > 0,
          uriCount: uris.length,
          uris,
          personal: true,
        },
      }
    }
    const id = encodeURIComponent(String(offerId || "").slice(0, 80))
    // URIs are deliberately never written to catalog_espelho.
    return withFallback(
      async () => get(`/catalog/v1/retro/offers/${id}`, { noMirror: true }),
      async () => fallback.getOffer(offerId),
    )
  }

  async function audit(params = {}) {
    const query = new URLSearchParams()
    if (params.system) query.set("system", String(params.system).slice(0, 80))
    if (params.samples !== undefined) query.set("samples", String(Math.min(100, Math.max(0, Number(params.samples) || 0))))
    return withFallback(
      async () => get(`/catalog/v1/retro/audit?${query}`),
      async () => ({ ok: false, error: "retro_audit_unavailable" }),
    )
  }

  return {
    list,
    getGame,
    getOffer,
    audit,
    refresh: () => fallback.refresh(),
    migrateFromV1: () => fallback.migrateFromV1(),
    repository: fallback.repository,
  }
}

module.exports = { createRetroServerCatalog }
