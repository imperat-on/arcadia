import test from "node:test"
import assert from "node:assert/strict"
import vm from "node:vm"
import { fileURLToPath } from "node:url"
import { build } from "../ui/node_modules/esbuild/lib/main.js"

const bundle = await build({ entryPoints: [fileURLToPath(new URL("../ui/src/components/desktop/retroPageCache.ts", import.meta.url))], bundle: true, write: false, format: "iife", globalName: "RetroCache", platform: "browser" })
const params = { offset: 0, limit: 24, mode: "all" }
const game = index => ({ id: `retro:ps2:${index}`, title: `Fixture ${index}`, cover: `https://images.launchbox-app.com/fixture-${index}.jpg` })

function harness(fetchPage, brokenImages = false) {
  let time = 0
  const images = []
  const timers = new Map()
  let timerId = 0
  const context = vm.createContext({
    URL,
    Date: { now: () => time },
    window: { launcherAPI: { retroList: fetchPage } },
    Image: class {
      constructor() {
        if (brokenImages) throw new Error("image unavailable")
        images.push(this)
      }
    },
    setTimeout: callback => { timers.set(++timerId, callback); return timerId },
    clearTimeout: id => timers.delete(id),
  })
  vm.runInContext(bundle.outputFiles[0].text, context)
  return { cache: context.RetroCache, images, timers, advance: ms => time += ms }
}

test("paginas Retro sao imediatas no cache, deduplicadas e isoladas por conta", async () => {
  let calls = 0
  const page = { ok: true, games: [game(1)], total: 1, hasMore: false }
  const h = harness(async () => { calls++; return page })
  const results = await Promise.all(Array.from({ length: 20 }, () => h.cache.loadRetroPage(params, "user-a")))
  assert.equal(calls, 1)
  assert.ok(results.every(result => result === page))
  assert.equal(h.cache.getCachedRetroPage(params, "user-a"), page)
  assert.equal(await h.cache.loadRetroPage(params, "user-a"), page)
  assert.equal(calls, 1)
  assert.equal(h.cache.getCachedRetroPage(params, "user-b"), undefined)
  await h.cache.loadRetroPage(params, "user-b")
  assert.equal(calls, 2)
  h.advance(60_000)
  assert.equal(h.cache.getCachedRetroPage(params, "user-a"), undefined)
  await h.cache.loadRetroPage(params, "user-a")
  assert.equal(calls, 3)
})

test("cache diferencia pagina/filtro e limita o numero de paginas guardadas", async () => {
  const h = harness(async payload => ({ ok: true, games: [game(payload.offset)] }))
  for (let i = 0; i < 25; i++) await h.cache.loadRetroPage({ ...params, offset: i * 24 }, "user")
  assert.equal(h.cache.getCachedRetroPage(params, "user"), undefined)
  assert.ok(h.cache.getCachedRetroPage({ ...params, offset: 24 * 24 }, "user"))
  assert.equal(h.cache.getCachedRetroPage({ ...params, offset: 24 * 24, system: "sony-playstation" }, "user"), undefined)
})

test("pagina conhecida fica disponivel durante revalidacao, sem cache eterno", async () => {
  let calls = 0
  let finish
  const first = { ok: true, games: [game(1)] }
  const next = { ok: true, games: [game(2)] }
  const h = harness(() => ++calls === 1 ? Promise.resolve(first) : new Promise(resolve => { finish = resolve }))
  await h.cache.loadRetroPage(params, "user")
  h.advance(60_000)
  assert.equal(h.cache.getCachedRetroPage(params, "user"), undefined)
  assert.equal(h.cache.getCachedRetroPage(params, "user", true), first)
  const refreshing = h.cache.loadRetroPage(params, "user")
  await Promise.resolve()
  assert.equal(h.cache.getCachedRetroPage(params, "user", true), first)
  finish(next)
  await refreshing
  assert.equal(h.cache.getCachedRetroPage(params, "user"), next)
  h.advance(10 * 60_000)
  assert.equal(h.cache.getCachedRetroPage(params, "user", true), undefined)
})

test("falhas nao ficam em cache e uma rejeicao pode ser tentada novamente", async () => {
  let calls = 0
  const h = harness(async () => {
    calls++
    if (calls === 1) throw new Error("offline")
    if (calls === 2) return { ok: false, error: "offline" }
    return { ok: true, games: [] }
  })
  const failed = await Promise.allSettled([h.cache.loadRetroPage(params, "user"), h.cache.loadRetroPage(params, "user")])
  assert.ok(failed.every(result => result.status === "rejected"))
  assert.equal(calls, 1)
  await h.cache.loadRetroPage(params, "user")
  assert.equal(h.cache.getCachedRetroPage(params, "user"), undefined)
  await h.cache.loadRetroPage(params, "user")
  assert.ok(h.cache.getCachedRetroPage(params, "user"))
  assert.equal(calls, 3)
})

test("preload nao bloqueia a pagina: aquece so oito capas com quatro em paralelo", async () => {
  const page = { ok: true, games: Array.from({ length: 24 }, (_, index) => game(index)) }
  const h = harness(async () => page)
  assert.equal(await h.cache.loadRetroPage(params, "user"), page)
  assert.equal(h.images.length, 4)
  assert.ok(h.images.every(image => image.fetchPriority === "low"))
  assert.equal(h.timers.size, 4)
  h.cache.preloadRetroCovers(page.games)
  assert.equal(h.images.length, 4, "mesmas URLs em voo nao sao repetidas")
  for (let i = 0; i < 8; i++) h.images[i].onload()
  assert.equal(h.images.length, 8)
  assert.equal(h.timers.size, 0)
  h.cache.preloadRetroCovers(page.games)
  assert.equal(h.images.length, 8, "capas aquecidas nao sao pedidas de novo")
})

test("fila e limitada, timeout libera vagas e URLs privadas nao sao antecipadas", () => {
  const h = harness(async () => ({ ok: true, games: [] }))
  for (let i = 0; i < 20; i++) h.cache.preloadRetroCovers(Array.from({ length: 8 }, (_, index) => game(i * 8 + index)))
  assert.equal(h.images.length, 4)
  for (let i = 0; i < 16; i++) h.images[i].onload()
  assert.equal(h.images.length, 16, "no maximo 16 tarefas, nao o catalogo inteiro")
  h.cache.preloadRetroCovers([{ ...game(1000), cover: "https://127.0.0.1/private" }])
  assert.equal(h.images.length, 16)
  h.cache.preloadRetroCovers([game(1001)])
  h.timers.values().next().value()
  h.cache.preloadRetroCovers([game(1001)])
  assert.equal(h.images.length, 18, "capa que falhou pode ser tentada outra vez")
})

test("indisponibilidade de Image nao transforma catalogo valido em erro", async () => {
  const page = { ok: true, games: [game(1)] }
  const h = harness(async () => page, true)
  assert.equal(await h.cache.loadRetroPage(params, "user"), page)
  assert.equal(h.cache.getCachedRetroPage(params, "user"), page)
})
