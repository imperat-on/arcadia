"use strict"

// Feed unificado de downloads: protege as regras puras de normalização e o
// roteamento de ações (dm* para a fila Epic/Steam, torrent* para o P2P) que
// unificaram as duas telas de Downloads.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const APP = path.join(__dirname, "..")
const {
  buildFeed,
  normalizeDmItem,
  normalizeTorrentItem,
  actionsFor,
  kindKey,
  statusKey,
} = require(path.join(APP, "src/components/downloads/normalize.ts"))

const dmItem = (over = {}) => ({
  appid: "epic:Fortnite",
  appName: "Fortnite",
  title: "Fortnite",
  cover: "c1.jpg",
  status: "downloading",
  percent: 42.6,
  done: 100,
  total: 200,
  eta: "5m",
  speed: 12.5,
  error: "",
  ...over,
})

const torItem = (over = {}) => ({
  gameId: "tor:123",
  url: "magnet:?xt=urn:btih:AA",
  savePath: "C:/Games",
  title: "Repack",
  progress: 0.5,
  downloadSpeed: 1048576,
  numPeers: 7,
  numSeeds: 3,
  bytesDownloaded: 500,
  fileSize: 1000,
  ...over,
})

test("dm: kind por appid, status mapeado e velocidade MiB/s -> bytes/s", () => {
  const epic = normalizeDmItem(dmItem())
  assert.equal(epic.kind, "legendary")
  assert.equal(epic.status, "active")
  assert.equal(epic.percent, 43)
  assert.equal(epic.speedBps, 12.5 * 1024 ** 2)
  assert.equal(epic.dmDoneMiB, 100)
  assert.equal(epic.peers, -1)

  const steam = normalizeDmItem(dmItem({ appid: "steam:470220", status: "paused" }))
  assert.equal(steam.kind, "steam")
  assert.equal(steam.status, "paused")
})

test("torrent: kind por engine, precedência de status e campos convertidos", () => {
  const t1 = normalizeTorrentItem(torItem())
  assert.equal(t1.kind, "torrent")
  assert.equal(t1.status, "active")
  assert.equal(t1.percent, 50)
  assert.equal(t1.bytesDone, 500)
  assert.equal(t1.peers, 7)

  assert.equal(normalizeTorrentItem(torItem({ engine: "http" })).kind, "http")
  const debrid = normalizeTorrentItem(torItem({ engine: "debrid", cacheando: true }))
  assert.equal(debrid.kind, "debrid")
  assert.equal(debrid.status, "caching")
  // cacheando sem engine explícito também é debrid
  assert.equal(normalizeTorrentItem(torItem({ cacheando: true })).kind, "debrid")

  // Precedência: erro > completo > pausado > cacheando > ativo
  assert.equal(normalizeTorrentItem(torItem({ erro: "falhou", pausado: true })).status, "error")
  const done = normalizeTorrentItem(torItem({ completo: true }))
  assert.equal(done.status, "done")
  assert.equal(done.percent, 100)
  // Concluído usa fileSize (os campos vivos somem quando o polling para).
  assert.equal(done.bytesDone, 1000)
  assert.equal(normalizeTorrentItem(torItem({ pausado: true })).status, "paused")
})

test("buildFeed: agrupa, ordena por status e conta as duas fontes juntas", () => {
  const feed = buildFeed(
    [dmItem(), dmItem({ appid: "epic:B", status: "queued" }), dmItem({ appid: "epic:C", status: "error" })],
    [torItem({ pausado: true }), torItem({ gameId: "tor:2", completo: true }), torItem({ gameId: "tor:3", cacheando: true, engine: "debrid" })],
  )
  assert.equal(feed.total, 6)
  assert.equal(feed.ativosCount, 4)
  assert.equal(feed.falhasCount, 1)
  assert.equal(feed.concluidos.length, 1)
  // Ordem dos ativos: active -> caching -> queued -> paused (estável em empates)
  assert.deepEqual(
    feed.ativos.map((i) => i.status),
    ["active", "caching", "queued", "paused"],
  )
  assert.equal(feed.ativos[0].id, "epic:Fortnite")
  assert.equal(feed.falhas[0].id, "epic:C")
  assert.equal(feed.concluidos[0].id, "tor:2")
})

test("actionsFor: matriz de ações por estado (fila e torrent usam a mesma)", () => {
  assert.deepEqual(actionsFor("legendary", "active"), ["pause", "cancel"])
  assert.deepEqual(actionsFor("steam", "queued"), ["cancel"])
  assert.deepEqual(actionsFor("torrent", "paused"), ["resume", "cancel"])
  assert.deepEqual(actionsFor("debrid", "caching"), ["cancel"])
  assert.deepEqual(actionsFor("http", "error"), ["retry", "dismiss"])
  assert.deepEqual(actionsFor("torrent", "done"), ["dismiss"])
  assert.deepEqual(actionsFor("steam", "done"), ["dismiss"])
})

test("kindKey/statusKey: selos reusam as chaves i18n existentes", () => {
  assert.equal(kindKey("legendary"), "downloads.origem.epic")
  assert.equal(kindKey("steam"), "downloads.origem.steam")
  assert.equal(kindKey("torrent"), "downloads.origem.torrent")
  assert.equal(kindKey("http"), "downloads.origem.http")
  assert.equal(kindKey("debrid"), "downloads.origem.debrid")
  assert.equal(statusKey("active"), "downloads.status.baixando")
  assert.equal(statusKey("caching"), "torrent.status.cacheando")
  assert.equal(statusKey("queued"), "downloads.status.queued")
  assert.equal(statusKey("error"), "downloads.status.error")
})

test("wiring: o card único roteia dm* para a fila e torrent* para o P2P", () => {
  const card = fs.readFileSync(path.join(APP, "src/components/downloads/DownloadCard.tsx"), "utf8")
  for (const api of ["dmPause", "dmResume", "dmCancel", "dmRetry", "dmDismiss"]) {
    assert.ok(card.includes(`${api}(`), `card sem ${api}`)
  }
  for (const api of ["torrentPause", "torrentResume", "torrentCancel"]) {
    assert.ok(card.includes(`${api}(`), `card sem ${api}`)
  }
  // O roteamento é gated por kind — nunca chama os dois lados para o mesmo item.
  assert.match(card, /kind === "legendary" \|\| kind === "steam"/)
  // Retry de torrent = resume (o start() re-adiciona o magnet no back-end).
  assert.match(card, /type === "resume" \|\| type === "retry"/)
})

test("wiring: as duas telas usam o feed unificado e a seção antiga sumiu", () => {
  const view = fs.readFileSync(path.join(APP, "src/components/desktop/DownloadsView.tsx"), "utf8")
  const desktop = fs.readFileSync(path.join(APP, "src/components/desktop/DesktopLauncher.tsx"), "utf8")
  const manager = fs.readFileSync(
    path.join(APP, "src/components/ps5-launcher/DownloadManager.tsx"),
    "utf8",
  )
  assert.match(view, /useDownloadsFeed/)
  // A aba recebe o feed do launcher e não abre uma segunda assinatura dos
  // mesmos canais IPC quando o feed já veio de cima.
  assert.match(view, /useDownloadsFeed\(!feedProp\)/)
  assert.match(desktop, /<DownloadsView feed=\{downloadsFeed\} \/>/)
  assert.match(manager, /useDownloadsFeed/)
  assert.ok(view.includes("DownloadCard"), "desktop sem card unificado")
  assert.ok(manager.includes("DownloadCard"), "console sem card unificado")
  assert.ok(!fs.existsSync(path.join(APP, "src/components/desktop/TorrentSection.tsx")))
  assert.ok(!view.includes("TorrentSection"))
  assert.ok(manager.includes("downloads.secao.falhas"))
})

test("i18n: as três traduções têm as chaves novas do feed", () => {
  const keys = [
    "downloads.origem.debrid",
    "downloads.origem.epic",
    "downloads.origem.http",
    "downloads.origem.steam",
    "downloads.origem.torrent",
    "downloads.secao.concluidos",
    "downloads.secao.falhas",
    "downloads.vazio_sub",
    "downloads.vazio_titulo",
  ]
  for (const loc of ["pt-BR", "en-US", "es-ES"]) {
    const dict = JSON.parse(fs.readFileSync(path.join(APP, `src/i18n/${loc}.json`), "utf8"))
    for (const k of keys) {
      assert.ok(typeof dict[k] === "string" && dict[k].length > 0, `${loc} sem ${k}`)
    }
  }
})
