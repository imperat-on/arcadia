"use client"

import { useEffect, useState } from "react"
import type { AppConfig } from "../../global"
import { aplicarA11y } from "./AccessibilityView"
import { TEMAS } from "../../themes"
import { useI18n } from "../../i18n/I18nContext"

const LANGS = [
  { id: "pt-BR", label: "Português (Brasil)" },
  { id: "en-US", label: "English (US)" },
  { id: "es-ES", label: "Español" },
]

const FEATURED_OPTS_FN = (t: (k: string) => string) => [
  { id: "disabled", label: t("featured.disabled") },
  { id: "recent", label: t("featured.recent") },
  { id: "favorites", label: t("featured.favorites") },
  { id: "most-played", label: t("featured.most_played") },
] as const

export function GeneralSection({ onSaved }: { onSaved: () => void }) {
  const { t, lang, setLang } = useI18n()
  const [cfg, setCfg] = useState<AppConfig>({})
  const home = window.launcherPaths?.home || "~"

  useEffect(() => {
    window.launcherAPI?.getConfig().then((c) => setCfg(c || {}))
  }, [])

  const set = async <K extends keyof AppConfig>(key: K, val: AppConfig[K]) => {
    setCfg((c) => ({ ...c, [key]: val }))
    await window.launcherAPI?.setConfig({ [key]: val } as Partial<AppConfig>)
    onSaved()
  }

  const pickFolder = async (key: keyof AppConfig) => {
    const r = await window.launcherAPI?.pickFolder()
    if (r?.path) await set(key, r.path as never)
  }

  const FEATURED_OPTS = FEATURED_OPTS_FN(t)

  return (
    <div className="max-w-2xl">
      <h2 className="text-3xl font-light tracking-wide text-white mb-1">{t("settings.general")}</h2>
      <p className="text-sm text-[#8a93a6] mb-8">
        {t("settings.general_desc")}
      </p>

      <Group title={t("settings.language")}>
        <Select
          label={t("settings.language.label")}
          desc={t("settings.language.desc")}
          value={cfg.language ?? lang}
          options={LANGS}
          onChange={(v) => {
            set("language", v)
            setLang(v)
          }}
        />
        <Select
          label={t("settings.theme.label")}
          desc={t("settings.theme.desc")}
          value={cfg.theme_name ?? "midnight"}
          options={TEMAS.map((t) => ({ id: t.id, label: t.nome }))}
          onChange={(v) => {
            const tema = TEMAS.find((t) => t.id === v)
            const patch = { theme_name: v, accent: tema?.accent ?? cfg.accent }
            setCfg((c) => {
              const next = { ...c, ...patch }
              aplicarA11y(next)
              return next
            })
            window.launcherAPI?.setConfig(patch)
            onSaved()
          }}
        />
        <Path
          label={t("settings.custom_css.label")}
          desc={t("settings.custom_css.desc")}
          value={cfg.custom_css_path ?? ""}
          placeholder={t("settings.custom_css.placeholder")}
          onPick={() => pickFolder("custom_css_path")}
        />
      </Group>

      <Group title={t("settings.paths")}>
        <Path
          label={t("settings.install_path.label")}
          desc={t("settings.install_path.desc")}
          value={cfg.default_install_path ?? `${home}/Games/Arcadia`}
          onPick={() => pickFolder("default_install_path")}
        />
        <Path
          label={t("settings.wine_prefix.label")}
          desc={t("settings.wine_prefix.desc")}
          value={cfg.default_wine_prefix_path ?? `${home}/Games/Arcadia/Prefixes`}
          onPick={() => pickFolder("default_wine_prefix_path")}
        />
        <Path
          label={t("settings.steam_path.label")}
          desc={t("settings.steam_path.desc")}
          value={cfg.steam_path ?? `${home}/.steam/steam`}
          onPick={() => pickFolder("steam_path")}
        />
        <Path
          label={t("settings.egs_prefix.label")}
          desc={t("settings.egs_prefix.desc")}
          value={cfg.epic_egs_prefix ?? ""}
          placeholder={t("settings.egs_prefix.placeholder")}
          onPick={() => pickFolder("epic_egs_prefix")}
        />
      </Group>

      <Group title={t("settings.behavior")}>
        <Toggle
          label={t("settings.check_updates.label")}
          desc={t("settings.check_updates.desc")}
          value={cfg.check_updates_on_start !== false}
          onChange={(v) => set("check_updates_on_start", v)}
        />
        <Toggle
          label={t("settings.auto_update.label")}
          desc={t("settings.auto_update.desc")}
          value={cfg.auto_update_games === true}
          onChange={(v) => set("auto_update_games", v)}
        />
        <Toggle
          label={t("settings.hide_changelog.label")}
          desc={t("settings.hide_changelog.desc")}
          value={cfg.hide_changelog_on_start === true}
          onChange={(v) => set("hide_changelog_on_start", v)}
        />
        <Toggle
          label={t("settings.console_mode.label")}
          desc={t("settings.console_mode.desc")}
          value={cfg.start_in_console_mode === true}
          onChange={(v) => set("start_in_console_mode", v)}
        />
        <Toggle
          label={t("settings.hide_tray.label")}
          desc={t("settings.hide_tray.desc")}
          value={cfg.hide_tray_icon === true}
          onChange={(v) => set("hide_tray_icon", v)}
        />
        <Toggle
          label={t("settings.close_to_tray.label")}
          desc={t("settings.close_to_tray.desc")}
          value={cfg.close_to_tray === true}
          onChange={(v) => set("close_to_tray", v)}
        />
        <Toggle
          label={t("settings.start_minimized.label")}
          desc={t("settings.start_minimized.desc")}
          value={cfg.start_minimized === true}
          disabled
          onChange={(v) => set("start_minimized", v)}
        />
        <Toggle
          label={t("settings.minimize_on_launch.label")}
          desc={t("settings.minimize_on_launch.desc")}
          value={cfg.minimize_on_game_launch === true}
          onChange={(v) => set("minimize_on_game_launch", v)}
        />
        <Toggle
          label={t("settings.dark_tray.label")}
          desc={t("settings.dark_tray.desc")}
          value={cfg.dark_tray_icon === true}
          onChange={(v) => set("dark_tray_icon", v)}
        />
        <Toggle
          label={t("settings.frameless.label")}
          desc={t("settings.frameless.desc")}
          value={cfg.frameless_window === true}
          onChange={(v) => set("frameless_window", v)}
        />
      </Group>

      <Group title={t("settings.shortcuts")}>
        <Toggle
          label={t("settings.auto_desktop.label")}
          desc={t("settings.auto_desktop.desc")}
          value={cfg.auto_desktop_shortcuts === true}
          onChange={(v) => set("auto_desktop_shortcuts", v)}
        />
        <Toggle
          label={t("settings.auto_menu.label")}
          desc={t("settings.auto_menu.desc")}
          value={cfg.auto_start_menu_shortcuts === true}
          onChange={(v) => set("auto_start_menu_shortcuts", v)}
        />
        <Toggle
          label={t("settings.auto_steam.label")}
          desc={t("settings.auto_steam.desc")}
          value={cfg.auto_add_to_steam !== false}
          onChange={(v) => set("auto_add_to_steam", v)}
        />
        <Toggle
          label={t("settings.disable_playtime.label")}
          desc={t("settings.disable_playtime.desc")}
          value={cfg.disable_playtime_tracking === true}
          onChange={(v) => set("disable_playtime_tracking", v)}
        />
        <Toggle
          label={t("settings.discord_rpc.label")}
          desc={t("settings.discord_rpc.desc")}
          value={cfg.discord_rich_presence === true}
          onChange={(v) => set("discord_rich_presence", v)}
        />
      </Group>

      <Group title={t("settings.library")}>
        <Select
          label={t("settings.featured_column.label")}
          desc={t("settings.featured_column.desc")}
          value={cfg.library_featured_column ?? "disabled"}
          options={FEATURED_OPTS}
          onChange={(v) => set("library_featured_column", v as AppConfig["library_featured_column"])}
        />
        <NumberField
          label={t("settings.recent_max.label")}
          desc={t("settings.recent_max.desc")}
          value={cfg.recent_games_max ?? 5}
          min={1}
          max={30}
          onChange={(v) => set("recent_games_max", v)}
        />
        <NumberField
          label={t("settings.download_cores.label")}
          desc={t("settings.download_cores.desc")}
          value={cfg.download_cpu_cores ?? 0}
          min={0}
          max={64}
          onChange={(v) => set("download_cpu_cores", v)}
        />
      </Group>
    </div>
  )
}

/* ------------------------------------------------------------------ */
/*  Componentes base (mesmo visual do resto das Configurações)         */
/* ------------------------------------------------------------------ */

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mb-8">
      <h3 className="mb-3 text-xs font-semibold uppercase tracking-[0.2em] text-white/40">{title}</h3>
      <div className="flex flex-col gap-2 rounded-2xl border border-white/[0.06] bg-white/[0.02] p-2">
        {children}
      </div>
    </section>
  )
}

function Row({ label, desc, control }: { label: string; desc?: string; control: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-xl px-3 py-2.5 transition-colors hover:bg-white/[0.03]">
      <div className="min-w-0">
        <div className="text-[14px] text-white/90">{label}</div>
        {desc && <div className="mt-0.5 text-xs leading-snug text-white/40">{desc}</div>}
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  )
}

function Toggle({
  label,
  desc,
  value,
  disabled,
  onChange,
}: {
  label: string
  desc?: string
  value: boolean
  disabled?: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <Row
      label={label}
      desc={desc}
      control={
        <button
          role="switch"
          aria-checked={value}
          disabled={disabled}
          onClick={() => onChange(!value)}
          className={`relative h-6 w-11 rounded-full transition-colors ${disabled ? "cursor-not-allowed opacity-35" : ""}`}
          style={{ background: value ? "var(--accent)" : "rgba(255,255,255,0.12)" }}
        >
          <span
            className="absolute top-0.5 h-5 w-5 rounded-full bg-white transition-all"
            style={{ left: value ? "22px" : "2px" }}
          />
        </button>
      }
    />
  )
}

function Select({
  label,
  desc,
  value,
  options,
  onChange,
}: {
  label: string
  desc?: string
  value: string
  options: readonly { id: string; label: string }[]
  onChange: (v: string) => void
}) {
  return (
    <Row
      label={label}
      desc={desc}
      control={
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="rounded-lg border border-white/10 bg-[#141419] px-3 py-1.5 text-[13px] text-white/85 outline-none transition-colors hover:border-white/25 focus:border-[var(--accent)]"
        >
          {options.map((o) => (
            <option key={o.id} value={o.id}>
              {o.label}
            </option>
          ))}
        </select>
      }
    />
  )
}

function Path({
  label,
  desc,
  value,
  placeholder,
  onPick,
}: {
  label: string
  desc?: string
  value: string
  placeholder?: string
  onPick: () => void
}) {
  const { t } = useI18n()
  return (
    <Row
      label={label}
      desc={desc}
      control={
        <button
          onClick={onPick}
          title={value || placeholder}
          className="flex max-w-[260px] items-center gap-2 rounded-lg border border-white/10 bg-[#141419] px-3 py-1.5 text-[12px] text-white/70 outline-none transition-colors hover:border-white/25 hover:text-white"
        >
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className="shrink-0">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
          </svg>
          <span className="truncate">{value || placeholder || t("common.selecionar")}</span>
        </button>
      }
    />
  )
}

function NumberField({
  label,
  desc,
  value,
  min,
  max,
  onChange,
}: {
  label: string
  desc?: string
  value: number
  min: number
  max: number
  onChange: (v: number) => void
}) {
  return (
    <Row
      label={label}
      desc={desc}
      control={
        <input
          type="number"
          min={min}
          max={max}
          value={value}
          onChange={(e) => {
            const n = Number(e.target.value)
            if (Number.isFinite(n)) onChange(Math.min(max, Math.max(min, n)))
          }}
          className="w-20 rounded-lg border border-white/10 bg-[#141419] px-3 py-1.5 text-[13px] text-white/85 outline-none transition-colors hover:border-white/25 focus:border-[var(--accent)]"
        />
      }
    />
  )
}
