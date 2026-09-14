"use strict"

// Vídeo de boot: refresh da cópia em DATA_DIR e codec "universal" que o main
// entrega ao renderer (para o canPlayType do BootScreen).
//
// A cópia em DATA_DIR vencia o asset empacotado e era escrita uma única vez
// (COPYFILE_EXCL) — uma cópia STALE de uma versão anterior (codec antigo, ex.:
// VP9+Opus em MP4) travava o conserto para sempre. O refresh compara tamanho e
// depois md5 e sobrescreve quando o asset da versão mudou. Barato no boot
// (uma leitura única de ~3 MB) e nunca derruba o app: qualquer falha cai no
// warn do chamador, e o BootScreen segue o caminho "sem vídeo" dele.
const fs = require("node:fs")
const path = require("node:path")
const crypto = require("node:crypto")

/** Codec: H.264 High + AAC-LC em MP4 — suportado por todos os Chromium/Electron. */
const BOOT_VIDEO_CODECS = 'video/mp4; codecs="avc1.640028, mp4a.40.2"'

function md5(dado) {
  return crypto.createHash("md5").update(dado).digest("hex")
}

/**
 * Garante a cópia atual do boot.mp4 em DATA_DIR. Retorna { codecs } mesmo sem
 * escrever nada, e { ok:false } nunca: erros são do chamador (try/catch).
 */
function refreshBootVideo({ dataDir, bundled }) {
  if (!bundled || !fs.existsSync(bundled)) return { codecs: BOOT_VIDEO_CODECS }
  fs.mkdirSync(dataDir, { recursive: true })
  const destino = path.join(dataDir, "boot.mp4")
  const novinho = fs.readFileSync(bundled)
  let desatualizado = true
  try {
    const atual = fs.readFileSync(destino)
    desatualizado = atual.length !== novinho.length || md5(atual) !== md5(novinho)
  } catch {
    /* sem cópia legível (ausente ou corrompida): escreve direto */
  }
  if (desatualizado) fs.writeFileSync(destino, novinho)
  return { codecs: BOOT_VIDEO_CODECS }
}

module.exports = { refreshBootVideo, BOOT_VIDEO_CODECS }
