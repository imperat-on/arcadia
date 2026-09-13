"use client"

import { useEffect, useMemo, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
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
  const { t } = useI18n()
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
        setError(result?.error || t("emulador.erro_detectar"))
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
        String(cause instanceof Error ? cause.message : cause || t("emulador.erro_carregar")),
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
        setError(result?.error || t("emulador.erro_pesquisar"))
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
      if (!result.roms?.length) setError(t("emulador.nenhuma_rom_pasta"))
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause || t("emulador.erro_pesquisar")))
    } finally {
      setScanBusy(false)
    }
  }

  const scanConfiguredFolders = async () => {
    if (!selected || !romFolders.length) {
      setError(t("emulador.salve_pasta"))
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
        setError(result?.error || t("emulador.erro_pesquisar_pastas"))
        return
      }
      setScanResults(result.roms || [])
      setScanTruncated(Boolean(result.truncated))
      setRomFolders(result.folders || romFolders)
      if (!result.roms?.length) setError(t("emulacao.nenhuma_rom_pastas"))
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause || t("emulador.erro_pesquisar")))
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
      setError(check?.error || t("emulador.rom_invalida_hydra"))
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
      setError(result?.error || t("emulador.erro_importar"))
      return
    }
    await window.launcherAPI?.gameSettingsSet(`custom:${slug}`, {
      emulatorId: selected.id,
      romPath: rom.path,
      emulatorArgs: settings.emulatorArgs || [],
      emulatorCorePath: settings.emulatorCorePath || undefined,
    })
    setNotice(t("emulador.rom_importada", { title }))
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
      setError(t("emulador.informe_executavel"))
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
        setError(result?.error || t("emulador.erro_salvar_perfil"))
        return
      }
      await load()
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause || t("emulador.falha_salvar_perfil")))
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
    <section className="flex flex-col gap-4" aria-label={t("emulador.aria")}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold text-white/85">{t("emulador.titulo")}</h3>
          <p className="mt-1 text-xs leading-relaxed text-white/45">
            {t("emulador.desc")}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void load(true)}
          disabled={busy}
          className="rounded-lg border border-white/10 px-3 py-1.5 text-xs text-white/65 hover:bg-white/10 disabled:opacity-40"
        >
          {busy ? t("common.detectando") : t("emulador.detectar")}
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
          <option value="" className="bg-[color:var(--surface-2)] text-white">{t("emulador.usar_comando_padrao")}</option>
          {items.map((item) => (
            <option key={item.id} value={item.id} className="bg-[color:var(--surface-2)] text-white">
              {item.name} · {item.systems.join(" / ")}
              {item.available ? "" : t("emulador.nao_detectado_sufixo")}
            </option>
          ))}
        </select>
      </label>

      {selected && !showProfileConfig && (
        <div className="rounded-xl border border-white/[0.08] bg-black/20 p-3">
          <div className="flex items-center justify-between gap-2 text-xs">
            <span className="font-medium text-white/80">{selected.name}</span>
            <span className={selected.available ? "text-emerald-200/80" : "text-amber-200/80"}>
              {selected.available ? t("emulador.detectado") : t("emulador.nao_detectado")}
            </span>
          </div>
          <p className="mt-1 text-[11px] leading-relaxed text-white/40">
            {t("emulador.configurados_em")}
          </p>
        </div>
      )}

      {selected && showProfileConfig && (
        <div className="rounded-xl border border-white/[0.08] bg-black/20 p-3">
          <div className="mb-2 flex items-center justify-between gap-2 text-xs">
            <span className="font-medium text-white/80">{selected.name}</span>
            <span className={selected.available ? "text-emerald-200/80" : "text-amber-200/80"}>
              {selected.available ? t("emulador.disponivel") : t("emulador.nao_detectado")}
            </span>
          </div>
          <label className="text-xs text-white/50">
            {t("emulador.executavel_label")}
            <input
              value={executable}
              onChange={(event) => setExecutable(event.target.value)}
              maxLength={1024}
              className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 font-mono text-xs text-white outline-none focus:border-[color:var(--accent)]"
            />
          </label>
          {selected.id === "retroarch" && (
            <label className="mt-2 block text-xs text-white/50">
              {t("emulador.core_libretro_so")}
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
                  {t("emulador.escolher")}
                </button>
              </div>
            </label>
          )}
          {(selected.id === "duckstation" || selected.id === "pcsx2") && (
            <label className="mt-2 block text-xs text-white/50">
              {t("emulador.pasta_bios_opcional")}
              <div className="mt-1 flex gap-2">
                <input
                  value={biosPath}
                  readOnly
                  placeholder={t("emulador.pasta_bios_placeholder")}
                  className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 font-mono text-xs text-white/70"
                />
                <button
                  type="button"
                  onClick={() => void pickBios()}
                  className="rounded-lg border border-white/10 px-2.5 text-xs text-white/65 hover:bg-white/10"
                >
                  {t("emulador.escolher")}
                </button>
              </div>
              <span className={status?.installed ? "mt-1 block text-emerald-200/70" : "mt-1 block text-amber-200/70"}>
                {status?.installed ? t("emulador.bios_detectado") : t("emulador.bios_nao_detectado")}
              </span>
            </label>
          )}
          {selected.id === "rpcs3" && status && (
            <p className={status.installed ? "mt-2 text-xs text-emerald-200/70" : "mt-2 text-xs text-amber-200/70"}>
              {status.installed ? t("emulador.firmware_detectado") : t("emulador.firmware_nao_detectado")}
            </p>
          )}
          {status?.running && (
            <p className="mt-2 text-xs text-red-200/80">
              {t("emulador.rodando_bloqueado", { pid: status.runningPid ? ` (PID ${status.runningPid})` : "" })}
            </p>
          )}
          <div className="mt-3 rounded-lg border border-white/[0.06] bg-black/20 p-2">
            <p className="text-[11px] text-white/45">{t("emulador.pastas_rom_perfil")}</p>
            {romFolders.length > 0 && (
              <div className="mt-1 space-y-1">
                {romFolders.map((folder) => (
                  <div key={folder.path} className="flex items-center gap-2 text-[11px] text-white/60">
                    <span className="min-w-0 flex-1 truncate font-mono" title={folder.path}>{folder.path}</span>
                    <button
                      type="button"
                      onClick={() => removeRomFolder(folder.path)}
                      className="shrink-0 text-red-100/60 hover:text-red-100"
                      aria-label={t("emulador.remover_pasta_aria", { pasta: folder.path })}
                    >
                      {t("common.remover")}
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
                {t("emulador.adicionar_pasta")}
              </button>
              <button
                type="button"
                onClick={() => void scanConfiguredFolders()}
                disabled={scanBusy || !romFolders.length}
                className="rounded border border-white/10 px-2 py-1 text-[11px] text-white/65 hover:bg-white/10 disabled:opacity-40"
              >
                {scanBusy ? t("common.pesquisando") : t("emulador.pesquisar_configuradas")}
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
              {profileBusy ? t("common.salvando") : t("emulador.salvar_perfil")}
            </button>
            {selected.profile && (
              <button
                type="button"
                onClick={async () => {
                  const result = await window.launcherAPI?.emulatorProfileRemove(selected.id)
                  if (!result?.ok) setError(result?.error || t("emulador.erro_remover_perfil"))
                  else {
                    setExecutable(selected.executable || "")
                    await load()
                  }
                }}
                disabled={profileBusy}
                className="rounded-lg border border-red-300/20 px-3 py-1.5 text-xs text-red-100/75 hover:bg-red-400/10 disabled:opacity-50"
              >
                {t("emulador.remover_perfil")}
              </button>
            )}
          </div>
        </div>
      )}

      {settings.emulatorId && (
        <>
          <label className="text-xs text-white/50">
            {t("emulador.arquivo_rom")}
            <div className="mt-1 flex gap-2">
              <input
                value={settings.romPath || ""}
                readOnly
                placeholder={t("emulador.selecione_arquivo")}
                className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 font-mono text-xs text-white/70"
              />
              <button
                type="button"
                onClick={() => void pickRom()}
                className="rounded-lg border border-white/10 px-2.5 text-xs text-white/65 hover:bg-white/10"
              >
                {t("emulador.escolher")}
              </button>
              <button
                type="button"
                onClick={() => void scanRomFolder()}
                disabled={scanBusy}
                className="rounded-lg border border-white/10 px-2.5 text-xs text-white/65 hover:bg-white/10 disabled:opacity-40"
              >
                {scanBusy ? t("common.pesquisando") : t("emulador.pesquisar_pasta")}
              </button>
            </div>
          </label>
          {scanResults.length > 0 && (
            <div className="rounded-lg border border-white/[0.08] bg-black/20 p-2" aria-label={t("emulacao.roms_encontradas")}>
              <p className="mb-1 text-[11px] text-white/45">
                {t("emulador.roms_clique")}{scanTruncated ? t("emulador.limite_atingido") : ""}
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
                      {t("emulador.importar")}
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )}
          <label className="text-xs text-white/50">
            {t("emulador.argumentos")}
            <input
              value={argsText}
              onChange={(event) => setArgs(event.target.value)}
              maxLength={4096}
              placeholder="--fullscreen"
              className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 font-mono text-xs text-white outline-none focus:border-[color:var(--accent)]"
            />
          </label>
          <p className="text-[11px] text-white/35">
            {t("emulador.aviso_inicio")}
          </p>
        </>
      )}
    </section>
  )
}
