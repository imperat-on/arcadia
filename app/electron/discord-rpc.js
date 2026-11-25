const net = require("net")
const os = require("os")

const DEFAULT_CLIENT_ID = "1537346240579248219"

// Discord RPC usa um socket local e um protocolo simples com frames JSON.
// Sem client id ou Discord aberto, todos os métodos são silenciosos.
class DiscordRpc {
  constructor(readConfig) {
    this.readConfig = readConfig
    this.socket = null
    this.connected = false
    this.pending = []
    this.retry = null
    this.buffer = Buffer.alloc(0)
  }

  clientId() {
    const cfg = this.readConfig() || {}
    return String(
      process.env.ARCADIA_DISCORD_CLIENT_ID || cfg.discord_client_id || DEFAULT_CLIENT_ID,
    ).trim()
  }

  enabled() {
    return (this.readConfig() || {}).discord_rich_presence === true && Boolean(this.clientId())
  }

  socketPath() {
    if (process.platform === "win32") return `\\\\?\\pipe\\discord-ipc-0`
    return `/run/user/${process.getuid?.() ?? os.userInfo().uid}/discord-ipc-0`
  }

  socketPaths() {
    if (process.platform === "win32") return [this.socketPath()]
    const uid = process.getuid?.() ?? os.userInfo().uid
    const runtime = process.env.XDG_RUNTIME_DIR || `/run/user/${uid}`
    const paths = Array.from({ length: 10 }, (_, i) => `${runtime}/discord-ipc-${i}`)
    // Flatpak Vesktop keeps its IPC socket in its private xdg-run mount.
    const flatpakRuntime = `${runtime}/.flatpak/dev.vencord.Vesktop/xdg-run`
    paths.push(...Array.from({ length: 10 }, (_, i) => `${flatpakRuntime}/discord-ipc-${i}`))
    return paths
  }

  frame(opcode, data) {
    const body = Buffer.from(JSON.stringify(data))
    const out = Buffer.alloc(8 + body.length)
    out.writeUInt32LE(opcode, 0)
    out.writeUInt32LE(body.length, 4)
    body.copy(out, 8)
    return out
  }

  connect() {
    if (!this.enabled() || this.connected || this.socket) return
    const paths = this.socketPaths()
    this.connectPath(paths[0], paths.slice(1))
  }

  connectPath(socketPath, paths) {
    if (this.socket) return
    const socket = net.createConnection(socketPath)
    socket._arcadiaPath = socketPath
    socket._arcadiaPaths = paths
    this.socket = socket
    socket.once("connect", () => {
      socket.write(this.frame(0, { v: 1, client_id: this.clientId() }))
    })
    socket.on("data", (data) => this.handleData(socket, data))
    socket.on("error", () => {
      const next = socket._arcadiaPaths?.[0]
      if (next) {
        socket._arcadiaPaths.shift()
        try { socket.destroy() } catch {}
        this.socket = null
        this.connectPath(next, socket._arcadiaPaths)
      } else this.reset(socket)
    })
    socket.on("close", () => {
      this.reset(socket)
      if (this.pending.length && !this.retry) {
        this.retry = setInterval(() => {
          if (!this.enabled()) return this.stopRetry()
          this.connect()
          if (this.connected) this.stopRetry()
        }, 5000)
      }
    })
  }

  handleData(socket, data) {
    if (this.socket !== socket) return
    this.buffer = Buffer.concat([this.buffer, data])
    while (this.buffer.length >= 8) {
      const size = this.buffer.readUInt32LE(4)
      if (this.buffer.length < 8 + size) break
      const opcode = this.buffer.readUInt32LE(0)
      this.buffer = this.buffer.subarray(8 + size)
      if (opcode === 1) {
        this.connected = true
        for (const activity of this.pending.splice(0)) this.sendActivity(activity)
      }
    }
  }

  reset(socket) {
    if (this.socket !== socket) return
    this.socket = null
    this.connected = false
    this.buffer = Buffer.alloc(0)
  }

  stopRetry() {
    if (this.retry) clearInterval(this.retry)
    this.retry = null
  }

  sendActivity(activity) {
    if (!this.connected || !this.socket) return false
    try {
      this.socket.write(this.frame(1, {
        cmd: "SET_ACTIVITY",
        nonce: String(Date.now()),
        args: { pid: process.pid, activity },
      }))
      return true
    } catch {
      return false
    }
  }

  setGame(title, launcher) {
    if (!this.enabled()) return
    const activity = {
      details: String(title || "Jogando"),
      state: `Arcadia | ${String(launcher || "jogo")}`,
      timestamps: { start: Math.floor(Date.now() / 1000) },
    }
    if (this.connected) this.sendActivity(activity)
    else {
      this.pending = [activity]
      this.connect()
    }
  }

  clear() {
    this.pending = []
    this.stopRetry()
    if (this.connected) this.sendActivity(null)
  }

  close() {
    this.pending = []
    this.stopRetry()
    try { this.socket?.destroy() } catch {}
    this.socket = null
    this.connected = false
  }
}

module.exports = DiscordRpc
