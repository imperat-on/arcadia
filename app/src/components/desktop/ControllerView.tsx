"use client"

import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { useGamepadConnection, type ControleConectado } from "../useGamepadConnection"
import { DEADZONE_MAX, DEADZONE_MIN, DEADZONE_PADRAO, setControllerConfig } from "../controllerConfig"
import { GamepadDiagnostics } from "../GamepadDiagnostics"

// Nome amigável por família de controle.
function nomeControle(c: ControleConectado): string {
  const s = (c.id || "").toLowerCase()
  if (/dualsense/.test(s)) return "DualSense (PS5)"
  if (/dualshock|playstation|wireless controller/.test(s)) return "DualShock (PS4)"
  if (/xbox|microsoft/.test(s)) return "Xbox (XInput)"
  if (/joy-?con|switch|nintendo/.test(s)) return "Joy-Con / Switch Pro"
  if (c.id) return c.id.slice(0, 60)
  return "Controle"
}

// Ícone/emoji por família — simples e sem dependência.
function iconeControle(tipo: ControleConectado["tipo"]): string {
  switch (tipo) {
    case "xbox": return "🟢"
    case "playstation": return "🔵"
    case "switch": return "🔴"
    default: return "⚪"
  }
}

export function ControllerView() {
  const { t } = useI18n()
  const { controles, primeiro } = useGamepadConnection()
  const [cfg, setCfg] = useState<Record<string, unknown>>({})
  const [statusEmulador, setStatusEmulador] = useState<string>("checking")

  useEffect(() => {
    window.launcherAPI?.getConfig?.().then((c) => setCfg((c as Record<string, unknown>) || {}))
  }, [])

  const navegacaoAtiva = cfg.enable_controller_navigation !== false
  const deadzone = Number(cfg.controller_deadzone ?? DEADZONE_PADRAO)

  // Salva patch de config.
  const salvar = (patch: Record<string, unknown>) => {
    const next = { ...cfg, ...patch }
    setCfg(next)
    // O cache dos laços de gamepad anda junto: o efeito da mudança precisa ser
    // imediato (o slider é a zona morta que o laço lê no quadro seguinte).
    setControllerConfig({
      enable_controller_navigation: next.enable_controller_navigation as boolean | undefined,
      controller_deadzone: next.controller_deadzone as number | undefined,
    })
    window.launcherAPI?.setConfig?.(patch)
  }

  // Consulta o status do emulador XInput no main (IPC se existir).
  useEffect(() => {
    const launcher = window as unknown as {
      launcherAPI?: { gamepadStatus?: () => Promise<{ emulator: string; running: boolean }> }
    }
    const check = async () => {
      try {
        const r = await launcher.launcherAPI?.gamepadStatus?.()
        if (r) setStatusEmulador(r.running ? "ativo" : "inativo")
        else setStatusEmulador("indisponivel")
      } catch {
        setStatusEmulador("indisponivel")
      }
    }
    check()
    const id = setInterval(check, 3000)
    return () => clearInterval(id)
  }, [])

  const controleConectado = Boolean(primeiro)

  return (
    <div className="max-w-2xl space-y-8 pb-10">
      <h1 className="mb-2 text-2xl font-light tracking-wide text-white">
        {t("controller.titulo")}
      </h1>
      <p className="mb-6 text-sm text-white/50">{t("controller.descricao")}</p>

      {/* Estado do(s) controle(s) conectado(s) */}
      <section className="rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5">
        <h2 className="mb-4 text-sm font-semibold text-[#a8b3cc]">{
          t("controller.controles_conectados")
        }</h2>
        {controleConectado ? (
          <ul className="space-y-3">
            {(controles as ControleConectado[]).map((c) => (
              <li key={c.index} className="flex items-center gap-3">
                <span className="flex h-10 w-10 items-center justify-center rounded-xl bg-white/[0.05] text-lg">
                  {iconeControle(c.tipo)}
                </span>
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium text-white">
                    {nomeControle(c)}
                  </div>
                  <div className="text-xs text-white/40">
                    {t("controller.tipo")}: {t(`controller.tipo_${c.tipo}`)} · #{c.index}
                  </div>
                </div>
                <span className="ml-auto inline-flex items-center gap-1.5 rounded-full bg-emerald-500/15 px-2.5 py-1 text-xs font-medium text-emerald-300">
                  <span className="h-1.5 w-1.5 rounded-full bg-emerald-400" />
                  {t("controller.conectado")}
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <div className="flex items-center gap-3 rounded-xl border border-dashed border-white/15 px-4 py-6 text-sm text-white/40">
            <span className="text-2xl">🎮</span>
            <div>
              <div className="font-medium text-white/70">{t("controller.nenhum_conectado")}</div>
              <div className="text-xs text-white/40">{t("controller.nenhum_conectado_desc")}</div>
            </div>
          </div>
        )}
      </section>

      {/* Emulador XInput (p/ jogos Windows) */}
      <section className="rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5">
        <h2 className="mb-2 text-sm font-semibold text-[#a8b3cc]">{t("controller.emulador_titulo")}</h2>
        <p className="mb-4 text-xs leading-relaxed text-white/40">{t("controller.emulador_desc")}</p>
        <div className="flex items-center gap-2">
          <StatusBadge status={statusEmulador} />
          <span className="text-xs text-white/50">
            {statusEmulador === "ativo"
              ? t("controller.emulador_ativo")
              : statusEmulador === "inativo"
                ? t("controller.emulador_inativo")
                : t("controller.emulador_indisponivel")}
          </span>
        </div>
      </section>
      <section>
        <h2 className="mb-3 text-sm font-semibold text-[#a8b3cc]">{t("controller.navegacao_titulo")}</h2>
        <div className="space-y-2.5">
          <Check
            label={t("accessibility.navegacao_controle")}
            checked={navegacaoAtiva}
            onChange={(v) => salvar({ enable_controller_navigation: v })}
          />
          <div
            className={`rounded-xl border border-white/[0.08] bg-white/[0.02] px-4 py-3 transition-opacity ${
              navegacaoAtiva ? "" : "pointer-events-none opacity-40"
            }`}
          >
            <div className="mb-2 flex items-center justify-between gap-4">
              <span className="text-sm text-white/80">{t("controller.zona_morta")}</span>
              <span className="font-mono text-xs text-white/50">{Math.round(deadzone * 100)}%</span>
            </div>
            <input
              type="range"
              min={DEADZONE_MIN}
              max={DEADZONE_MAX}
              step={0.05}
              value={deadzone}
              onChange={(e) => salvar({ controller_deadzone: Number(e.target.value) })}
              disabled={!navegacaoAtiva}
              className="w-full accent-[color:var(--accent)]"
            />
            <p className="mt-1.5 text-xs leading-relaxed text-white/40">
              {t("accessibility.zona_morta_desc")}
            </p>
          </div>
        </div>
      </section>

      {/* Testar controle ao vivo */}
      <section>
        <h2 className="mb-3 text-sm font-semibold text-[#a8b3cc]">{t("controller.testar_titulo")}</h2>
        <GamepadDiagnostics deadzone={deadzone} />
      </section>
    </div>
  )
}

function StatusBadge({ status }: { status: string }) {
  const map: Record<string, { dot: string; bg: string; text: string }> = {
    ativo: { dot: "bg-emerald-400", bg: "bg-emerald-500/15", text: "text-emerald-300" },
    inativo: { dot: "bg-amber-400", bg: "bg-amber-500/15", text: "text-amber-300" },
    indisponivel: { dot: "bg-slate-400", bg: "bg-slate-500/15", text: "text-slate-300" },
    checking: { dot: "bg-slate-400 animate-pulse", bg: "bg-slate-500/15", text: "text-slate-300" },
  }
  const m = map[status] || map.checking
  return (
    <span className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium ${m.bg} ${m.text}`}>
      <span className={`h-1.5 w-1.5 rounded-full ${m.dot}`} />
    </span>
  )
}

function Check({
  label,
  checked,
  onChange,
}: {
  label: string
  checked: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <button
      onClick={() => onChange(!checked)}
      className="flex w-full items-center gap-3 rounded-xl border border-white/[0.08] bg-white/[0.02] px-4 py-3 text-left transition-colors hover:bg-white/[0.04]"
      role="checkbox"
      aria-checked={checked}
    >
      <span
        className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-md border transition-colors ${
          checked
            ? "border-[color:var(--accent)] bg-[color:var(--accent)] text-black"
            : "border-white/25"
        }`}
      >
        {checked && (
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3.5" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12" />
          </svg>
        )}
      </span>
      <span className="text-sm text-white/85">{label}</span>
    </button>
  )
}
