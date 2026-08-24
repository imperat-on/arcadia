"use strict"

const fsDefault = require("node:fs")
const path = require("node:path")
const { execFile: execFileDefault, spawn: spawnDefault } = require("node:child_process")

const EXTENSIONS = [".mp4", ".webm", ".mkv"]

function safeName(id) {
  return String(id).replace(/[^a-z0-9._-]/gi, "_")
}

/**
 * Serviço de trailers sem dependência de Electron.
 *
 * A factory recebe filesystem/processos/logger para que o comportamento possa
 * ser testado sem yt-dlp, rede ou uma janela real. O main process continua
 * dono de cookies e apenas adapta os resultados para os canais IPC.
 */
function createTrailerService({
  trailersDir,
  ytdlpPath,
  ffmpegDir = "",
  env = process.env,
  getCookiesPath = () => "",
  execFileImpl = execFileDefault,
  spawnImpl = spawnDefault,
  fsImpl = fsDefault,
  logger = () => {},
} = {}) {
  if (!trailersDir) throw new Error("trailersDir é obrigatório")
  const trailerJobs = new Map()
  const ffmpegArgs = ffmpegDir ? ["--ffmpeg-location", ffmpegDir] : []
  const commandEnv = { ...env }

  function isAvailable() {
    return Boolean(ytdlpPath) && fsImpl.existsSync(ytdlpPath)
  }

  function localPath(id) {
    const base = path.join(trailersDir, safeName(id))
    for (const ext of EXTENSIONS) {
      if (fsImpl.existsSync(base + ext)) return base + ext
    }
    return ""
  }

  function clearFiles(safe) {
    try {
      for (const file of fsImpl.readdirSync(trailersDir)) {
        if (file === safe || file.startsWith(safe + ".")) {
          try {
            fsImpl.unlinkSync(path.join(trailersDir, file))
          } catch {
            // Uma tentativa concorrente pode já ter removido o parcial.
          }
        }
      }
    } catch {
      // A pasta ainda não existe.
    }
  }

  function cookieArgs() {
    try {
      const cookies = String(getCookiesPath() || "").trim()
      return cookies && fsImpl.existsSync(cookies) ? ["--cookies", cookies] : []
    } catch {
      return []
    }
  }

  function download(id, title) {
    const existing = localPath(id)
    if (existing) return Promise.resolve({ ok: true, path: existing })
    if (!isAvailable()) return Promise.resolve({ ok: false, error: "yt-dlp ausente" })
    if (trailerJobs.has(id)) return trailerJobs.get(id)

    const job = new Promise((resolve) => {
      fsImpl.mkdirSync(trailersDir, { recursive: true })
      const safe = safeName(id)
      clearFiles(safe)
      const args = [
        `ytsearch5:${String(title || "")} trailer`,
        "--no-playlist",
        "--no-warnings",
        "--no-continue",
        "--no-part",
        "--match-filter",
        "duration > 20 & duration < 360",
        "-f",
        "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b",
        "--remux-video",
        "mp4",
        ...ffmpegArgs,
        ...cookieArgs(),
        "-o",
        path.join(trailersDir, `${safe}.%(ext)s`),
      ]
      execFileImpl(ytdlpPath, args, { timeout: 180000, env: commandEnv }, (error) => {
        const result = localPath(id)
        if (result) return resolve({ ok: true, path: result })
        if (error && error.code === "ENOENT") {
          return resolve({ ok: false, error: "yt-dlp não instalado (instale o pacote yt-dlp)" })
        }
        resolve({ ok: false, error: "trailer não encontrado" })
      })
    }).finally(() => trailerJobs.delete(id))

    trailerJobs.set(id, job)
    return job
  }

  function streamUrl(url) {
    if (!url || !isAvailable()) return Promise.resolve({ ok: false, error: "pedido inválido" })
    return new Promise((resolve) => {
      execFileImpl(
        ytdlpPath,
        [
          "-g",
          "-f",
          "best[height<=720][ext=mp4]/22/18/best[ext=mp4]/best",
          "--no-warnings",
          ...cookieArgs(),
          url,
        ],
        { timeout: 40000, maxBuffer: 1024 * 1024 * 4, env: commandEnv },
        (error, stdout, stderr) => {
          const link = String(stdout || "")
            .split("\n")
            .find((line) => line.startsWith("http"))
          if (link) return resolve({ ok: true, url: link })
          const age = /confirm your age|inappropriate/i.test(String(stderr || ""))
          resolve({ ok: false, error: age ? "age" : "sem stream" })
        },
      )
    })
  }

  function search(query) {
    if (!isAvailable()) {
      logger("busca abortada: yt-dlp não instalado")
      return Promise.resolve({ results: [], error: "yt-dlp não está instalado — instale o pacote yt-dlp" })
    }
    return new Promise((resolve) => {
      const args = [`ytsearch12:${String(query || "")} trailer`, "--flat-playlist", "--dump-json", "--no-warnings"]
      execFileImpl(
        ytdlpPath,
        args,
        { timeout: 40000, maxBuffer: 1024 * 1024 * 8, env: commandEnv },
        (error, stdout, stderr) => {
          const results = []
          for (const line of String(stdout || "").split("\n")) {
            if (!line.trim()) continue
            try {
              const data = JSON.parse(line)
              const thumbnails = data.thumbnails || []
              results.push({
                id: data.id,
                url: data.url || `https://www.youtube.com/watch?v=${data.id}`,
                title: data.title || "",
                duration: data.duration || 0,
                channel: data.channel || data.uploader || "",
                thumbnail: data.thumbnail || (thumbnails.length ? thumbnails[thumbnails.length - 1].url : ""),
              })
            } catch {
              // yt-dlp pode misturar avisos no stdout; ignora a linha.
            }
          }
          if (!results.length && error) {
            const message =
              String(stderr || "")
                .split("\n")
                .filter((line) => /error/i.test(line))[0] ||
              (error.code === "ENOENT" ? "yt-dlp não encontrado" : `yt-dlp falhou (${error.code ?? error.message})`)
            logger(`busca "${query}": ${message}`)
            return resolve({ results: [], error: message })
          }
          logger(`busca "${query}": ${results.length} resultado(s)`)
          resolve({ results })
        },
      )
    })
  }

  function downloadUrl(id, url, { onProgress = () => {} } = {}) {
    if (!id || !url || !isAvailable()) return Promise.resolve({ ok: false, error: "pedido inválido" })
    return new Promise((resolve) => {
      fsImpl.mkdirSync(trailersDir, { recursive: true })
      const safe = safeName(id)
      clearFiles(safe)
      const args = [
        url,
        "--no-playlist",
        "--no-warnings",
        "--no-continue",
        "--no-part",
        "--newline",
        "-f",
        "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b",
        "--remux-video",
        "mp4",
        ...ffmpegArgs,
        ...cookieArgs(),
        "-o",
        path.join(trailersDir, `${safe}.%(ext)s`),
      ]
      const emit = (data) => {
        try {
          onProgress({ id, ...data })
        } catch {
          // Atualização de UI nunca pode interromper o download.
        }
      }
      let errorBuffer = ""
      const child = spawnImpl(ytdlpPath, args, { env: commandEnv })
      const onData = (buffer) => {
        const output = buffer.toString()
        const match = output.match(/\[download\]\s+([0-9.]+)%/)
        if (match) emit({ percent: parseFloat(match[1]), stage: "download" })
        if (/\[VideoRemuxer\]|Merging/.test(output)) emit({ percent: 100, stage: "processando" })
      }
      child.stdout.on("data", onData)
      child.stderr.on("data", (buffer) => {
        errorBuffer += buffer.toString()
        onData(buffer)
      })
      child.on("close", () => {
        const result = localPath(id)
        emit({ percent: 100, stage: "done" })
        if (result) return resolve({ ok: true, path: result })
        if (/confirm your age|inappropriate/i.test(errorBuffer)) {
          return resolve({ ok: false, error: "age" })
        }
        const message =
          errorBuffer
            .split("\n")
            .reverse()
            .find((line) => /error|ffmpeg/i.test(line)) || ""
        resolve({ ok: false, error: message.trim() || "falha ao baixar" })
      })
      child.on("error", (error) => resolve({ ok: false, error: String(error.message || error) }))
    })
  }

  return { isAvailable, localPath, download, search, streamUrl, downloadUrl }
}

module.exports = { createTrailerService, safeName }
