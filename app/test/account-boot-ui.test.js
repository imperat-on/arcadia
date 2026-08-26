"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

test("boot da conta tem timeout e não mantém o launcher preto indefinidamente", () => {
  const source = fs.readFileSync(path.join(__dirname, "..", "src", "components", "account", "AccountContext.tsx"), "utf8")
  assert.match(source, /withTimeout\(window\.launcherAPI\?\.accountStatus\(\), 12_000\)/)
  assert.match(source, /withTimeout\(window\.launcherAPI\?\.accountProfile\(\), 10_000\)/)
  assert.match(source, /setStatus\(r\?\.session \? "logado" : "deslogado"\)/)
})
