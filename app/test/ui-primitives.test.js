// Rede de segurança das fases 1-3. Estes três invariantes travam coisas que já
// quebraram uma vez de verdade:
//
//  1. var(--x) usado sem definição — a paleta foi unificada na Fase 2; um token
//     digitado errado (ou apagado do :root) pinta preto/invisível sem erro de
//     build nem de teste.
//  2. Classe arbitraria malformada — na Fase 2 uma passada de substituição
//     gerou `text-[color:var(--text-2)]]` (colchete duplo). O Tailwind IGNORA a
//     classe em silêncio: a cor simplesmente não aplica e nada avisa.
//  3. Modal fora do primitivo — havia 22 overlays feitos à mão, com
//     role="dialog" em 10, Escape em 7 e foco preso em 2. O src/ui/Modal.tsx
//     passa a ser o único lugar; a lista de dívida abaixo só pode encolher.

import { test } from "node:test"
import assert from "node:assert/strict"
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

const aqui = path.dirname(fileURLToPath(import.meta.url))
const APP = path.resolve(aqui, "..")
const SRC = path.join(APP, "src")

/** Ainda montam o próprio diálogo. Migre e remova daqui; NUNCA acrescente. */
const DIVIDA_DIALOGOS = [
  "components/UpdateDialog.tsx",
  "components/desktop/AchievementsFullScreen.tsx",
  "components/desktop/EmulationSection.tsx",
  "components/desktop/EmulatorInstallDialog.tsx",
  "components/desktop/RetroStoreView.tsx",
  "components/ps5-launcher/ArtSearch.tsx",
  "components/ps5-launcher/EditMetadata.tsx",
  "components/ps5-launcher/GameOverview.tsx",
  "components/ps5-launcher/PS5Launcher.tsx",
  "components/ps5-launcher/TextSearch.tsx",
]

/** Escreve var na raiz em runtime (aplicarA11y), então não vive no :root. */
const RUNTIME = ["--bg", "--sidebar-bg", "--card-bg", "--text"]

function arquivosFonte() {
  const saida = []
  const anda = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name)
      if (e.isDirectory()) anda(p)
      else if (/\.(ts|tsx|css)$/.test(e.name)) saida.push(p)
    }
  }
  anda(SRC)
  return saida
}

test("todo var(--token) usado tem definição", () => {
  const fontes = arquivosFonte()
  const definidos = new Set(RUNTIME)
  const usados = new Map()

  for (const f of fontes) {
    const texto = fs.readFileSync(f, "utf-8")
    const rel = path.relative(SRC, f)
    // definição: `--x:` no CSS, ou setProperty("--x" em qualquer arquivo
    for (const m of texto.matchAll(/(^|[\s;{])(--[a-z0-9-]+)\s*:/gi)) definidos.add(m[2])
    for (const m of texto.matchAll(/setProperty\(\s*"(--[a-z0-9-]+)"/g)) definidos.add(m[1])
    // uso: var(--x) em qualquer lugar
    for (const m of texto.matchAll(/var\(\s*(--[a-z0-9-]+)/g)) {
      if (!usados.has(m[1])) usados.set(m[1], rel)
    }
  }

  const orfaos = [...usados].filter(([tok]) => !definidos.has(tok))
  assert.deepEqual(
    orfaos,
    [],
    `tokens usados sem definição: ${orfaos.map(([t, f]) => `${t} (${f})`).join(", ")}`,
  )
})

test("nenhuma classe arbitrária com colchete duplo", () => {
  const ruins = []
  for (const f of arquivosFonte()) {
    const rel = path.relative(SRC, f)
    fs.readFileSync(f, "utf-8")
      .split("\n")
      .forEach((linha, i) => {
        // var(--x)]] ou var(--x)/40]] — o Tailwind ignora a classe em silêncio
        if (/var\(--[a-z0-9-]+\)(\/\d+)?\]\]/.test(linha)) ruins.push(`${rel}:${i + 1}`)
      })
  }
  assert.deepEqual(ruins, [], `classe arbitrária malformada em: ${ruins.join(", ")}`)
})

test("diálogo novo não pode fugir do src/ui/Modal.tsx", () => {
  const foraDoPrimitivo = []
  for (const f of arquivosFonte()) {
    if (!f.endsWith(".tsx")) continue
    const rel = path.relative(SRC, f)
    if (rel === path.join("ui", "Modal.tsx")) continue
    const texto = fs.readFileSync(f, "utf-8")
    if (texto.includes('role="dialog"') && !/ui\/Modal/.test(texto)) foraDoPrimitivo.push(rel)
  }
  const novos = foraDoPrimitivo.filter((f) => !DIVIDA_DIALOGOS.includes(f))
  assert.deepEqual(
    novos,
    [],
    `diálogo montado à mão fora do primitivo: ${novos.join(", ")} — use src/ui/Modal.tsx`,
  )

  // Os pilotos da Fase 3 precisam continuar delegando (é o que prova que a
  // migração não voltou atrás em algum rebase).
  for (const piloto of ["components/desktop/LaunchModeDialog.tsx", "components/ps5-launcher/ConsoleDestinoDialog.tsx"]) {
    const texto = fs.readFileSync(path.join(SRC, piloto), "utf-8")
    assert.match(texto, /ui\/Modal/, `${piloto} deveria usar o primitivo`)
    assert.doesNotMatch(texto, /fixed inset-0/, `${piloto} não deveria montar overlay próprio`)
  }
})
