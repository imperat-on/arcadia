// HowLongToBeat: busca de tempos de jogo via scrape do site (API seek
// protegida por hash do bundle _next). Cache local de 30 dias.
// Falha silenciosa: qualquer erro retorna null, nunca lança.
const fs = require("fs")
const path = require("path")
const os = require("os")
const { fetchRede } = require("./httpfetch")

// Mesmo DATA_DIR de main.js (HOME/.local/share/arcadia).
const CACHE_PATH = path.join(os.homedir(), ".local/share/arcadia", "hltb_cache.json")

const TTL_MS = 30 * 24 * 60 * 60 * 1000

const UA =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"

// Fallback quando a extração do bundle falhar. Os hashes mudam a cada deploy
// do site; a extração runtime é o caminho principal. ponytail: se o padrão do
// bundle mudar, atualizar descobrirHash (ou colocar hash recente aqui).
const SEEK_HASHES_FALLBACK = []

function normalizar(s) {
  return String(s || "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .trim()
}

function carregarCache() {
  try {
    const c = JSON.parse(fs.readFileSync(CACHE_PATH, "utf-8"))
    return c && typeof c === "object" && !Array.isArray(c) ? c : {}
  } catch {
    return {}
  }
}

function salvarCache(c) {
  try {
    const tmp = CACHE_PATH + ".tmp"
    fs.writeFileSync(tmp, JSON.stringify(c, null, 2), "utf-8")
    fs.renameSync(tmp, CACHE_PATH)
  } catch {
    /* cache não é crítico */
  }
}

async function descobrirHash() {
  const html = await (
    await fetchRede("https://howlongtobeat.com/", { headers: { "User-Agent": UA } })
  ).text()
  const m1 = html.match(/_next\/static\/chunks\/pages\/(_app-[a-f0-9]+\.js)/i)
  if (!m1) return null
  const bundleUrl = `https://howlongtobeat.com/_next/static/chunks/pages/${m1[1]}`
  const js = await (
    await fetchRede(bundleUrl, {
      headers: { "User-Agent": UA, Referer: "https://howlongtobeat.com/" },
    })
  ).text()
  const m2 =
    js.match(/["']\/api\/seek\/["']\s*\+\s*["']([a-f0-9]+)["']/i) ||
    js.match(/["']\/api\/seek\/["']\.concat\(\s*["']([a-f0-9]+)["']/i) ||
    js.match(/api\/seek\/([a-f0-9]{20,})/i)
  return m2 ? m2[1] : null
}

async function seek(hash, titulo) {
  const body = {
    searchType: "games",
    searchTerms: normalizar(titulo).split(" ").filter(Boolean),
    searchPage: 1,
    size: 5,
    searchOptions: {
      games: {
        userId: 0,
        platform: "",
        sortCategory: "popular",
        rangeCategory: "main",
        rangeTime: { min: null, max: null },
        gameplay: { perspective: "", flow: "", genre: "" },
        modifier: "",
      },
      users: { sortCategory: "postcount" },
      filter: "",
      sort: 0,
      randomizer: 0,
    },
  }
  const r = await fetchRede(`https://howlongtobeat.com/api/seek/${hash}`, {
    method: "POST",
    signal: AbortSignal.timeout(15000),
    headers: {
      "User-Agent": UA,
      Referer: "https://howlongtobeat.com/",
      Origin: "https://howlongtobeat.com",
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  })
  if (!r.ok) throw new Error("hltb http " + r.status)
  return await r.json()
}

async function hltbBuscar(titulo) {
  const key = normalizar(titulo)
  if (!key) return null
  const cache = carregarCache()
  const c = cache[key]
  if (c && Date.now() - Number(c.ts || 0) < TTL_MS) {
    return {
      main: c.main || 0,
      mainExtra: c.mainExtra || 0,
      completionist: c.completionist || 0,
      ts: c.ts,
    }
  }
  let hash = null
  try {
    hash = await descobrirHash()
  } catch {
    /* site fora ou mudou: cai no fallback */
  }
  if (!hash) {
    for (const h of SEEK_HASHES_FALLBACK) {
      hash = h
      break
    }
  }
  if (!hash) return null
  let data = null
  try {
    data = await seek(hash, titulo)
  } catch {
    return null
  }
  const items = Array.isArray(data?.data) ? data.data : []
  if (!items.length) return null
  const escolhido = items.find((it) => normalizar(it.game_name) === key) || items[0]
  const out = {
    main: Math.round((Number(escolhido.comp_main) || 0) / 60),
    mainExtra: Math.round((Number(escolhido.comp_plus) || 0) / 60),
    completionist: Math.round((Number(escolhido.comp_100) || 0) / 60),
    ts: Date.now(),
  }
  if (!out.main && !out.mainExtra && !out.completionist) return null
  cache[key] = out
  salvarCache(cache)
  return out
}

module.exports = { hltbBuscar }
