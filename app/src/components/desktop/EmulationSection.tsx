"use client"

import { useEffect, useMemo, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { EmulatorInstallDialog } from "./EmulatorInstallDialog"
import { RetroAchievementsPanel } from "./RetroAchievementsPanel"
import type {
  EmulatorInfo,
  EmulatorRomEntry,
  EmulatorRomFolder,
  EmulatorRomIndexEntry,
  EmulatorStatus,
} from "../../global"

/**
 * Configuração global dos emuladores. ROMs de um jogo continuam sendo
 * escolhidas no diálogo do jogo; este painel concentra executáveis, BIOS/core
 * e pastas persistentes para que a configuração não fique escondida em um
 * jogo específico.
 */
export function EmulationSection() {
  const { t } = useI18n()
  const [items, setItems] = useState<EmulatorInfo[]>([])
  const [statuses, setStatuses] = useState<EmulatorStatus[]>([])
  const [romIndex, setRomIndex] = useState<Record<string, EmulatorRomIndexEntry>>({})
  const [selectedId, setSelectedId] = useState("")
  const [busy, setBusy] = useState(true)
  const [error, setError] = useState("")

  const carregar = async (detect = false) => {
    setBusy(true)
    setError("")
    try {
      const api = window.launcherAPI
      const [catalog, status, index] = await Promise.all([
        detect ? api?.emulatorsDetect() : api?.emulatorsList(),
        api?.emulatorsStatus(),
        api?.emulatorsRomIndex?.(),
      ])
      if (!catalog?.ok) {
        setError(catalog?.error || t("emulacao.erro_carregar"))
        return
      }
      setItems(catalog.emulators || [])
      setStatuses(status?.statuses || [])
      setRomIndex(index?.emulators || {})
    } catch (cause) {
      setError(
        String(cause instanceof Error ? cause.message : cause || t("emulacao.falha_carregar")),
      )
    } finally {
      setBusy(false)
    }
  }

  useEffect(() => {
    void carregar()
  }, [])

  const statusMap = useMemo(
    () => new Map(statuses.map((status) => [status.emulatorId || "", status])),
    [statuses],
  )

  return (
    <div className="min-h-full bg-[#08080a] px-8 py-7">
      <div className="desktop-fluid-column mx-auto max-w-6xl pb-10">
        <div className="mb-7 flex flex-wrap items-end justify-between gap-4">
          <div>
            <p className="mb-2 text-[11px] font-semibold uppercase tracking-[0.24em] text-white/35">
              {t("settings.emulacao.grupo")}
            </p>
            <h1 className="text-3xl font-light tracking-wide text-white">
              {t("settings.emulacao")}
            </h1>
            <p className="mt-2 max-w-2xl text-sm leading-relaxed text-white/45">
              {t("settings.emulacao.desc")}
            </p>
          </div>
          <button
            type="button"
            onClick={() => void carregar(true)}
            disabled={busy}
            className="rounded-lg border border-white/10 bg-white/[0.03] px-3.5 py-2 text-xs text-white/65 transition-colors hover:border-white/20 hover:text-white disabled:opacity-40"
          >
            {busy ? t("common.atualizando") : t("emulacao.detectar_novamente")}
          </button>
        </div>

        {error && (
          <p
            role="alert"
            className="mb-5 rounded-xl border border-red-300/20 bg-red-400/10 px-4 py-3 text-xs text-red-100"
          >
            {error}
          </p>
        )}

        <RetroAchievementsPanel />

        {busy && !items.length ? (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {Array.from({ length: 6 }, (_, index) => (
              <div
                key={index}
                className="h-[310px] animate-pulse rounded-2xl border border-white/[0.07] bg-white/[0.025]"
              />
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {items.map((item) => (
              <EmulatorCard
                key={item.id}
                item={item}
                status={statusMap.get(item.id)}
                roms={romIndex[item.id]}
                onConfigure={() => setSelectedId(item.id)}
              />
            ))}
          </div>
        )}
      </div>

      {selectedId &&
        (() => {
          const item = items.find((candidate) => candidate.id === selectedId)
          if (!item) return null
          return (
            <EmulatorConfigDialog
              key={item.id}
              item={item}
              status={statusMap.get(item.id)}
              roms={romIndex[item.id]}
              onClose={() => setSelectedId("")}
              onSaved={async (detect = false) => {
                await carregar(detect)
              }}
            />
          )
        })()}
    </div>
  )
}

function EmulatorCard({
  item,
  status,
  roms,
  onConfigure,
}: {
  item: EmulatorInfo
  status?: EmulatorStatus
  roms?: EmulatorRomIndexEntry
  onConfigure: () => void
}) {
  const { t } = useI18n()
  const art = artFor(item.id)
  const biosMissing = Boolean(status?.required && !status.installed)
  const coreMissing = Boolean(item.requiresCore && !item.profile?.corePath)
  const ready = Boolean(
    item.profile && item.available && !biosMissing && !coreMissing && !status?.running,
  )

  // Determine the card state: running > not-available > detected > configured > ready
  const cardState = status?.running
    ? "running"
    : !item.available
      ? "not-available"
      : biosMissing || coreMissing
        ? "missing-deps"
        : !item.profile
          ? "detected"
          : "configured"

  const statusText = status?.running
    ? t("emulacao.em_execucao")
    : !item.available
      ? t("emulacao.config_necessaria")
      : biosMissing
        ? t("emulacao.bios_necessaria")
        : coreMissing
          ? t("emulacao.core_necessario")
          : !item.profile
            ? t("emulacao.emulador_detectado")
            : t("emulacao.configurado")

  // Visual styling based on card state
  const getBadgeStyle = () => {
    switch (cardState) {
      case "running":
        return "border-purple-300/20 text-purple-200/80"
      case "not-available":
        return "border-amber-300/20 text-amber-200/80"
      case "missing-deps":
        return "border-amber-300/20 text-amber-200/80"
      case "detected":
        return "border-blue-300/20 text-blue-200/80"
      case "configured":
        return "border-emerald-300/20 text-emerald-200/80"
      default:
        return "border-white/20 text-white/60"
    }
  }

  const getBadgeLabel = () => {
    switch (cardState) {
      case "running":
        return t("emulacao.em_uso")
      case "not-available":
        return t("emulacao.acao")
      case "missing-deps":
        return t("emulacao.acao")
      case "detected":
        return t("emulacao.detectado")
      case "configured":
        return t("emulacao.ok")
      default:
        return "—"
    }
  }

  const getStatusBoxStyle = () => {
    switch (cardState) {
      case "running":
        return "border-purple-300/10 bg-purple-400/[0.06]"
      case "not-available":
        return "border-amber-300/10 bg-amber-400/[0.06]"
      case "missing-deps":
        return "border-amber-300/10 bg-amber-400/[0.06]"
      case "detected":
        return "border-blue-300/10 bg-blue-400/[0.06]"
      case "configured":
        return "border-emerald-300/10 bg-emerald-400/[0.06]"
      default:
        return "border-white/10 bg-white/[0.03]"
    }
  }

  const getStatusIcon = () => {
    switch (cardState) {
      case "running":
        return "●"
      case "not-available":
        return "△"
      case "missing-deps":
        return "△"
      case "detected":
        return "◆"
      case "configured":
        return "●"
      default:
        return "○"
    }
  }

  return (
    <article className="group overflow-hidden rounded-2xl border border-white/[0.08] bg-[#101014] shadow-xl shadow-black/10 transition-colors hover:border-white/[0.16]">
      <ConsoleArt item={item} />
      <div className="p-4">
        <div className="mb-1 flex items-start justify-between gap-3">
          <div>
            <h2 className="text-xl font-semibold tracking-tight text-white">{art.system}</h2>
            <p className="mt-0.5 text-sm text-white/45">{item.name}</p>
          </div>
          <span
            className={`rounded-full border px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide ${getBadgeStyle()}`}
          >
            {getBadgeLabel()}
          </span>
        </div>
        <div
          className={`mt-4 rounded-xl border px-3 py-2.5 ${getStatusBoxStyle()}`}
        >
          <div className="flex items-center gap-2 text-sm text-white/75">
            <span aria-hidden="true">{getStatusIcon()}</span>
            <span>{statusText}</span>
          </div>
          <p className="mt-1 text-[11px] leading-relaxed text-white/40">
            {status?.running
              ? t("emulacao.fechar_sessao")
              : biosMissing
                ? t("emulacao.config_bios")
                : coreMissing
                  ? t("emulacao.config_core")
                  : !item.available
                    ? t("emulacao.apontar_exe", { name: item.name })
                    : !item.profile
                      ? t("emulacao.detectado_auto")
                      : t("emulacao.pastas_rom_cnt", { count: item.profile?.romFolders?.length || 0 })}
          </p>
        </div>
        <div className="mt-4 flex items-center justify-between gap-2">
          <span className="rounded-full bg-white/[0.07] px-2.5 py-1 text-[11px] text-white/55">
            {item.systems.join(" / ")}
          </span>
          <button
            type="button"
            onClick={onConfigure}
            className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 text-xs font-medium text-white/75 transition-colors hover:border-white/25 hover:bg-white/[0.08] hover:text-white"
          >
            <GearIcon />
            {t("emulacao.configurar")}
            <span aria-hidden="true">›</span>
          </button>
        </div>
      </div>
    </article>
  )
}

function EmulatorConfigDialog({
  item,
  status,
  roms: savedRoms,
  onClose,
  onSaved,
}: {
  item: EmulatorInfo
  status?: EmulatorStatus
  roms?: EmulatorRomIndexEntry
  onClose: () => void
  onSaved: (detect?: boolean) => Promise<void>
}) {
  const { t } = useI18n()
  const initialProfile = item.profile
  const [executable, setExecutable] = useState(initialProfile?.executable || item.executable || "")
  const [biosPath, setBiosPath] = useState(initialProfile?.biosPath || "")
  const [corePath, setCorePath] = useState(initialProfile?.corePath || "")
  const [argsText, setArgsText] = useState(
    (initialProfile?.args || item.detectedArgs || []).join("\n"),
  )
  const [romFolders, setRomFolders] = useState<EmulatorRomFolder[]>(
    initialProfile?.romFolders || [],
  )
  const [scanResults, setScanResults] = useState<EmulatorRomEntry[]>(savedRoms?.roms || [])
  const [scanTruncated, setScanTruncated] = useState(Boolean(savedRoms?.truncated))
  const [busy, setBusy] = useState(false)
  const [scanBusy, setScanBusy] = useState(false)
  const [error, setError] = useState("")
  const [step, setStep] = useState(0)
  const [installOpen, setInstallOpen] = useState(false)

  const requiredBios = item.id === "duckstation" || item.id === "pcsx2"
  const coreMissing = Boolean(item.requiresCore && !corePath)
  const hasExecutable = Boolean(executable.trim())
  const parseArgs = () =>
    argsText.trim()
      ? argsText
          .split(/\r?\n/)
          .map((arg) => arg.trim())
          .filter(Boolean)
          .slice(0, 32)
      : []

  useEffect(() => {
    if (installOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose()
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [onClose, installOpen])

  // Detection can refresh while the installation guide is open. Pull newly
  // discovered executable/Flatpak argv into the draft without overwriting
  // anything the user has already edited.
  useEffect(() => {
    if (!executable && item.available && item.executable) setExecutable(item.executable)
    if (!argsText.trim() && item.detectedArgs?.length) setArgsText(item.detectedArgs.join("\n"))
  }, [item.available, item.executable, item.detectedArgs, executable, argsText])

  const escolherExecutavel = async () => {
    const result = await window.launcherAPI?.pickFile()
    if (result?.ok && result.path) setExecutable(result.path)
  }

  const escolherBios = async () => {
    const result = await window.launcherAPI?.pickFolder()
    if (result?.ok && result.path) setBiosPath(result.path)
  }

  const adicionarPasta = async () => {
    const result = await window.launcherAPI?.pickFolder()
    if (!result?.ok || !result.path) return
    setRomFolders((current) =>
      current.some((folder) => folder.path === result.path)
        ? current
        : [...current, { path: result.path!, recursive: true }],
    )
  }

  const pesquisarPastas = async () => {
    setScanBusy(true)
    setError("")
    try {
      const merged = new Map<string, EmulatorRomEntry>()
      let truncated = false
      for (const folder of romFolders) {
        const result = await window.launcherAPI?.emulatorsRoms({
          emulatorId: item.id,
          directory: folder.path,
          recursive: folder.recursive,
          maxResults: 256,
        })
        if (!result?.ok) {
          setError(result?.error || t("emulacao.erro_pesquisar_pasta"))
          continue
        }
        truncated = truncated || Boolean(result.truncated)
        for (const rom of result.roms || []) merged.set(rom.path, rom)
      }
      setScanResults([...merged.values()].sort((a, b) => a.path.localeCompare(b.path)))
      setScanTruncated(truncated)
      if (!merged.size) setError(t("emulacao.nenhuma_rom_pastas"))
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause || t("emulacao.falha_pesquisar_roms")))
    } finally {
      setScanBusy(false)
    }
  }

  const continuar = () => {
    setError("")
    if (step === 0 && !hasExecutable) {
      setError(t("emulacao.apontar_exe_emulador"))
      return
    }
    if (step === 1 && item.requiresCore && !corePath.trim()) {
      setError(t("emulacao.selecione_core"))
      return
    }
    setStep((current) => Math.min(3, current + 1))
  }

  const voltar = () => {
    setError("")
    setStep((current) => Math.max(0, current - 1))
  }

  const remover = async () => {
    if (!item.profile) return
    setBusy(true)
    setError("")
    try {
      const result = await window.launcherAPI?.emulatorProfileRemove(item.id)
      if (!result?.ok) {
        setError(result?.error || t("emulacao.erro_remover_config"))
        return
      }
      await onSaved()
      onClose()
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause || t("emulacao.falha_remover")))
    } finally {
      setBusy(false)
    }
  }

  const salvar = async () => {
    if (!executable.trim()) {
      setError(t("emulacao.apontar_exe_emulador"))
      return
    }
    if (item.requiresCore && !corePath.trim()) {
      setError(t("emulacao.selecione_core"))
      return
    }
    setBusy(true)
    setError("")
    try {
      const result = await window.launcherAPI?.emulatorProfileSet({
        id: item.id,
        executable: executable.trim(),
        biosPath: requiredBios ? biosPath || undefined : undefined,
        corePath: item.id === "retroarch" ? corePath || undefined : undefined,
        romFolders,
        args: parseArgs(),
      })
      if (!result?.ok) {
        setError(result?.error || t("emulacao.erro_salvar_config"))
        return
      }
      await onSaved()
      onClose()
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause || t("emulacao.falha_salvar")))
    } finally {
      setBusy(false)
    }
  }

  if (installOpen) {
    return (
      <EmulatorInstallDialog
        item={item}
        onClose={() => setInstallOpen(false)}
        onRefresh={() => onSaved(true)}
        onManualBrowse={async () => {
          setInstallOpen(false)
          await escolherExecutavel()
        }}
      />
    )
  }

  return (
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/75 p-4 backdrop-blur-sm"
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="emulator-config-title"
        className="flex max-h-[92vh] w-[680px] max-w-full flex-col overflow-hidden rounded-2xl border border-white/[0.1] bg-[#0d0d10] shadow-2xl shadow-black/60"
      >
        <div className="flex items-start justify-between border-b border-white/[0.07] px-6 py-5">
          <div>
            <p className="mb-1 text-[11px] font-semibold uppercase tracking-[0.2em] text-white/35">
              {t("emulacao.configuracao")}
            </p>
            <h2 id="emulator-config-title" className="text-xl font-semibold text-white">
              {t("emulacao.configuracao_do", { system: artFor(item.id).system })}
            </h2>
            <p className="mt-1 text-xs text-white/45">
              {item.name} · {item.description}
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1.5 text-white/45 transition-colors hover:bg-white/[0.07] hover:text-white"
            aria-label={t("common.fechar")}
          >
            <CloseIcon />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
          {error && (
            <p
              role="alert"
              className="mb-4 rounded-xl border border-red-300/20 bg-red-400/10 px-3 py-2 text-xs text-red-100"
            >
              {error}
            </p>
          )}
          {step === 0 ? (
            <div className="space-y-5">
              <div>
                <h3 className="text-2xl font-semibold tracking-tight text-white">
                  {t("emulacao.encontrar", { name: item.name })}
                </h3>
                <p className="mt-2 max-w-xl text-sm leading-relaxed text-white/50">
                  {t("emulacao.desc_sistema", { name: item.name })}
                </p>
              </div>
              <div
                className={`rounded-xl border p-4 ${item.available ? "border-emerald-300/15 bg-emerald-400/[0.06]" : hasExecutable ? "border-blue-300/15 bg-blue-400/[0.06]" : "border-amber-300/15 bg-amber-400/[0.07]"}`}
              >
                <div className="flex items-center gap-3">
                  <span
                    className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-lg ${item.available ? "bg-emerald-400/15 text-emerald-200" : hasExecutable ? "bg-blue-400/15 text-blue-200" : "bg-amber-400/15 text-amber-200"}`}
                    aria-hidden="true"
                  >
                    {item.available ? "✓" : hasExecutable ? "⌁" : "△"}
                  </span>
                  <div className="min-w-0">
                    <p className="text-sm font-semibold text-white/85">
                      {item.available
                        ? t("emulacao.detectado_txt", { name: item.name })
                        : hasExecutable
                          ? t("emulacao.executavel_manual")
                          : t("emulacao.nao_detectado_txt", { name: item.name })}
                    </p>
                    <p
                      className="mt-0.5 truncate text-xs text-white/45"
                      title={executable || undefined}
                    >
                      {item.available
                        ? executable || t("emulacao.pronto_configurar")
                        : hasExecutable
                          ? executable
                          : t("emulacao.clique_explorar")}
                    </p>
                  </div>
                </div>
              </div>
              <div className="flex flex-wrap items-center justify-between gap-3 text-xs">
                <button
                  type="button"
                  onClick={() => void escolherExecutavel()}
                  className="font-medium text-white/65 underline decoration-white/20 underline-offset-4 transition-colors hover:text-white hover:decoration-white/60"
                >
                  {t("emulacao.nao_correto")} <span className="text-white">{t("emulacao.explorar_manual")}</span>
                </button>
                <button
                  type="button"
                  onClick={() => setInstallOpen(true)}
                  className="font-semibold text-white/75 transition-colors hover:text-white"
                >
                  {t("emulacao.nao_tenho", { name: item.name })}
                </button>
              </div>
              <p className="rounded-lg border border-white/[0.06] bg-white/[0.025] px-3 py-2 text-[11px] leading-relaxed text-white/40">
                {t("emulacao.linux_intro")} {t("emulacao.linux_informe")} <code className="text-white/65">flatpak</code> {t("emulacao.linux_como_exe")}{" "}
                <code className="text-white/65">run</code> {t("emulacao.linux_final")}
              </p>
            </div>
          ) : (
            <div className="mb-5 rounded-xl border border-white/[0.08] bg-white/[0.025] p-3">
              <div className="flex items-center gap-2 text-sm font-medium text-white/85">
                <span aria-hidden="true">{status?.running ? "△" : item.available ? "✓" : "⌁"}</span>
                {status?.running
                  ? t("emulacao.em_execucao_aviso")
                  : item.available
                    ? t("emulacao.emulador_detectado")
                    : t("emulacao.executavel_manual")}
              </div>
              <p className="mt-1 truncate text-xs leading-relaxed text-white/45" title={executable}>
                {executable || t("emulacao.nenhum_executavel")}
              </p>
            </div>
          )}

          {step === 1 && (
            <StepHeading title={t("emulacao.ajustes_sistema")}>
              {t("emulacao.desc_ajustes")}
            </StepHeading>
          )}
          {step === 1 && requiredBios && (
            <FieldLabel
              label={t("emulacao.pasta_bios")}
              hint={t("emulacao.pasta_bios_hint")}
            >
              <div className="flex gap-2">
                <input
                  value={biosPath}
                  readOnly
                  placeholder={t("emulacao.bios_placeholder")}
                  className="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2.5 font-mono text-xs text-white/70"
                />
                <button
                  type="button"
                  onClick={() => void escolherBios()}
                  className="shrink-0 rounded-lg border border-white/10 px-3 text-xs text-white/70 transition-colors hover:bg-white/10 hover:text-white"
                >
                  {t("common.escolher_pasta")}
                </button>
              </div>
              <p
                className={`mt-1.5 text-[11px] ${status?.installed ? "text-emerald-200/70" : "text-amber-200/70"}`}
              >
                {status?.installed
                  ? status.detectedPath
                    ? t("emulacao.bios_detectado_em", { path: status.detectedPath })
                    : t("emulacao.bios_detectado")
                  : t("emulacao.bios_nao_detectado")}
              </p>
            </FieldLabel>
          )}

          {step === 1 && item.id === "rpcs3" && (
            <p className="mb-4 rounded-lg border border-white/[0.07] bg-white/[0.025] px-3 py-2 text-[11px] leading-relaxed text-white/45">
              {t("emulacao.firmware_rpcs3")}
              {status?.installed
                ? t("emulacao.firmware_detectado")
                : t("emulacao.firmware_config")}
            </p>
          )}

          {step === 1 && item.id === "retroarch" && (
            <FieldLabel
              label={t("emulacao.core_libretro")}
              hint={t("emulacao.core_libretro_hint")}
            >
              <div className="flex gap-2">
                <input
                  value={corePath}
                  readOnly
                  placeholder={t("emulacao.core_placeholder")}
                  className="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2.5 font-mono text-xs text-white/70"
                />
                <button
                  type="button"
                  onClick={async () => {
                    const result = await window.launcherAPI?.pickFile()
                    if (result?.ok && result.path) setCorePath(result.path)
                  }}
                  className="shrink-0 rounded-lg border border-white/10 px-3 text-xs text-white/70 transition-colors hover:bg-white/10 hover:text-white"
                >
                  {t("emulacao.escolher_core")}
                </button>
              </div>
            </FieldLabel>
          )}

          {step === 2 && (
            <StepHeading title={t("emulacao.sua_biblioteca_roms")}>
              {t("emulacao.desc_roms")}
            </StepHeading>
          )}
          {step === 2 && (
            <FieldLabel
              label={t("emulacao.pastas_rom_label")}
              hint={t("emulacao.pastas_rom_hint")}
            >
              <div className="space-y-1.5">
                {romFolders.length ? (
                  romFolders.map((folder) => (
                    <div
                      key={folder.path}
                      className="flex items-center gap-2 rounded-lg border border-white/[0.07] bg-white/[0.025] px-3 py-2 text-xs text-white/65"
                    >
                      <span className="min-w-0 flex-1 truncate font-mono" title={folder.path}>
                        {folder.path}
                      </span>
                      <button
                        type="button"
                        onClick={() =>
                          setRomFolders((current) =>
                            current.filter((candidate) => candidate.path !== folder.path),
                          )
                        }
                        className="text-red-100/60 hover:text-red-100"
                      >
                        {t("common.remover")}
                      </button>
                    </div>
                  ))
                ) : (
                  <p className="rounded-lg border border-dashed border-white/10 px-3 py-2 text-xs text-white/35">
                    {t("emulacao.nenhuma_pasta")}
                  </p>
                )}
                <button
                  type="button"
                  onClick={() => void adicionarPasta()}
                  className="rounded-lg border border-white/10 px-3 py-2 text-xs text-white/70 transition-colors hover:bg-white/10 hover:text-white"
                >
                  {t("emulacao.adicionar_pasta")}
                </button>
                <button
                  type="button"
                  onClick={() => void pesquisarPastas()}
                  disabled={scanBusy || !romFolders.length}
                  className="ml-2 rounded-lg border border-white/10 px-3 py-2 text-xs text-white/70 transition-colors hover:bg-white/10 hover:text-white disabled:opacity-40"
                >
                  {scanBusy ? t("common.pesquisando") : t("emulacao.pesquisar_roms")}
                </button>
              </div>
            </FieldLabel>
          )}

          {step === 2 && scanResults.length > 0 && (
            <div className="mb-4 rounded-xl border border-white/[0.07] bg-white/[0.02] p-3">
              <div className="mb-2 flex items-center justify-between gap-3">
                <p className="text-xs font-medium text-white/75">{t("emulacao.roms_encontradas")}</p>
                <span className="text-[10px] text-white/35">
                  {scanResults.length}
                  {scanTruncated ? "+" : ""}
                </span>
              </div>
              <div className="max-h-28 space-y-1 overflow-y-auto">
                {scanResults.slice(0, 256).map((rom) => (
                  <p key={rom.path} className="truncate text-[11px] text-white/45" title={rom.path}>
                    {rom.relativePath}
                  </p>
                ))}
              </div>
            </div>
          )}

          {step === 3 && (
            <StepHeading title={t("emulacao.preferencias_inicializacao")}>
              {t("emulacao.desc_inicializacao")}
            </StepHeading>
          )}
          {step === 3 && (
            <FieldLabel
              label={t("emulacao.argumentos_adicionais")}
              hint={t("emulacao.argumentos_hint")}
            >
              <textarea
                value={argsText}
                onChange={(event) => setArgsText(event.target.value)}
                maxLength={4096}
                spellCheck={false}
                rows={3}
                placeholder="--fullscreen"
                className="w-full resize-y rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2.5 font-mono text-xs text-white outline-none placeholder:text-white/25 focus:border-[color:var(--accent)]"
              />
            </FieldLabel>
          )}
        </div>

        <div className="flex items-center justify-between border-t border-white/[0.07] px-6 py-4">
          <div className="flex items-center gap-1" aria-label={t("emulacao.etapa_aria", { n: step + 1, total: 4 })}>
            {[0, 1, 2, 3].map((currentStep) => (
              <span
                key={currentStep}
                className={`h-1.5 w-6 rounded-full transition-colors ${currentStep <= step ? "bg-white/80" : "bg-white/15"}`}
              />
            ))}
          </div>
          <div className="flex items-center gap-2">
            {step === 3 && item.profile && (
              <button
                type="button"
                onClick={() => void remover()}
                disabled={busy}
                className="rounded-lg px-3 py-2 text-xs font-medium text-red-100/65 transition-colors hover:bg-red-400/10 hover:text-red-100 disabled:opacity-35"
              >
                {t("emulacao.remover_configuracao")}
              </button>
            )}
            {step > 0 ? (
              <button
                type="button"
                onClick={voltar}
                disabled={busy}
                className="rounded-lg px-4 py-2 text-xs font-medium text-white/60 transition-colors hover:bg-white/[0.06] hover:text-white disabled:opacity-35"
              >
                {t("common.voltar")}
              </button>
            ) : (
              <button
                type="button"
                onClick={onClose}
                className="rounded-lg px-4 py-2 text-xs font-medium text-white/55 transition-colors hover:bg-white/[0.06] hover:text-white"
              >
                {t("common.cancelar")}
              </button>
            )}
            {step < 3 ? (
              <button
                type="button"
                onClick={continuar}
                disabled={busy || (step === 0 && !hasExecutable) || (step === 1 && coreMissing)}
                className="rounded-lg bg-white px-5 py-2 text-xs font-semibold text-black transition-opacity disabled:cursor-not-allowed disabled:opacity-35"
              >
                {t("emulacao.continuar")}
              </button>
            ) : (
              <button
                type="button"
                onClick={() => void salvar()}
                disabled={busy || !hasExecutable || coreMissing}
                className="rounded-lg bg-white px-5 py-2 text-xs font-semibold text-black transition-opacity disabled:cursor-not-allowed disabled:opacity-35"
              >
                {busy ? t("common.salvando") : t("emulacao.salvar_configuracao")}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

function StepHeading({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="mb-5">
      <h3 className="text-2xl font-semibold tracking-tight text-white">{title}</h3>
      <p className="mt-2 max-w-xl text-sm leading-relaxed text-white/50">{children}</p>
    </div>
  )
}

function FieldLabel({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <label className="mb-4 block text-xs text-white/60">
      <span className="mb-1.5 block font-medium text-white/75">{label}</span>
      {children}
      {hint && (
        <span className="mt-1.5 block text-[11px] leading-relaxed text-white/35">{hint}</span>
      )}
    </label>
  )
}

function artFor(id: string) {
  const values: Record<string, { system: string; tag: string; color: string; accent: string }> = {
    duckstation: { system: "PlayStation 1", tag: "PS1", color: "#3d536c", accent: "#d8e6f5" },
    pcsx2: { system: "PlayStation 2", tag: "PS2", color: "#243c32", accent: "#77d4a3" },
    rpcs3: { system: "PlayStation 3", tag: "PS3", color: "#34334f", accent: "#9c9ce8" },
    dolphin: { system: "GameCube / Wii", tag: "GC", color: "#344a70", accent: "#a8c7ff" },
    ppsspp: { system: "PlayStation Portable", tag: "PSP", color: "#4d354e", accent: "#e5a6dc" },
    retroarch: { system: "Multi-sistema", tag: "RA", color: "#503b2b", accent: "#ffc777" },
    melonds: { system: "Nintendo DS", tag: "DS", color: "#344c4c", accent: "#9de8e8" },
    desmume: { system: "Nintendo DS", tag: "DS", color: "#493b31", accent: "#f0ca96" },
  }
  return values[id] || { system: "Clássicos", tag: "EMU", color: "#3c3c45", accent: "#d3d3df" }
}

function ConsoleArt({ item }: { item: EmulatorInfo }) {
  const art = artFor(item.id)
  const artworkById: Record<string, string> = {
    duckstation: "./emulation/ps1.png",
    pcsx2: "./emulation/ps2.png",
    rpcs3: "./emulation/ps3.png",
    dolphin: "./emulation/dolphin.svg",
    ppsspp: "./emulation/ppsspp.svg",
    melonds: "./emulation/melonds.svg",
    desmume: "./emulation/desmume.svg",
    retroarch: "./emulation/retroarch.svg",
  }
  const artwork = artworkById[item.id] || ""
  return (
    <div
      className="relative h-36 overflow-hidden"
      style={{ background: `linear-gradient(135deg, ${art.color}, #111116 78%)` }}
    >
      {artwork ? (
        <>
          <img
            src={artwork}
            alt=""
            className="absolute inset-0 h-full w-full object-contain object-center opacity-90"
            draggable={false}
          />
          <div className="absolute inset-0 bg-gradient-to-t from-black/45 via-transparent to-transparent" />
        </>
      ) : (
        <>
          <div
            className="absolute -right-8 -top-10 h-36 w-36 rounded-full opacity-20"
            style={{ background: art.accent, filter: "blur(14px)" }}
          />
          <div className="absolute bottom-4 left-5 flex items-end gap-3">
            <div
              className="relative h-14 w-28 rounded-[18px] border-2 shadow-2xl"
              style={{
                borderColor: `${art.accent}99`,
                background: `${art.color}dd`,
                transform: "skewY(-4deg)",
              }}
            >
              <span
                className="absolute left-4 top-4 h-3 w-3 rounded-full"
                style={{ background: art.accent }}
              />
              <span className="absolute right-4 top-3 h-2 w-8 rounded-full bg-black/40" />
              <span
                className="absolute bottom-2 right-4 text-[10px] font-bold tracking-[0.2em]"
                style={{ color: art.accent }}
              >
                {art.tag}
              </span>
            </div>
            <div className="mb-[-2px] flex h-8 w-12 items-center justify-center rounded-full border border-white/20 bg-black/30 text-[10px] font-bold text-white/65">
              {item.name}
            </div>
          </div>
        </>
      )}
      <div className="absolute right-4 top-4 rounded-full border border-white/15 bg-black/25 px-2 py-1 text-[10px] font-bold uppercase tracking-[0.18em] text-white/65">
        {art.tag}
      </div>
    </div>
  )
}

function GearIcon() {
  return <span aria-hidden="true">⚙</span>
}

function CloseIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
    >
      <path d="M6 6l12 12M18 6L6 18" />
    </svg>
  )
}
