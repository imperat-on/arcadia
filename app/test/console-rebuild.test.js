const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const read = name => fs.readFileSync(path.join(__dirname, '../src', name), 'utf8')

test('o quadrado do console enquadra com recorte central, sem barra nem esticamento', () => {
  const regras = [...read('index.css').matchAll(/([^{}]*\.ps5-cover-art[^{}]*)\{([^{}]*)\}/g)]
  assert.ok(regras.length >= 2)
  for (const [, , declaracoes] of regras) {
    assert.match(declaracoes, /object-fit:\s*cover/)
    assert.doesNotMatch(declaracoes, /object-fit:\s*(fill|contain)/)
    assert.match(declaracoes, /transform:\s*none/)
  }
  // A moldura do Big Picture é quadrada: é ela que define o enquadramento.
  const moldura = read('index.css').match(/\.retro-big-picture \.retro-library-cover \{[^}]*\}/)[0]
  assert.match(moldura, /aspect-ratio:1/)
})
test('console does not render navigation instructions', () => {
  assert.doesNotMatch(read('components/ps5-launcher/PS5Launcher.tsx'), /console-hints|footerNode/)
  assert.doesNotMatch(read('index.css'), /console-hints/)
})

test('console overview uses shared hours and renders complete achievement collections', () => {
  const source = read('components/ps5-launcher/GameOverview.tsx')
  assert.match(source, /horasCombinadas\(steamHoras, \[game.id, game.appid\], game.playtime_minutes\)/)
  assert.match(source, /achievements\.map\(/)
  assert.match(source, /result\?\.achievements/)
  assert.doesNotMatch(source, /formatPlaytime\(game.playtime_minutes\)/)
  assert.match(source, /data-gamepad-scroll/)
})
test('console navigation exposes filters and downloads and respects text editing', () => {
  const shell = read('components/ps5-launcher/PS5Launcher.tsx')
  const bar = read('components/ps5-launcher/TopBar.tsx')
  assert.match(bar, /onOpenDownloads/)
  assert.match(bar, /aria-pressed=\{libraryFilter===filter\}/)
  assert.match(shell, /target\?\.closest\("input, textarea, select, \[contenteditable=true\]"\)/)
  assert.match(shell, /useGamepadNav\(overviewRef, overviewNavActive, closeOverview, false, overviewNavExtras\)/)
  // A seta ↑ fecha o hub pelo mesmo caminho do B/Esc (animação inversa), mas
  // só quando o hub está no controle — não pode roubar o ↑ de um modal aberto
  // por cima dele.
  assert.match(shell, /event\.key === "Escape" \|\| event\.key === "ArrowUp"/)
  assert.match(shell, /if \(!overviewNavActive\) return/)
})
test('console hub refreshes authoritative progress and cleans IPC subscriptions', () => {
  const source = read('components/ps5-launcher/GameOverview.tsx')
  assert.match(source, /api\.onAchievementUnlocked\?\./)
  assert.match(source, /payload\.appid === appid/)
  assert.match(source, /api\.onLibraryChanged\?\./)
  assert.match(source, /api\.onSyncState\?\./)
  assert.match(source, /current !== request/)
  for (const name of ['offUnlock', 'offLibrary', 'offSync', 'offFocus']) assert.ok(source.includes(`${name}?.()`))
})
test('console nested editors suspend parents and B flushes pending profile edits', () => {
  assert.match(read('components/ps5-launcher/EditProfile.tsx'), /open && !cropSrc, \(\) => fecharRef\.current\(\)/)
  assert.match(read('components/ps5-launcher/EditMetadata.tsx'), /open && !buscando && !buscandoTexto/)
  assert.match(read('components/ps5-launcher/ProfilePage.tsx'), /embedded \|\| !navActive/)
  assert.match(read('components/ps5-launcher/AvatarCrop.tsx'), /useGamepadNav\(rootRef, isConsole, onCancel\)/)
})
test('console store remains reachable and receives the complete library', () => {
  assert.match(read('components/ps5-launcher/TopBar.tsx'), /export const TABS=.*"store.titulo"/)
  const shell = read('components/ps5-launcher/PS5Launcher.tsx')
  assert.doesNotMatch(shell, /if \(activeTab > 1\) setActiveTab\(1\)/)
  assert.match(shell, /games=\{games\}\s+bigPicture/)
})
test('console art loads before fading and reacts to same-game art updates', () => {
  const source = read('components/ps5-launcher/HeroBackground.tsx')
  assert.match(source, /image.onload = commit/)
  assert.match(source, /ticket !== generation.current/)
  assert.match(source, /\[hero, id\]/)
  assert.match(source, /prefers-reduced-motion/)
})
