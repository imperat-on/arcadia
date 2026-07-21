"use client"

import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import type { AppConfig } from "../../global"
import { temaPorId } from "../../themes"

const ZOONS = [60, 80, 100, 120, 140, 160, 180, 200]

// Nomes de fonte não se traduzem; só as duas entradas com texto descritivo
// passam pelo dicionário.
const FONTES: { id: string; label?: string; labelKey?: string }[] = [
  { id: "Inter", labelKey: "accessibility.fonte.inter" },
  { id: "Cabin", label: "Cabin" },
  { id: "Rubik", label: "Rubik" },
  { id: "Ubuntu", label: "Ubuntu" },
  { id: "system-ui", labelKey: "accessibility.fonte.sistema" },
]

// Aplica o tema completo (cores de fundo/sidebar/cards/texto) + acessibilidade
// (fontes, animações) no documento. Chamado ao abrir o app e a cada mudança.
export function aplicarA11y(cfg: AppConfig) {
  const root = document.documentElement
  const tema = temaPorId(cfg.theme_name)

  // Variáveis de cor consumidas pelo stylesheet dinâmico e por componentes
  // que usam var(--accent) diretamente.
  root.style.setProperty("--accent", tema.accent)
  root.style.setProperty("--bg", tema.bg)
  root.style.setProperty("--sidebar-bg", tema.sidebar)
  root.style.setProperty("--card-bg", tema.card)
  root.style.setProperty("--text", tema.text)
  root.style.setProperty("--muted", tema.muted)

  const conteudo = cfg.content_font && cfg.content_font !== "Inter" ? cfg.content_font : "Inter"
  root.style.setProperty("font-family", `'${conteudo}', 'Inter', sans-serif`)
  const acoes = cfg.actions_font && cfg.actions_font !== "Rubik" ? cfg.actions_font : "Rubik"

  let style = document.getElementById("a11y-style") as HTMLStyleElement | null
  if (!style) {
    style = document.createElement("style")
    style.id = "a11y-style"
    document.head.appendChild(style)
  }
  style.textContent = `
    body, #root { background: var(--bg) !important; color: var(--text) !important; }
    /* Fundos utilitários comuns passam a respeitar o tema */
    .bg-black { background: var(--bg) !important; }
    .bg-\\[\\#0d0d0f\\], .bg-\\[\\#101014\\] { background: var(--sidebar-bg) !important; }
    .bg-\\[\\#141419\\], .bg-\\[\\#16161a\\] { background: var(--card-bg) !important; }
    /* Texto muted padrão do app */
    .text-\\[\\#8a93a6\\], .text-\\[\\#a8b3cc\\] { color: var(--muted) !important; }
    /* Inputs e selects escuros seguem o card do tema */
    input, select, textarea { color-scheme: dark; }
    button, select, input, [role="button"] { font-family: '${acoes}', '${conteudo}', sans-serif; }
    ${cfg.no_smooth_scroll ? "* { scroll-behavior: auto !important; }" : ""}
    ${cfg.no_anim ? "*, *::before, *::after { animation: none !important; transition: none !important; }" : ""}
  `
}

// Aba Acessibilidade (modo desktop): zoom, fontes, tema, CSS custom e toggles.
export function AccessibilityView() {
  const { t } = useI18n()
  const [cfg, setCfg] = useState<AppConfig>({})

  useEffect(() => {
    window.launcherAPI?.getConfig().then((c) => {
      setCfg(c || {})
      aplicarA11y(c || {})
    })
  }, [])

  const salvar = (patch: AppConfig) => {
    const next = { ...cfg, ...patch }
    setCfg(next)
    window.launcherAPI?.setConfig(patch)
    aplicarA11y(next)
  }

  const zoom = Math.round((cfg.ui_scale ?? 1) * 100)
  // Slider de zoom: aplica só ao SOLTAR (aplicar a cada mousemove dava aquela
  // "tremida" — a janela re-renderizava com a escala nova no meio do arrasto).
  const [zoomDraft, setZoomDraft] = useState<number | null>(null)
  const zoomTela = zoomDraft ?? zoom
  const aplicarZoom = (v: number) => {
    const z = ZOONS[Math.max(0, Math.min(ZOONS.length - 1, v))] / 100
    salvar({ ui_scale: z })
    window.launcherAPI?.setZoom(z)
    setZoomDraft(null)
  }

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <h1 className="mb-8 text-2xl font-light tracking-wide text-white">{t("accessibility.titulo")}</h1>

      <div className="max-w-2xl space-y-8 pb-10">
        {/* Zoom */}
        <section>
          <div className="mb-2 flex items-center justify-between">
            <h2 className="text-sm font-semibold text-[#a8b3cc]">{t("accessibility.zoom")}</h2>
            <span className="text-sm font-bold tabular-nums text-white">{zoomTela}%</span>
          </div>
          <input
            type="range"
            min={0}
            max={ZOONS.length - 1}
            step={1}
            value={Math.max(0, ZOONS.indexOf(zoomTela))}
            onChange={(e) => setZoomDraft(ZOONS[Number(e.target.value)])}
            onMouseUp={(e) => aplicarZoom(Number((e.target as HTMLInputElement).value))}
            onTouchEnd={(e) => aplicarZoom(Number((e.target as HTMLInputElement).value))}
            onKeyUp={(e) => aplicarZoom(Number((e.target as HTMLInputElement).value))}
            className="w-full"
            style={{ accentColor: "var(--accent)" }}
          />
          <div className="mt-1 flex justify-between text-[10px] text-white/35">
            {ZOONS.map((z) => (
              <span key={z} className={z === zoomTela ? "font-bold text-white" : ""}>{z}</span>
            ))}
          </div>
        </section>

        {/* Fontes */}
        <section>
          <h2 className="mb-3 text-sm font-semibold text-[#a8b3cc]">{t("accessibility.fontes")}</h2>
          <div className="space-y-3">
            <SelectFonte
              label={t("accessibility.fonte_conteudo")}
              value={cfg.content_font || "Inter"}
              onChange={(v) => salvar({ content_font: v })}
            />
            <SelectFonte
              label={t("accessibility.fonte_acoes")}
              value={cfg.actions_font || "Rubik"}
              onChange={(v) => salvar({ actions_font: v })}
            />
          </div>
        </section>

        {/* CSS customizado */}
        <section>
          <h2 className="mb-1 text-sm font-semibold text-[#a8b3cc]">{t("accessibility.caminho_temas")}</h2>
          <p className="mb-2 text-xs text-white/40">
            {t("accessibility.pasta_css_desc")}
          </p>
          <div className="flex gap-2">
            <input
              value={cfg.custom_css_path || ""}
              onChange={(e) => salvar({ custom_css_path: e.target.value })}
              placeholder="/home/voce/temas-arcadia"
              spellCheck={false}
              className="flex-1 rounded-xl border border-white/10 bg-white/[0.04] px-4 py-2.5 text-sm text-white outline-none transition-colors focus:border-[color:var(--accent)]"
            />
            <button
              onClick={async () => {
                const r = await window.launcherAPI?.pickFolder?.()
                if (r?.ok && r.path) salvar({ custom_css_path: r.path })
              }}
              title={t("common.escolher_pasta")}
              className="rounded-xl border border-white/10 bg-white/[0.06] px-3.5 text-white/80 transition-colors hover:bg-white/10"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
              </svg>
            </button>
          </div>
        </section>

        {/* Links */}
        <section className="flex gap-3">
          <button
            onClick={() => window.launcherAPI?.openExternal(`file://${window.launcherPaths?.dataDir}/README.md`)}
            className="rounded-xl border border-white/10 px-4 py-2 text-xs font-medium text-white/70 transition-colors hover:bg-white/10 hover:text-white"
          >
            {t("accessibility.ajuda_readme")}
          </button>
          <button
            onClick={() => window.launcherAPI?.openExternal("https://www.protondb.com")}
            className="rounded-xl border border-white/10 px-4 py-2 text-xs font-medium text-white/70 transition-colors hover:bg-white/10 hover:text-white"
          >
            {t("accessibility.aviso_protondb")}
          </button>
        </section>

        {/* Checkboxes */}
        <section className="space-y-2.5">
          <Check
            label={t("accessibility.blocos_coloridos")}
            checked={Boolean(cfg.tiles_color)}
            onChange={(v) => salvar({ tiles_color: v })}
          />
          <Check
            label={t("accessibility.sempre_titulos")}
            checked={cfg.always_titles !== false}
            onChange={(v) => salvar({ always_titles: v })}
          />
          <Check
            label={t("accessibility.nao_fechar_dialogos")}
            checked={Boolean(cfg.no_click_outside)}
            onChange={(v) => salvar({ no_click_outside: v })}
          />
          <Check
            label={t("accessibility.desativar_rolagem")}
            checked={Boolean(cfg.no_smooth_scroll)}
            onChange={(v) => salvar({ no_smooth_scroll: v })}
          />
          <Check
            label={t("accessibility.desativar_animacoes")}
            checked={Boolean(cfg.no_anim)}
            onChange={(v) => salvar({ no_anim: v })}
          />
        </section>
      </div>
    </div>
  )
}

function SelectFonte({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  const { t } = useI18n()
  return (
    <label className="flex items-center justify-between gap-4 rounded-xl border border-white/[0.08] bg-white/[0.02] px-4 py-3">
      <span className="text-sm text-white/80">{label}</span>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="rounded-lg border border-white/10 bg-[#16161a] px-3 py-1.5 text-sm text-white outline-none focus:border-[color:var(--accent)]"
      >
        {FONTES.map((f) => (
          <option key={f.id} value={f.id}>{f.labelKey ? t(f.labelKey) : f.label}</option>
        ))}
      </select>
    </label>
  )
}

function Check({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      onClick={() => onChange(!checked)}
      className="flex w-full items-center gap-3 rounded-xl border border-white/[0.08] bg-white/[0.02] px-4 py-3 text-left transition-colors hover:bg-white/[0.04]"
      role="checkbox"
      aria-checked={checked}
    >
      <span
        className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-md border transition-colors ${
          checked ? "border-[color:var(--accent)] bg-[color:var(--accent)] text-black" : "border-white/25"
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
