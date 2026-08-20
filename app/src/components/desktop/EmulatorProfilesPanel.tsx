"use client"

import { useEffect, useMemo, useState } from "react"
import type { EmulatorInfo, GameSettings } from "../../global"

/**
 * Configura o perfil de emulador e a ROM do jogo. A seleção apenas persiste
 * dados; a montagem/execução do argv continua no main process.
 */
export function EmulatorProfilesPanel({
  gameId,
  settings,
  onChange,
}: {
  gameId: string
  settings: GameSettings
  onChange: (patch: Partial<GameSettings>) => void
}) {
  const [items, setItems] = useState<EmulatorInfo[]>([])
  const [busy, setBusy] = useState(true)
  const [error, setError] = useState("")
  const [profileBusy, setProfileBusy] = useState(false)
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

  const pickCore = async () => {
    const result = await window.launcherAPI?.pickFile()
    if (result?.ok && result.path) {
      setCorePath(result.path)
      onChange({ emulatorCorePath: result.path })
    }
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
      <label className="text-xs text-white/55">
        Emulador
        <select
          value={settings.emulatorId || ""}
          onChange={(event) => selectEmulator(event.target.value)}
          className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-3 py-2 text-sm text-white outline-none focus:border-[color:var(--accent)]"
        >
          <option value="">Usar comando padrão do jogo</option>
          {items.map((item) => (
            <option key={item.id} value={item.id}>
              {item.name} · {item.systems.join(" / ")}
              {item.available ? "" : " (não detectado)"}
            </option>
          ))}
        </select>
      </label>

      {selected && (
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
            </div>
          </label>
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
