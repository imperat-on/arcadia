"use strict"

// Navegação do webview da loja Steam: a página de terceiros navega só dentro
// de store.steampowered.com; links de comunidade/loja abrem no navegador do
// sistema e qualquer outro destino é bloqueado sem abrir nada.
//
// O handler é EXTRAÍDO do main.js e executado com shell falso — o teste cobra
// comportamento, não texto. Antes desta correção um clique em "Community Hub"
// era apenas engolido pelo preventDefault.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const vm = require("node:vm")

const root = path.join(__dirname, "..")
const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")

function carregarWebview() {
  const start = main.indexOf('app.on("web-contents-created"')
  const end = main.indexOf('app.on("activate"', start)
  assert.ok(start >= 0 && end > start, "handler de web-contents-created presente no main")

  const abertos = []
  let registrar = null
  vm.runInNewContext(main.slice(start, end), {
    app: { on: (evento, handler) => { if (evento === "web-contents-created") registrar = handler } },
    shell: { openExternal: (url) => { abertos.push(url) } },
    URL,
  })
  assert.equal(typeof registrar, "function", "main registra o handler de web-contents-created")

  const listeners = []
  let windowOpen = null
  const contents = {
    getType: () => "webview",
    on: (evento, handler) => { if (evento === "will-navigate") listeners.push(handler) },
    setWindowOpenHandler: (handler) => { windowOpen = handler },
  }

  let registrado = false
  const garantirRegistro = () => {
    if (registrado) return
    registrado = true
    registrar(null, contents)
  }

  return {
    abertos,
    contents,
    registrar,
    disparar(url) {
      garantirRegistro()
      assert.equal(listeners.length, 1, "webview registra will-navigate")
      let prevented = false
      listeners[0]({ preventDefault: () => { prevented = true } }, url)
      return prevented
    },
    popup(url) {
      garantirRegistro()
      assert.equal(typeof windowOpen, "function", "webview registra setWindowOpenHandler")
      return windowOpen({ url })
    },
  }
}

test("webview ignora contents que não são webview", () => {
  const wv = carregarWebview()
  const listeners = []
  wv.contents.getType = () => "window"
  wv.contents.on = (evento, handler) => { if (evento === "will-navigate") listeners.push(handler) }
  wv.contents.setWindowOpenHandler = () => {
    throw new Error("não deveria registrar handler fora do webview")
  }
  wv.registrar(null, wv.contents)
  assert.deepEqual(listeners, [], "nenhuma regra aplicada fora do webview")
})

test("navegação interna da loja continua permitida", () => {
  const wv = carregarWebview()
  assert.equal(wv.disparar("https://store.steampowered.com/app/10/"), false)
  assert.equal(wv.disparar("http://store.steampowered.com/app/10/"), true, "só HTTPS")
  assert.deepEqual(wv.abertos, [])
})

test("links da comunidade abrem no navegador do sistema", () => {
  const wv = carregarWebview()
  const url = "https://steamcommunity.com/app/10/discussions/"
  assert.equal(wv.disparar(url), true, "sai da webview")
  assert.deepEqual(wv.abertos, [url], "abriu externamente em vez de engolir o clique")
})

test("destino desconhecido é bloqueado sem abrir nada", () => {
  const wv = carregarWebview()
  assert.equal(wv.disparar("https://evil.example/phish"), true)
  assert.deepEqual(wv.abertos, [])
})

test("URL com credenciais embutidas nunca chega ao shell", () => {
  const wv = carregarWebview()
  assert.equal(wv.disparar("https://user:senha@steamcommunity.com/"), true)
  assert.deepEqual(wv.abertos, [])
})

test("popup https é negado dentro do app e aberto no sistema", () => {
  const wv = carregarWebview()
  const url = "https://store.steampowered.com/app/10/"
  // O objeto vem do contexto do vm (outro realm): comparar campo a campo.
  assert.equal(wv.popup(url).action, "deny")
  assert.deepEqual(wv.abertos, [url])
  assert.equal(wv.popup("http://inseguro.example/").action, "deny")
  assert.deepEqual(wv.abertos, [url], "http puro não abre")
})
