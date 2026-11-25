"use strict"

// Bloqueio "release sem debrid não baixa" (política 2026-09-12): guard no
// engine (start/resume), IPC debrid:status, gates no diálogo e nas fontes.
// O runtime roda em subprocesso (o polling do torrent.js arma intervalos;
// process.exit garante que o teste não pendura o runner).

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { spawnSync } = require("node:child_process")

const raiz = path.join(__dirname, "..")
const ler = (rel) => fs.readFileSync(path.join(raiz, rel), "utf8")

test("engine: start() bloqueia magnet sem debrid e registra o motivo", () => {
  const script = `
const fs = require("fs"), os = require("os"), path = require("path")
const dd = fs.mkdtempSync(path.join(os.tmpdir(), "dgate-"))
process.env.ARCADIA_DATA_DIR = dd
const t = require(${JSON.stringify(path.join(raiz, "electron/torrent.js"))})
;(async () => {
  const r = await t.start({
    gameId: "tor:sp2",
    url: "magnet:?xt=urn:btih:" + "a".repeat(40) + "&dn=x",
    savePath: path.join(dd, "dl"),
  })
  if (!r || r.ok !== false || !/Sem debrid/.test(String(r.error || ""))) {
    console.error("FAIL start: " + JSON.stringify(r)); process.exit(1)
  }
  const it = (t.list() || []).find((i) => i.gameId === "tor:sp2")
  if (!it || !/Sem debrid/.test(String(it.erro || ""))) {
    console.error("FAIL estado: " + JSON.stringify(it)); process.exit(1)
  }
  fs.writeFileSync(
    path.join(dd, "torrent_state.json"),
    JSON.stringify([{ gameId: "tor:old", engine: "aria2", url: "magnet:?xt=urn:btih:" + "b".repeat(40), pausado: true }]),
  )
  const r2 = await t.resume("tor:old")
  if (!r2 || r2.ok !== false || !/Sem debrid/.test(String(r2.error || ""))) {
    console.error("FAIL resume: " + JSON.stringify(r2)); process.exit(1)
  }
  console.log("GATE_OK"); process.exit(0)
})().catch((e) => { console.error("ERR " + e); process.exit(1) })
`
  const out = spawnSync(process.execPath, ["-e", script], { timeout: 30000, encoding: "utf8" })
  assert.equal(out.status, 0, `subprocesso falhou: ${out.stderr || out.stdout}`)
  assert.match(out.stdout, /GATE_OK/)
})

test("engine: guard usa temDebridConfigurado e exports o helper", () => {
  const t = ler("electron/torrent.js")
  assert.match(t, /const ERRO_DEBRID = "Sem debrid conectado/)
  assert.match(t, /if \(!temDebridConfigurado\(\)\) \{/)
  assert.match(t, /it\.engine === "aria2" && !temDebridConfigurado\(\)/)
  assert.match(t, /^  temDebridConfigurado,$/m)
  // Política nova: o caminho antigo de P2P sem debrid não decide mais sozinho.
  assert.ok(t.includes("dormente"))
})

test("IPC: debrid:status exposto e torrent:swarm removido", () => {
  assert.match(ler("electron/main.js"), /ipcMain\.handle\("debrid:status", \(\) => \(\{ ok: true, configured:/)
  assert.match(ler("electron/preload.js"), /debridStatus: \(\) => ipcRenderer\.invoke\("debrid:status"\)/)
  assert.match(ler("src/global.d.ts"), /debridStatus: \(\) => Promise<\{ ok: boolean; configured: boolean \}>/)
  for (const rel of ["electron/main.js", "electron/preload.js", "electron/torrent.js", "src/components/desktop/MetodoDownloadDialog.tsx"]) {
    assert.ok(!ler(rel).includes("torrentSwarm"), `${rel} ainda referencia torrentSwarm`)
    assert.ok(!ler(rel).includes("torrent:swarm"), `${rel} ainda referencia torrent:swarm`)
  }
})

test("UI: diálogo bloqueia torrent sem debrid; fontes idem", () => {
  const dlg = ler("src/components/desktop/MetodoDownloadDialog.tsx")
  assert.match(dlg, /debridStatus/)
  assert.match(dlg, /disabled=\{debridOk === false\}/)
  assert.match(dlg, /store\.metodo\.torrent_debrid/)
  assert.match(dlg, /store\.fonte\.debrid_obrigatorio/)
  assert.ok(!dlg.includes("store.fonte.seeds"))
  const fon = ler("src/components/desktop/SourcesView.tsx")
  assert.match(fon, /debridStatus/)
  assert.match(fon, /fontes\.debrid_obrigatorio/)
})

test("i18n: chaves do bloqueio nas 3 línguas e seeds removido", () => {
  const dicts = {}
  for (const loc of ["pt-BR", "en-US", "es-ES"]) {
    dicts[loc] = JSON.parse(ler(`src/i18n/${loc}.json`))
    for (const k of ["store.metodo.torrent_debrid", "store.fonte.debrid_obrigatorio", "fontes.debrid_obrigatorio"]) {
      assert.ok(typeof dicts[loc][k] === "string" && dicts[loc][k].length > 0, `${loc}: ${k}`)
    }
    assert.ok(!("store.fonte.seeds" in dicts[loc]), `${loc}: store.fonte.seeds deveria ter sumido`)
  }
  const n = Object.values(dicts).map((d) => Object.keys(d).length)
  assert.equal(n[0], n[1])
  assert.equal(n[1], n[2])
})
