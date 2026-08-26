"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.join(__dirname, "..")
const read = (file) => fs.readFileSync(path.join(root, file), "utf8")

test("useGameActions concentra lançamento, escolha Steam/exe e playtime", () => {
  const source = read("src/components/useGameActions.ts")
  assert.match(source, /export function useGameActions/)
  assert.match(source, /game\.launcher === "steam" && game\.temExe/)
  assert.match(source, /onChooseLaunch/)
  assert.match(source, /launcherAPI\?\.launch\(command, gameId, mode\)/)
  assert.match(source, /disable_playtime_tracking/)
  assert.match(source, /last_played: Date\.now\(\)/)
})

test("as mutações da biblioteca dos dois launchers usam o contrato compartilhado", () => {
  const ps5 = read("src/components/ps5-launcher/PS5Launcher.tsx")
  const desktop = read("src/components/desktop/DesktopLauncher.tsx")
  const library = read("src/components/desktop/LibraryView.tsx")
  assert.match(ps5, /useGameActions/)
  assert.match(desktop, /useGameActions/)
  assert.match(desktop, /actions=\{gameActions\}/)
  assert.match(library, /actions\.saveMetadata/)
  assert.match(library, /actions\.toggleFavorite/)
  assert.match(library, /actions\.toggleHidden/)
  assert.match(library, /actions\.refresh/)
  assert.doesNotMatch(ps5, /window\.launcherAPI\?\.setOverride/)
  assert.doesNotMatch(desktop, /window\.launcherAPI\?\.setOverride/)
})

test("a preferência de inicialização é editável na configuração desktop", () => {
  const general = read("src/components/desktop/GeneralSection.tsx")
  assert.match(general, /settings\.console_mode\.label/)
  assert.match(general, /start_in_console_mode === true/)
  assert.match(general, /set\("start_in_console_mode", v\)/)
})

test("o estado compartilhado aguarda a configuração antes de liberar defaults visuais", () => {
  const state = read("src/components/useLibraryState.ts")
  const desktop = read("src/components/desktop/DesktopLauncher.tsx")
  const ps5 = read("src/components/ps5-launcher/PS5Launcher.tsx")
  assert.ok(state.indexOf("setLibraryLoaded(true)") > state.indexOf("getConfig"))
  assert.match(desktop, /if \(!libraryLoaded\) return/)
  assert.match(ps5, /if \(!libraryLoaded\) return/)
})

test("o Big Picture usa o novo overview cinematográfico", () => {
  const ps5 = read("src/components/ps5-launcher/PS5Launcher.tsx")
  const css = read("src/index.css")
  assert.equal(
    fs.existsSync(path.join(root, "src/components/ps5-launcher/GameOverview.tsx")),
    true,
  )
  assert.match(ps5, /GameOverview/)
  assert.match(ps5, /openOverview/)
  assert.match(css, /arcadia-overview__backdrop/)
  assert.match(css, /arcadiaOverviewCoverIn/)
  assert.doesNotMatch(css, /retro-game-overview|retro-detail-v2|retro-detail-scanlines|retro-detail-card/)
})

test("nenhum componente visual lê o modo estático; só o contexto consome a seed", () => {
  const srcDir = path.join(root, "src/components")
  const files = []
  const visit = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const target = path.join(dir, entry.name)
      if (entry.isDirectory()) visit(target)
      else if (/\.(tsx?|jsx?)$/.test(entry.name)) files.push(target)
    }
  }
  visit(srcDir)
  for (const file of files) {
    if (file.endsWith(path.join("components", "ModeContext.tsx"))) continue
    assert.doesNotMatch(fs.readFileSync(file, "utf8"), /window\.launcherMode/)
  }
})
