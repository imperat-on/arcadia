// Executa o callback real do componente, sem duplicar sua logica ou exigir DOM.
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import ts from "../ui/node_modules/typescript/lib/typescript.js"

const file = new URL("../ui/src/components/desktop/StoreView.tsx", import.meta.url)
const source = ts.createSourceFile(file.pathname, readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
let callback
let suggestCallback
function visit(node) {
  if (ts.isVariableDeclaration(node) && node.name.getText(source) === "pesquisar") callback = node.initializer.arguments[0].getText(source)
  if (ts.isCallExpression(node) && node.expression.getText(source) === "setTimeout" && node.arguments[0]?.getText(source).includes("storeSuggest")) suggestCallback = node.arguments[0].getText(source)
  ts.forEachChild(node, visit)
}
visit(source)
assert.ok(callback, "callback pesquisar encontrado no componente")
const js = ts.transpileModule(`const handler = ${callback}`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText

function fixture(search) {
  const state = { busy: false, message: "", results: null }
  const create = new Function("window", "busca", "fecharSugestoes", "gerBusca", "setBuscando", "setMsg", "setResultados", "t", "ignorarSug", "setBusca", `${js}\nreturn handler`)
  const handler = create({ launcherAPI: { storeSearch: search } }, "teste", () => {}, { current: 0 }, v => state.busy = v, v => state.message = v, v => state.results = v, k => k, { current: false }, () => {})
  return { handler, state }
}

const offline = fixture(async () => { throw new Error("offline") })
await offline.handler()
assert.equal(offline.state.busy, false)
assert.equal(offline.state.message, "store.busca_falhou")
assert.deepEqual(offline.state.results, [])

const pending = []
const racing = fixture(() => new Promise((resolve, reject) => pending.push({ resolve, reject })))
const old = racing.handler()
const latest = racing.handler()
pending[0].reject(new Error("erro antigo"))
await old
assert.equal(racing.state.busy, true, "resposta antiga nao libera a busca nova")
pending[1].resolve({ ok: true, jogos: [{ appid: "10" }] })
await latest
assert.equal(racing.state.busy, false)
assert.deepEqual(racing.state.results, [{ appid: "10" }])
assert.equal(racing.state.message, "")

assert.ok(suggestCallback)
const suggestJs = ts.transpileModule(`const handler = ${suggestCallback}`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
const suggestions = { items: ["antiga"], selected: 0 }
const suggest = new Function("window", "q", "meu", "gerSug", "setSugestoes", "setSugSel", `${suggestJs}\nreturn handler`)(
  { launcherAPI: { storeSuggest: async () => { throw new Error("offline") } } }, "teste", 1, { current: 1 }, v => suggestions.items = v, v => suggestions.selected = v,
)
await suggest()
assert.deepEqual(suggestions, { items: [], selected: -1 })
console.log("store search ok: falhas IPC de busca/sugestoes recuperaveis e resposta antiga isolada")
