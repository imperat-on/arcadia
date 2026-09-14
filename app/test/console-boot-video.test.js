const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
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

// A cópia em DATA_DIR vencia o asset (COPYFILE_EXCL, só se ausente): uma cópia
// STALE de versão anterior travava o conserto de codec (ex.: VP9/Opus →
// H.264/AAC) para sempre. O main compara tamanho/md5 e sobrescreve.
test('boot video refreshes a stale DATA_DIR copy (compare-and-copy)', () => {
  const mod = require('../electron/bootVideo.js')

  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'arcadia-boot-'))
  const dataDir = path.join(root, 'data', 'nested') // mkdir -p coberto pelo módulo
  const asset = path.join(root, 'bundled.mp4')

  // 1) sem cópia: escreve.
  fs.writeFileSync(asset, 'abcabc')
  const r1 = mod.refreshBootVideo({ dataDir, bundled: asset })
  assert.ok(r1.codecs.includes('avc1') && r1.codecs.includes('mp4a'), 'expõe o codec do asset')
  assert.deepEqual(fs.readFileSync(path.join(dataDir, 'boot.mp4')), Buffer.from('abcabc'))

  // 2) cópia STALE de tamanho diferente: sobrescreve.
  fs.writeFileSync(path.join(dataDir, 'boot.mp4'), Buffer.from('antiga e diferente'))
  mod.refreshBootVideo({ dataDir, bundled: asset })
  assert.deepEqual(fs.readFileSync(path.join(dataDir, 'boot.mp4')), Buffer.from('abcabc'))

  // 3) mesmo tamanho, conteúdo diferente: md5 pega e sobrescreve.
  fs.writeFileSync(asset + '.outro', 'xxxxxx')
  fs.writeFileSync(path.join(dataDir, 'boot.mp4'), Buffer.from('xxxxxx'))
  mod.refreshBootVideo({ dataDir, bundled: asset + '.outro' })
  assert.deepEqual(fs.readFileSync(path.join(dataDir, 'boot.mp4')), Buffer.from('xxxxxx'))

  // 4) cópia idêntica: não reescreve (mtime preservado).
  fs.utimesSync(path.join(dataDir, 'boot.mp4'), new Date(1e6), new Date(1e6))
  mod.refreshBootVideo({ dataDir, bundled: asset + '.outro' })
  assert.equal(fs.statSync(path.join(dataDir, 'boot.mp4')).mtime.getTime(), 1e6, 'cópia idêntica não é reescrita')

  // 5) sem asset empacotado: no-op e não lança.
  assert.doesNotThrow(() => mod.refreshBootVideo({ dataDir, bundled: path.join(root, 'nada.mp4') }))
  // 6) DATA_DIR não é diretório: lança EEXIST/ENOTDIR — o chamador (main) só
  // warna; nunca derruba o app.
  const arquivoNoLugar = path.join(root, 'arquivo')
  fs.writeFileSync(arquivoNoLugar, 'nao e dir')
  assert.throws(() => mod.refreshBootVideo({ dataDir: arquivoNoLugar, bundled: asset }), /EEXIST|ENOTDIR/)
})

test('main wires boot:video with refresh + spills the codec string', () => {
  const main = read('electron/main.js')
  assert.match(main, /refreshBootVideo\(\{\s*dataDir:\s*DATA_DIR,\s*bundled:\s*BUNDLED_BOOT_VIDEO\s*\}\)/)
  assert.match(main, /BOOT_VIDEO_CODECS/)
  assert.match(main, /require\("\.\/bootVideo"\)/)
})

// Se o ENGINE do renderer não tocar o codec do asset (canPlayType "" e nada de
// fallback), o boot pula via onError — nunca preto-para-sempre.
test('boot screen checks canPlayType and skips boot on unsupported codec', () => {
  const boot = read('src/components/ps5-launcher/BootScreen.tsx')
  assert.match(boot, /canPlayType/)
  assert.match(boot, /tocaCodec/)
  assert.match(boot, /avisarFalha\(\)/)
})

test('main keeps stale-copy refresh cheap and failure-tolerant', () => {
  const main = read('electron/main.js')
  // não derruba o app: try/catch + warn
  assert.match(main, /console\.warn\(`\[arcadia:boot\]/)
})
