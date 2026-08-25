"use client"

import { useEffect, useMemo, useState } from "react"
import type { EmulatorInfo, EmulatorRomEntry, EmulatorRomFolder, EmulatorStatus, GameSettings } from "../../global"

/**
 * Configura o perfil de emulador e a ROM do jogo. A seleção apenas persiste
 * dados; a montagem/execução do argv continua no main process.
 */
export function EmulatorProfilesPanel({
  gameId,
  settings,
  onChange,
  showProfileConfig = true,
}: {
  gameId: string
  settings: GameSettings
  onChange: (patch: Partial<GameSettings>) => void
  /** Global executable/BIOS/core editing belongs in Settings > Emulação. */
  showProfileConfig?: boolean
}) {
  const [items, setItems] = useState<EmulatorInfo[]>([])
  const [busy, setBusy] = useState(true)
  const [error, setError] = useState("")
  const [notice, setNotice] = useState("")
  const [profileBusy, setProfileBusy] = useState(false)
  const [scanBusy, setScanBusy] = useState(false)
  const [scanResults, setScanResults] = useState<EmulatorRomEntry[]>([])
  const [scanTruncated, setScanTruncated] = useState(false)
  const [romFolders, setRomFolders] = useState<EmulatorRomFolder[]>([])
  const [status, setStatus] = useState<EmulatorStatus | null>(null)
  const [biosPath, setBiosPath] = useState("")
  const [executable, setExecutable] = useState("")
  const [corePath, setCorePath] = useState(settings.emulatorCorePath || "")
  const selected = useMemo(
    () => items.find((item) => item.id === settings.emulatorId) || null,
    [items, settings.emulatorId],
  )

  const load = async (detect = false) => {
    setBusy(true)
    setError("")
    try {
      const api = window.launcherAPI
      const result = detect ? await api?.emulatorsDetect() : await api?.emulatorsList()
      if (!result?.ok) {
        setError(result?.error || "Não foi possível detectar emuladores.")
        return
      }
      setItems(result.emulators || [])
      const current = (result.emulators || []).find((item) => item.id === settings.emulatorId)
      setExecutable(current?.profile?.executable || current?.executable || "")
      setBiosPath(current?.profile?.biosPath || "")
      setRomFolders(current?.profile?.romFolders || [])
      const statuses = await api?.emulatorsStatus()
      setStatus(statuses?.statuses?.find((item) => item.emulatorId === settings.emulatorId) || null)
      const index = await api?.emulatorsRomIndex?.()
      const cached = index?.emulators?.[settings.emulatorId || ""]
      if (cached) {
        setScanResults(cached.roms || [])
        setScanTruncated(Boolean(cached.truncated))
      }
    } catch (cause) {
      setError(
        String(cause instanceof Error ? cause.message : cause || "Falha ao carregar emuladores."),
      )
    } finally {
      setBusy(false)
    }
  }

  useEffect(() => {
    void load()
    // The dialog owns persistence; reloading only when the game changes avoids
    // overwriting a path while the user is typing it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [gameId])

  useEffect(() => {
    setCorePath(settings.emulatorCorePath || "")
  }, [settings.emulatorCorePath])

  const selectEmulator = (id: string) => {
    const item = items.find((candidate) => candidate.id === id)
    setExecutable(item?.profile?.executable || item?.executable || "")
    setBiosPath(item?.profile?.biosPath || "")
    setRomFolders(item?.profile?.romFolders || [])
    setStatus(null)
    setScanResults([])
    setScanTruncated(false)
    if (id !== "retroarch") {
      setCorePath("")
      onChange({ emulatorId: id || undefined, emulatorCorePath: undefined })
    } else {
      onChange({ emulatorId: id || undefined })
    }
  }

  const pickRom = async () => {
    const result = await window.launcherAPI?.pickFile()
    if (result?.ok && result.path) onChange({ romPath: result.path })
  }

  const scanRomFolder = async () => {
    if (!selected) return
    const folder = await window.launcherAPI?.pickFolder()
    if (!folder?.ok || !folder.path) return
    setScanBusy(true)
    setError("")
    try {
      const result = await window.launcherAPI?.emulatorsRoms({
        emulatorId: selected.id,
        directory: folder.path,
        recursive: true,
        maxResults: 256,
      })
      if (!result?.ok) {
        setError(result?.error || "Não foi possível pesquisar ROMs.")
        setScanResults([])
        return
      }
      setScanResults(result.roms || [])
      setScanTruncated(Boolean(result.truncated))
      if (showProfileConfig) {
        setRomFolders((current) =>
          current.some((item) => item.path === folder.path)
            ? current
            : [...current, { path: folder.path, recursive: true }],
        )
      }
      if (!result.roms?.length) setError("Nenhuma ROM compatível encontrada nessa pasta.")
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause || "Falha ao pesquisar ROMs."))
    } finally {
      setScanBusy(false)
    }
  }

  const scanConfiguredFolders = async () => {
    if (!selected || !romFolders.length) {
      setError("Salve ao menos uma pasta de ROM no perfil.")
      return
    }
    setScanBusy(true)
    setError("")
    try {
      const result = await window.launcherAPI?.emulatorsRoms({
        emulatorId: selected.id,
        recursive: true,
        maxResults: 256,
      })
      if (!result?.ok) {
        setError(result?.error || "Não foi possível pesquisar as pastas configuradas.")
        return
      }
      setScanResults(result.roms || [])
      setScanTruncated(Boolean(result.truncated))
      setRomFolders(result.folders || romFolders)
      if (!result.roms?.length) setError("Nenhuma ROM compatível encontrada nas pastas configuradas.")
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause || "Falha ao pesquisar ROMs."))
    } finally {
      setScanBusy(false)
    }
  }

  const removeRomFolder = (folderPath: string) => {
    setRomFolders((current) => current.filter((item) => item.path !== folderPath))
  }

  const importRom = async (rom: EmulatorRomEntry) => {
    if (!selected) return
    const title = (rom.name.replace(/\.[^.]+$/, "").trim() || rom.name).slice(0, 200)
    const slug = `${selected.id}-${title}`
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 90) || `${selected.id}-rom`
    setError("")
    setNotice("")
    const check = await window.launcherAPI?.emulatorsResolve({
      emulatorId: selected.id,
      romPath: rom.path,
      extraArgs: settings.emulatorArgs || [],
      corePath: settings.emulatorCorePath || undefined,
      launchMode: "hydra",
    })
    if (!check?.ok) {
      setError(check?.error || "ROM inválida para o modo Hydra.")
      return
    }
    const result = await window.launcherAPI?.customGameAdd({
      id: `custom:${slug}`,
      title,
      platform: "emulator",
      emulatorId: selected.id,
      romPath: rom.path,
      emulatorArgs: settings.emulatorArgs || [],
      emulatorCorePath: settings.emulatorCorePath || undefined,
    })
    if (!result?.ok) {
      setError(result?.error || "Não foi possível importar a ROM.")
      return
    }
    await window.launcherAPI?.gameSettingsSet(`custom:${slug}`, {
      emulatorId: selected.id,
      romPath: rom.path,
      emulatorArgs: settings.emulatorArgs || [],
      emulatorCorePath: settings.emulatorCorePath || undefined,
    })
    setNotice(`ROM importada como “${title}”.`)
  }

  const pickCore = async () => {
    const result = await window.launcherAPI?.pickFile()
    if (result?.ok && result.path) {
      setCorePath(result.path)
      onChange({ emulatorCorePath: result.path })
    }
  }

  const pickBios = async () => {
    const result = await window.launcherAPI?.pickFolder()
    if (result?.ok && result.path) setBiosPath(result.path)
  }

  const saveProfile = async () => {
    if (!selected || !executable.trim()) {
      setError("Informe o executável do emulador.")
      return
    }
    setProfileBusy(true)
    setError("")
    try {
      const result = await window.launcherAPI?.emulatorProfileSet({
        id: selected.id,
        executable: executable.trim(),
        corePath: selected.id === "retroarch" ? corePath || undefined : undefined,
        biosPath: selected.id === "duckstation" || selected.id === "pcsx2" ? biosPath || undefined : undefined,
        romFolders,
        args: selected.profile?.args || [],
      })
      if (!result?.ok) {
        setError(result?.error || "Não foi possível salvar o perfil.")
        return
      }
      await load()
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause || "Falha ao salvar perfil."))
    } finally {
      setProfileBusy(false)
    }
  }

  const argsText = (settings.emulatorArgs || []).join(" ")
  const setArgs = (value: string) => {
    // Args remain an argv array; split on whitespace and cap at 32 entries.
    // Quotes/shell syntax is deliberately not interpreted.
    const args = value.trim() ? value.trim().split(/\s+/).slice(0, 32) : []
    onChange({ emulatorArgs: args })
  }

  return (
    <section className="flex flex-col gap-4" aria-label="Emuladores">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold text-white/85">Emulador e ROM</h3>
          <p className="mt-1 text-xs leading-relaxed text-white/45">
            PCSX2, RPCS3, Dolphin, PPSSPP, DuckStation, RetroArch e outros. O main monta o comando
            sem shell.
          </p>
        </div>
        <button
          type="button"
          onClick={() => void load(true)}
          disabled={busy}
          className="rounded-lg border border-white/10 px-3 py-1.5 text-xs text-white/65 hover:bg-white/10 disabled:opacity-40"
        >
          {busy ? "Detectando…" : "Detectar"}
        </button>
      </div>

      {error && (
        <p
          role="alert"
          className="rounded-lg border border-red-300/20 bg-red-400/10 px-3 py-2 text-xs text-red-100"
        >
          {error}
        </p>
      )}
      {notice && (
        <p className="rounded-lg border border-emerald-300/20 bg-emerald-400/10 px-3 py-2 text-xs text-emerald-100">
          {notice}
        </p>
      )}
      <label className="text-xs text-white/55">
        Emulador
        <select
          value={settings.emulatorId || ""}
          onChange={(event) => selectEmulator(event.target.value)}
          className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-3 py-2 text-sm text-white outline-none [color-scheme:dark] focus:border-[color:var(--accent)]"
          style={{ colorScheme: "dark" }}
        >
          <option value="" className="bg-[#151515] text-white">Usar comando padrão do jogo</option>
          {items.map((item) => (
            <option key={item.id} value={item.id} className="bg-[#151515] text-white">
              {item.name} · {item.systems.join(" / ")}
              {item.available ? "" : " (não detectado)"}
            </option>
          ))}
        </select>
      </label>

      {selected && !showProfileConfig && (
        <div className="rounded-xl border border-white/[0.08] bg-black/20 p-3">
          <div className="flex items-center justify-between gap-2 text-xs">
            <span className="font-medium text-white/80">{selected.name}</span>
            <span className={selected.available ? "text-emerald-200/80" : "text-amber-200/80"}>
              {selected.available ? "detectado" : "não detectado"}
            </span>
          </div>
          <p className="mt-1 text-[11px] leading-relaxed text-white/40">
            Executável, BIOS, core e pastas de ROM são configurados em <strong className="font-medium text-white/60">Configurações › Emulação</strong>.
          </p>
        </div>
      )}

      {selected && showProfileConfig && (
        <div className="rounded-xl border border-white/[0.08] bg-black/20 p-3">
          <div className="mb-2 flex items-center justify-between gap-2 text-xs">
            <span className="font-medium text-white/80">{selected.name}</span>
            <span className={selected.available ? "text-emerald-200/80" : "text-amber-200/80"}>
              {selected.available ? "disponível" : "não detectado"}
            </span>
          </div>
          <label className="text-xs text-white/50">
            Executável (nome no PATH ou caminho absoluto)
            <input
              value={executable}
              onChange={(event) => setExecutable(event.target.value)}
              maxLength={1024}
              className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 font-mono text-xs text-white outline-none focus:border-[color:var(--accent)]"
            />
          </label>
          {selected.id === "retroarch" && (
            <label className="mt-2 block text-xs text-white/50">
              Core libretro (.so)
              <div className="mt-1 flex gap-2">
                <input
                  value={corePath}
                  readOnly
                  className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 font-mono text-xs text-white/70"
                />
                <button
                  type="button"
                  onClick={() => void pickCore()}
                  className="rounded-lg border border-white/10 px-2.5 text-xs text-white/65 hover:bg-white/10"
                >
                  Escolher
                </button>
              </div>
            </label>
          )}
          {(selected.id === "duckstation" || selected.id === "pcsx2") && (
            <label className="mt-2 block text-xs text-white/50">
              Pasta do BIOS (opcional; detecção automática)
              <div className="mt-1 flex gap-2">
                <input
                  value={biosPath}
                  readOnly
                  placeholder="Pasta bios do emulador"
                  className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 font-mono text-xs text-white/70"
                />
                <button
                  type="button"
                  onClick={() => void pickBios()}
                  className="rounded-lg border border-white/10 px-2.5 text-xs text-white/65 hover:bg-white/10"
                >
                  Escolher
                </button>
              </div>
              <span className={status?.installed ? "mt-1 block text-emerald-200/70" : "mt-1 block text-amber-200/70"}>
                {status?.installed ? "BIOS detectado" : "BIOS não detectado; o lançamento será bloqueado"}
              </span>
            </label>
          )}
          {selected.id === "rpcs3" && status && (
            <p className={status.installed ? "mt-2 text-xs text-emerald-200/70" : "mt-2 text-xs text-amber-200/70"}>
              {status.installed ? "Firmware RPCS3 detectado" : "Firmware RPCS3 não detectado (configure no RPCS3)"}
            </p>
          )}
          {status?.running && (
            <p className="mt-2 text-xs text-red-200/80">
              Este emulador já está em execução{status.runningPid ? ` (PID ${status.runningPid})` : ""}; o lançamento será bloqueado.
            </p>
          )}
          <div className="mt-3 rounded-lg border border-white/[0.06] bg-black/20 p-2">
            <p className="text-[11px] text-white/45">Pastas de ROM do perfil (salve para persistir)</p>
            {romFolders.length > 0 && (
              <div className="mt-1 space-y-1">
                {romFolders.map((folder) => (
                  <div key={folder.path} className="flex items-center gap-2 text-[11px] text-white/60">
                    <span className="min-w-0 flex-1 truncate font-mono" title={folder.path}>{folder.path}</span>
                    <button
                      type="button"
                      onClick={() => removeRomFolder(folder.path)}
                      className="shrink-0 text-red-100/60 hover:text-red-100"
                      aria-label={`Remover pasta ${folder.path}`}
                    >
                      Remover
                    </button>
                  </div>
                ))}
              </div>
            )}
            <div className="mt-2 flex flex-wrap gap-2">
              <button
                type="button"
                onClick={() => void scanRomFolder()}
                disabled={scanBusy}
                className="rounded border border-white/10 px-2 py-1 text-[11px] text-white/65 hover:bg-white/10 disabled:opacity-40"
              >
                Adicionar pasta
              </button>
              <button
                type="button"
                onClick={() => void scanConfiguredFolders()}
                disabled={scanBusy || !romFolders.length}
                className="rounded border border-white/10 px-2 py-1 text-[11px] text-white/65 hover:bg-white/10 disabled:opacity-40"
              >
                {scanBusy ? "Pesquisando…" : "Pesquisar configuradas"}
              </button>
            </div>
          </div>
          <div className="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => void saveProfile()}
              disabled={profileBusy}
              className="rounded-lg bg-[color:var(--accent)] px-3 py-1.5 text-xs font-semibold text-black disabled:opacity-50"
            >
              {profileBusy ? "Salvando…" : "Salvar perfil"}
            </button>
            {selected.profile && (
              <button
                type="button"
                onClick={async () => {
                  const result = await window.launcherAPI?.emulatorProfileRemove(selected.id)
                  if (!result?.ok) setError(result?.error || "Não foi possível remover o perfil.")
                  else {
                    setExecutable(selected.executable || "")
                    await load()
                  }
                }}
                disabled={profileBusy}
                className="rounded-lg border border-red-300/20 px-3 py-1.5 text-xs text-red-100/75 hover:bg-red-400/10 disabled:opacity-50"
              >
                Remover perfil
              </button>
            )}
          </div>
        </div>
      )}

      {settings.emulatorId && (
        <>
          <label className="text-xs text-white/50">
            Arquivo da ROM/ISO
            <div className="mt-1 flex gap-2">
              <input
                value={settings.romPath || ""}
                readOnly
                placeholder="Selecione um arquivo"
                className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 font-mono text-xs text-white/70"
              />
              <button
                type="button"
                onClick={() => void pickRom()}
                className="rounded-lg border border-white/10 px-2.5 text-xs text-white/65 hover:bg-white/10"
              >
                Escolher
              </button>
              <button
                type="button"
                onClick={() => void scanRomFolder()}
                disabled={scanBusy}
                className="rounded-lg border border-white/10 px-2.5 text-xs text-white/65 hover:bg-white/10 disabled:opacity-40"
              >
                {scanBusy ? "Pesquisando…" : "Pesquisar pasta"}
              </button>
            </div>
          </label>
          {scanResults.length > 0 && (
            <div className="rounded-lg border border-white/[0.08] bg-black/20 p-2" aria-label="ROMs encontradas">
              <p className="mb-1 text-[11px] text-white/45">
                ROMs encontradas — clique para selecionar{scanTruncated ? " (limite atingido)" : ""}
              </p>
              <div className="max-h-36 space-y-1 overflow-y-auto">
                {scanResults.map((rom) => (
                  <div key={rom.path} className="flex items-center gap-1 rounded px-1 py-0.5 hover:bg-white/10">
                    <button
                      type="button"
                      onClick={() => onChange({ romPath: rom.path })}
                      className={`min-w-0 flex-1 truncate px-1 py-1 text-left text-[11px] ${
                        settings.romPath === rom.path ? "text-white" : "text-white/60 hover:text-white"
                      }`}
                      title={rom.path}
                    >
                      {rom.relativePath} · {Math.max(0, Math.round(rom.sizeBytes / 1048576))} MiB
                    </button>
                    <button
                      type="button"
                      onClick={() => void importRom(rom)}
                      className="shrink-0 rounded border border-white/10 px-1.5 py-1 text-[10px] text-white/55 hover:text-white"
                    >
                      Importar
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )}
          <label className="text-xs text-white/50">
            Argumentos adicionais (sem interpretação de shell)
            <input
              value={argsText}
              onChange={(event) => setArgs(event.target.value)}
              maxLength={4096}
              placeholder="--fullscreen"
              className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 font-mono text-xs text-white outline-none focus:border-[color:var(--accent)]"
            />
          </label>
          <p className="text-[11px] text-white/35">
            O jogo só será iniciado quando o emulador e a ROM existirem; caminhos não são enviados
            ao backend.
          </p>
        </>
      )}
    </section>
  )
}
