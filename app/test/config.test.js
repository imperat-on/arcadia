"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const path = require("node:path")
const { spawnSync } = require("node:child_process")

const configPath = path.resolve(__dirname, "../electron/supabase/config.js")

function loadConfig(env) {
  const childEnv = { ...process.env, ...env }
  delete childEnv.NODE_TEST_CONTEXT
  const result = spawnSync(process.execPath, ["-e", `console.log(JSON.stringify(require(${JSON.stringify(configPath)})))`], {
    env: childEnv,
    encoding: "utf8",
  })
  assert.equal(result.status, 0, result.stderr)
  return JSON.parse(result.stdout)
}

test("ARCADIA_API_URL é canônico e normaliza barras finais", () => {
  const config = loadConfig({
    ARCADIA_API_URL: " https://api.example.test/// ",
    ARCADIA_SUPABASE_URL: "https://legacy.example.test",
    SUPABASE_URL: "https://older.example.test",
  })
  assert.equal(config.url, "https://api.example.test")
})

test("nomes legados continuam funcionando quando o URL canônico não existe", () => {
  const config = loadConfig({
    ARCADIA_API_URL: "",
    ARCADIA_SUPABASE_URL: "https://legacy.example.test/",
    SUPABASE_URL: "https://older.example.test",
  })
  assert.equal(config.url, "https://legacy.example.test")
})

test("instalação sem variáveis usa a API oficial publicada, com reserva", () => {
  const config = loadConfig({
    ARCADIA_API_URL: "",
    ARCADIA_SUPABASE_URL: "",
    SUPABASE_URL: "",
  })
  // Endereço público = Worker do Cloudflare (IPv4 e IPv6, não depende do DNS de
  // quem usa); o Funnel do Tailscale fica como reserva, tentada só se a rede
  // falhar (ver electron/httpfetch.js).
  assert.equal(config.url, "https://arcadiaserver.zesmehentperu.workers.dev")
  assert.deepEqual(config.urls, [
    "https://arcadiaserver.zesmehentperu.workers.dev",
    "https://zes.tail6e748d.ts.net",
  ])
})

test("com servidor próprio não existe reserva para o backend oficial", () => {
  const config = loadConfig({ ARCADIA_API_URL: "https://meu.servidor.test" })
  assert.equal(config.url, "https://meu.servidor.test")
  assert.deepEqual(config.urls, ["https://meu.servidor.test"])
})
