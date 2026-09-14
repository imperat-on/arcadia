const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const read = (name) => fs.readFileSync(path.join(__dirname, '../', name), 'utf8')

// O boot.mp4 não pode depender de file:// (bloqueado quando a página roda em
// http:// no dev/preview) nem de um arquivo externo ao pacote. O main entrega os
// bytes do asset e o renderer monta um blob URL com autoplay garantido (muted).
test('boot video is served by the main process and played from a blob URL', () => {
  const main = read('electron/main.js')
  const preload = read('electron/preload.js')
  const boot = read('src/components/ps5-launcher/BootScreen.tsx')

  assert.match(main, /ipcMain\.handle\(\s*"boot:video"/)
  assert.match(main, /BOOT_VIDEO,\s*BUNDLED_BOOT_VIDEO/)
  assert.match(preload, /bootVideo:\s*\(\)\s*=>\s*ipcRenderer\.invoke\(\s*"boot:video"\s*\)/)

  assert.match(boot, /api\?\.bootVideo|api\.bootVideo/)
  assert.match(boot, /URL\.createObjectURL/)
  assert.match(boot, /new Blob\(/)
  assert.match(boot, /\bmuted\b/)
  assert.match(boot, /\bautoPlay\b/)
})
