"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { EventEmitter } = require("node:events")
const { createTrailerService } = require("../electron/trailer-service")

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-trailer-"))
  const trailersDir = path.join(root, "trailers")
  const ytdlpPath = path.join(root, "yt-dlp")
  const cookiesPath = path.join(root, "cookies.txt")
  fs.writeFileSync(ytdlpPath, "fake")
  fs.writeFileSync(cookiesPath, "# Netscape HTTP Cookie File")
  return { root, trailersDir, ytdlpPath, cookiesPath }
}

function outputPath(args) {
  const template = args[args.indexOf("-o") + 1]
  return template.replace("%(ext)s", "mp4")
}

test("localPath usa extensões conhecidas e nunca atravessa trailersDir", () => {
  const f = fixture()
  const service = createTrailerService({ ...f })
  fs.mkdirSync(f.trailersDir, { recursive: true })
  fs.writeFileSync(path.join(f.trailersDir, "steam_10.webm"), "video")
  assert.equal(service.localPath("steam:10"), path.join(f.trailersDir, "steam_10.webm"))
  fs.writeFileSync(path.join(f.trailersDir, "steam_10.mp4"), "video")
  assert.equal(service.localPath("steam:10"), path.join(f.trailersDir, "steam_10.mp4"))
  assert.equal(service.localPath("../../outside"), "")
  assert.ok(service.localPath("../../outside").startsWith(f.trailersDir) || service.localPath("../../outside") === "")
  fs.rmSync(f.root, { recursive: true, force: true })
})

test("download respeita cache, deduplica jobs e limpa parciais", async () => {
  const f = fixture()
  const calls = []
  let release
  const gate = new Promise((resolve) => { release = resolve })
  const service = createTrailerService({
    ...f,
    ffmpegDir: "/usr/bin",
    getCookiesPath: () => f.cookiesPath,
    env: { PATH: "/fake" },
    execFileImpl: (command, args, options, callback) => {
      calls.push({ command, args, options })
      gate.then(() => {
        fs.mkdirSync(f.trailersDir, { recursive: true })
        fs.writeFileSync(outputPath(args), "video")
        callback(Object.assign(new Error("yt-dlp returned 1"), { code: 1 }))
      })
    },
  })
  fs.mkdirSync(f.trailersDir, { recursive: true })
  fs.writeFileSync(path.join(f.trailersDir, "steam_10.part"), "partial")
  const first = service.download("steam:10", "Portal 2")
  const second = service.download("steam:10", "Portal 2")
  assert.strictEqual(first, second)
  release()
  assert.deepEqual(await first, { ok: true, path: path.join(f.trailersDir, "steam_10.mp4") })
  assert.equal(calls.length, 1)
  assert.ok(calls[0].args.includes("--no-playlist"))
  assert.ok(calls[0].args.includes("--ffmpeg-location"))
  assert.ok(calls[0].args.includes("--cookies"))
  assert.equal(calls[0].options.env.PATH, "/fake")
  const cached = await service.download("steam:10", "Portal 2")
  assert.deepEqual(cached, await first)
  assert.equal(calls.length, 1)
  fs.rmSync(f.root, { recursive: true, force: true })
})

test("search ignora linhas inválidas e reporta falha sem resultados", async () => {
  const f = fixture()
  const logs = []
  let fail = false
  const service = createTrailerService({
    ...f,
    logger: (message) => logs.push(message),
    execFileImpl: (_command, _args, _options, callback) => {
      if (fail) return callback(Object.assign(new Error("network"), { code: "ETIMEDOUT" }), "", "ERROR: timeout")
      callback(
        null,
        [
          "not json",
          JSON.stringify({ id: "abc", title: "Trailer", duration: 42, uploader: "Canal", thumbnails: [{ url: "thumb" }] }),
        ].join("\n"),
        "",
      )
    },
  })
  const result = await service.search("Portal")
  assert.deepEqual(result.results, [{
    id: "abc",
    url: "https://www.youtube.com/watch?v=abc",
    title: "Trailer",
    duration: 42,
    channel: "Canal",
    thumbnail: "thumb",
  }])
  fail = true
  assert.deepEqual(await service.search("Portal"), { results: [], error: "ERROR: timeout" })
  assert.equal(logs.length, 2)
  fs.rmSync(f.root, { recursive: true, force: true })
})

test("streamUrl e downloadUrl expõem resultado e progresso sem Electron", async () => {
  const f = fixture()
  const calls = []
  const service = createTrailerService({
    ...f,
    execFileImpl: (_command, args, _options, callback) => {
      calls.push(args)
      callback(null, "https://cdn.example/video.mp4\n", "")
    },
    spawnImpl: (_command, args) => {
      calls.push(args)
      const child = new EventEmitter()
      child.stdout = new EventEmitter()
      child.stderr = new EventEmitter()
      setImmediate(() => {
        child.stdout.emit("data", Buffer.from("[download] 42.5% of 10MiB\n"))
        child.stderr.emit("data", Buffer.from("[VideoRemuxer] Merging\n"))
        fs.mkdirSync(f.trailersDir, { recursive: true })
        fs.writeFileSync(outputPath(args), "video")
        child.emit("close", 0)
      })
      return child
    },
  })
  assert.deepEqual(await service.streamUrl("https://www.youtube.com/watch?v=abc"), {
    ok: true,
    url: "https://cdn.example/video.mp4",
  })
  const progress = []
  const downloaded = await service.downloadUrl("steam:10", "https://www.youtube.com/watch?v=abc", {
    onProgress: (event) => progress.push(event),
  })
  assert.equal(downloaded.ok, true)
  assert.deepEqual(progress.map((event) => event.stage), ["download", "processando", "done"])
  assert.equal(progress[0].id, "steam:10")
  assert.ok(calls.length >= 2)
  fs.rmSync(f.root, { recursive: true, force: true })
})
