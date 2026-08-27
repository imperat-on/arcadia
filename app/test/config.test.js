"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const path = require("node:path")
const { spawnSync } = require("node:child_process")

const configPath = path.resolve(__dirname, "../electron/supabase/config.js")

function loadConfig(env) {
  const result = spawnSync(process.execPath, ["-e", `console.log(JSON.stringify(require(${JSON.stringify(configPath)})))`], {
    env: { ...process.env, ...env },
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

test("instalação sem variáveis usa a API oficial publicada", () => {
  const config = loadConfig({
    ARCADIA_API_URL: "",
    ARCADIA_SUPABASE_URL: "",
    SUPABASE_URL: "",
  })
  assert.equal(config.url, "https://zes.tail6e748d.ts.net")
})
