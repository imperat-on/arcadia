"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const {
  buildExternalGamescopeCommand,
  canUseSystemdSession,
  createSystemdUnitName,
  isSystemdUnitName,
  parseSystemdShow,
  readCgroupPids,
  systemdStopArgs,
} = require("../electron/gamescope-session")

const bins = (name) => name === "gamescope" || name === "systemd-run" || name === "systemctl"

test("monta Gamescope persistente com serviço systemd como primário", () => {
  const result = buildExternalGamescopeCommand(
    ["proton", "run", "/tmp/plutonium.exe"],
    {
      width: 2560,
      height: 1600,
      fps: 60,
      keepAlive: true,
      systemdUnit: "arcadia-game-1234-9.service",
      environmentKeys: ["DISPLAY", "DISPLAY", "BAD-KEY"],
    },
  )
  assert.deepEqual(result.cmd, [
    "gamescope",
    "-W",
    "2560",
    "-H",
    "1600",
    "-r",
    "60",
    "--keep-alive",
    "--",
    "systemd-run",
    "--user",
    "--setenv=DISPLAY",
    "--unit",
    "arcadia-game-1234-9.service",
    "--property=RemainAfterExit=yes",
    "--property=TimeoutStopSec=5s",
    "--wait",
    "--pipe",
    "--collect",
    "--",
    "proton",
    "run",
    "/tmp/plutonium.exe",
  ])
  assert.deepEqual(result.processSession, {
    type: "systemd",
    unit: "arcadia-game-1234-9.service",
    cgroupRoot: "/sys/fs/cgroup",
  })
})

test("mantém comando direto quando não há sessão systemd", () => {
  const result = buildExternalGamescopeCommand(["game"], { keepAlive: false })
  assert.deepEqual(result.cmd, ["gamescope", "-W", "1920", "-H", "1080", "--", "game"])
  assert.equal(result.processSession, null)
})

test("capability exige Linux, binários e bus da sessão", () => {
  const fsImpl = { existsSync: (file) => file === "/run/user/1000/bus" }
  assert.equal(
    canUseSystemdSession({
      platform: "linux",
      binExists: bins,
      env: { XDG_RUNTIME_DIR: "/run/user/1000" },
      fsImpl,
    }),
    true,
  )
  assert.equal(
    canUseSystemdSession({
      platform: "linux",
      binExists: bins,
      env: {},
      fsImpl,
    }),
    false,
  )
  assert.equal(
    canUseSystemdSession({
      platform: "win32",
      binExists: bins,
      env: { DBUS_SESSION_BUS_ADDRESS: "unix:path=/tmp/bus" },
      fsImpl,
    }),
    false,
  )
})

test("unidade da sessão é determinística e não aceita entrada arbitrária", () => {
  assert.equal(createSystemdUnitName({ pid: 1234, tokenId: 9 }), "arcadia-game-1234-9.service")
  assert.equal(isSystemdUnitName("arcadia-game-1234-9.service"), true)
  assert.equal(isSystemdUnitName("arcadia-game-1234-9.scope"), false)
  assert.deepEqual(systemdStopArgs("arcadia-game-1234-9.service"), [
    "--user",
    "stop",
    "--no-block",
    "arcadia-game-1234-9.service",
  ])
  assert.equal(systemdStopArgs("other.service"), null)
})

test("lê propriedades e PIDs somente dentro do cgroup informado", () => {
  assert.deepEqual(parseSystemdShow("ActiveState=active\nControlGroup=/user.slice/x.service\n"), {
    ActiveState: "active",
    ControlGroup: "/user.slice/x.service",
  })
  const files = {
    "/sys/fs/cgroup/user.slice/x.service/cgroup.procs": "101\n202\ninvalid\n",
  }
  const fsImpl = {
    readFileSync(file) {
      if (!(file in files)) throw new Error("not found")
      return files[file]
    },
  }
  assert.deepEqual(
    readCgroupPids("/user.slice/x.service", { fsImpl }),
    [101, 202],
  )
  assert.equal(readCgroupPids("/user.slice/../other", { fsImpl }), null)
})

test("monta HDR, limitador e modo de janela antes do separador do comando", () => {
  const result = buildExternalGamescopeCommand(["proton", "run", "/tmp/game.exe"], {
    width: 2560,
    height: 1600,
    fps: 60,
    framerateLimit: 45,
    hdr: true,
    windowMode: "fullscreen",
    keepAlive: true,
  })
  assert.deepEqual(result.cmd, [
    "gamescope",
    "-W",
    "2560",
    "-H",
    "1600",
    "-r",
    "60",
    "--framerate-limit",
    "45",
    "--hdr-enabled",
    "-f",
    "--keep-alive",
    "--",
    "proton",
    "run",
    "/tmp/game.exe",
  ])
})

test("modo de janela usa exatamente um flag e windowed não adiciona flag", () => {
  for (const [windowMode, expected] of [
    ["fullscreen", "-f"],
    ["borderless", "-b"],
    ["windowed", null],
    ["valor-invalido", null],
  ]) {
    const cmd = buildExternalGamescopeCommand(["game"], { windowMode }).cmd
    const modeFlags = cmd.filter((value) => value === "-f" || value === "-b")
    assert.equal(modeFlags.length, expected ? 1 : 0, windowMode)
    if (expected) assert.equal(modeFlags[0], expected)
  }
})

test("opções novas são opcionais e não alteram o contrato legado", () => {
  const result = buildExternalGamescopeCommand(["game"], {
    width: 1280,
    height: 720,
    fps: 30,
    hdr: false,
    framerateLimit: 0,
    windowMode: "windowed",
  })
  assert.deepEqual(result.cmd, ["gamescope", "-W", "1280", "-H", "720", "-r", "30", "--", "game"])
})

test("valores inválidos não viram argumentos Gamescope", () => {
  const result = buildExternalGamescopeCommand(["game"], {
    width: "2560 --hdr-enabled",
    height: -1,
    fps: "60;evil",
    framerateLimit: "45 --keep-alive",
    hdr: "true",
    windowMode: "--evil",
  })
  assert.deepEqual(result.cmd, ["gamescope", "-W", "1920", "-H", "1080", "--", "game"])
})
