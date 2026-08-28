"use strict"

// DepotDownloader é um runtime externo e não deve ser versionado nem
// embutido no AppImage. O Arcadia usa a variante compatível com os manifestos
// locais (`-depotkeys` e `-manifestfile`); a release atual do projeto oficial
// removeu esses parâmetros, por isso a versão é fixada nesta release do fork.
const fs = require("node:fs")
const path = require("node:path")
const crypto = require("node:crypto")
const { execFile: execFileDefault } = require("node:child_process")
const { fetchRede: fetchDefault } = require("./httpfetch")

const DEPOT_RELEASE_URL =
  "https://github.com/bedrockuser/DepotDownloaderMod/releases/download/release-2025_06_17/DepotDownloader-framework.zip"
// Hash do arquivo acima. Além de evitar baixar uma release incompatível, isto
// impede que um arquivo trocado no host seja extraído como executável.
const DEPOT_RELEASE_SHA256 = "e49a8eec7f09cd9ebcf8890e1aaedced787741634f7d1380d8f57558a920238e"
const MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
const DOWNLOAD_TIMEOUT_MS = 120_000
const RELEASE_HOST_PREFIX = "https://github.com/bedrockuser/DepotDownloaderMod/releases/download/"

// O zip framework é publicado com o nome do projeto (`DepotDownloaderMod`),
// enquanto o Arcadia mantém o nome histórico `DepotDownloader` no comando.
const RENAMES = [
  ["DepotDownloaderMod.dll", "DepotDownloader.dll"],
  ["DepotDownloaderMod.deps.json", "DepotDownloader.deps.json"],
  ["DepotDownloaderMod.runtimeconfig.json", "DepotDownloader.runtimeconfig.json"],
]

// Não basta criar um DLL vazio: o host .NET precisa dos metadados e de todas
// as bibliotecas laterais para carregar o programa.
const REQUIRED_FILES = [
  "DepotDownloader.dll",
  "DepotDownloader.deps.json",
  "DepotDownloader.runtimeconfig.json",
  "QRCoder.dll",
  "SteamKit2.dll",
  "System.IO.Hashing.dll",
  "ZstdSharp.dll",
  "protobuf-net.Core.dll",
  "protobuf-net.dll",
]

function erroTexto(error) {
  return String(error?.message || error || "erro desconhecido")
}

function arquivoRegular(fsImpl, file) {
  try {
    const stat = fsImpl.lstatSync(file)
    return stat.isFile() && !stat.isSymbolicLink() && stat.size > 0
  } catch {
    return false
  }
}

function executarExtracao(execFileImpl, archive, target) {
  return new Promise((resolve, reject) => {
    execFileImpl("python3", ["-m", "zipfile", "-e", archive, target], (error, _stdout, stderr) => {
      if (!error) return resolve()
      const detalhe = String(stderr || "").trim()
      reject(new Error(detalhe || erroTexto(error)))
    })
  })
}

function createDepotDownloaderManager({
  depsDir,
  tmpDir,
  fsImpl = fs,
  fetchImpl = fetchDefault,
  execFileImpl = execFileDefault,
  archiveUrl = DEPOT_RELEASE_URL,
  archiveSha256 = DEPOT_RELEASE_SHA256,
  now = () => Date.now(),
  pid = process.pid,
} = {}) {
  if (!depsDir || !tmpDir) throw new Error("depsDir e tmpDir são obrigatórios")

  const dllPath = path.join(depsDir, "DepotDownloader.dll")
  let inFlight = null

  function installed() {
    return REQUIRED_FILES.every((name) => arquivoRegular(fsImpl, path.join(depsDir, name)))
  }

  async function download() {
    if (installed()) return { ok: true, path: dllPath }

    let archivePath = ""
    let stageDir = ""
    try {
      const url = String(archiveUrl || "")
      if (!url.startsWith(RELEASE_HOST_PREFIX)) {
        return { ok: false, error: "DepotDownloader: URL de release não permitida" }
      }

      fsImpl.mkdirSync(tmpDir, { recursive: true })
      const response = await fetchImpl(url, {
        headers: {
          "User-Agent": "arcadia",
          Accept: "application/octet-stream",
        },
        signal: AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS),
      })
      if (!response?.ok) {
        return { ok: false, error: `DepotDownloader: download HTTP ${response?.status || "?"}` }
      }

      const bytes = Buffer.from(await response.arrayBuffer())
      if (bytes.length < 4 || bytes[0] !== 0x50 || bytes[1] !== 0x4b) {
        return { ok: false, error: "DepotDownloader: resposta não é um zip válido" }
      }
      if (bytes.length > MAX_ARCHIVE_BYTES) {
        return { ok: false, error: "DepotDownloader: arquivo de release grande demais" }
      }
      if (archiveSha256) {
        const digest = crypto.createHash("sha256").update(bytes).digest("hex")
        if (digest !== String(archiveSha256).toLowerCase()) {
          return { ok: false, error: "DepotDownloader: hash da release não confere" }
        }
      }

      const suffix = `${pid}-${now()}-${Math.random().toString(16).slice(2)}`
      archivePath = path.join(tmpDir, `.depotdownloader-${suffix}.zip`)
      stageDir = path.join(tmpDir, `.depotdownloader-stage-${suffix}`)
      fsImpl.writeFileSync(archivePath, bytes, { mode: 0o600 })
      fsImpl.mkdirSync(stageDir, { recursive: true })
      await executarExtracao(execFileImpl, archivePath, stageDir)

      for (const [from, to] of RENAMES) {
        const source = path.join(stageDir, from)
        const target = path.join(stageDir, to)
        if (!arquivoRegular(fsImpl, target) && arquivoRegular(fsImpl, source))
          fsImpl.renameSync(source, target)
      }
      const faltando = REQUIRED_FILES.filter(
        (name) => !arquivoRegular(fsImpl, path.join(stageDir, name)),
      )
      if (faltando.length) {
        return { ok: false, error: `DepotDownloader: release incompleta (${faltando.join(", ")})` }
      }

      // Outro processo pode ter terminado a instalação enquanto este baixava.
      // Nesse caso não substitui uma cópia válida.
      if (installed()) return { ok: true, path: dllPath }
      fsImpl.mkdirSync(path.dirname(depsDir), { recursive: true })
      fsImpl.rmSync(depsDir, { recursive: true, force: true })
      fsImpl.renameSync(stageDir, depsDir)
      stageDir = ""
      if (!installed()) return { ok: false, error: "DepotDownloader: instalação incompleta" }
      return { ok: true, path: dllPath }
    } catch (error) {
      return { ok: false, error: `DepotDownloader: ${erroTexto(error)}` }
    } finally {
      if (archivePath) {
        try {
          fsImpl.rmSync(archivePath, { force: true })
        } catch {}
      }
      if (stageDir) {
        try {
          fsImpl.rmSync(stageDir, { recursive: true, force: true })
        } catch {}
      }
    }
  }

  // Duas chamadas simultâneas (por exemplo, fila + tela de configurações)
  // compartilham o mesmo download e não corrompem o diretório final.
  function ensure() {
    if (installed()) return Promise.resolve({ ok: true, path: dllPath })
    if (inFlight) return inFlight
    inFlight = download().finally(() => {
      inFlight = null
    })
    return inFlight
  }

  return { installed, ensure, dllPath }
}

module.exports = {
  createDepotDownloaderManager,
  DEPOT_RELEASE_URL,
  DEPOT_RELEASE_SHA256,
  REQUIRED_FILES,
}
