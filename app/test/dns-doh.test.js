"use strict"

// Catraca do DNS resolvido pelo próprio app.
//
// O `net.fetch` do Electron (base do httpfetch) resolve nomes pela pilha do
// Chromium, que por padrão segue o resolvedor do sistema. Isso já custou caro
// duas vezes nesta base: primeiro uma busca de loja de 3,4s por conexão porque o
// DNS do roteador estava lento, e depois um perfil inteiro sem imagem porque o
// resolvedor da operadora passou a devolver só AAAA (IPv6) para o backend numa
// máquina sem IPv6. Com DoH o app não depende do DNS de quem usa.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const MAIN = path.resolve(__dirname, "..", "electron", "main.js")

test("o processo principal resolve DNS por DoH", () => {
  const fonte = fs.readFileSync(MAIN, "utf8")
  assert.match(fonte, /appendSwitch\(\s*"dns-over-https-mode"\s*,\s*"secure"\s*\)/)
  assert.match(
    fonte,
    /appendSwitch\(\s*"dns-over-https-templates"\s*,\s*"https:\/\/cloudflare-dns\.com\/dns-query"\s*\)/,
  )
  // Escape para rede que bloqueia DoH (portal cativo): documentado e explícito.
  assert.match(fonte, /ARCADIA_NO_DOH/)
})

test("os switches de DNS são definidos antes do app ficar pronto", () => {
  // Switch de linha de comando só vale antes de app.whenReady(); depois disso o
  // Chromium já leu a configuração e o DoH não tem efeito.
  const fonte = fs.readFileSync(MAIN, "utf8")
  const posSwitch = fonte.indexOf("dns-over-https-mode")
  const posReady = fonte.indexOf("app.whenReady")
  assert.ok(posSwitch > 0, "switch de DoH ausente")
  assert.ok(posReady > 0, "app.whenReady ausente")
  assert.ok(posSwitch < posReady, "switch de DoH depois de app.whenReady não faz nada")
})
