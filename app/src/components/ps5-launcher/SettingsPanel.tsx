"use client"

import { useEffect, useRef, useState } from "react"
import type { AppConfig, IntegrationsStatus } from "../../global"
import { useGamepadNav } from "./useGamepadNav"

interface SettingsPanelProps {
  open: boolean
  onClose: () => void
  onSaved: () => void // refresh da biblioteca
  onUiChange?: (c: { card_scale?: number; accent?: string }) => void
}

type Section = "temas"

export function SettingsPanel({
  open,
  onClose,
  onSaved,
  onUiChange,
}: SettingsPanelProps) {
  const [section, setSection] = useState<Section>("temas")
  const [cfg, setCfg] = useState<AppConfig>({})
  const rootRef = useRef<HTMLDivElement>(null)
  useGamepadNav(rootRef, open, onClose)

  useEffect(() => {
    if (!open) return
    window.launcherAPI?.getConfig().then((c) => setCfg(c || {}))
  }, [open])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose()
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, onClose])

  if (!open) return null

  const NAV: { id: Section; label: string; icon: JSX.Element }[] = [
    { id: "temas", label: "Temas", icon: <IconTheme /> },
  ]

  return (
    <div
      ref={rootRef}
      className="gp-scope fixed inset-0 z-50 flex bg-black/90 backdrop-blur-2xl"
    >
      {/* Sidebar */}
      <aside className="flex w-72 shrink-0 flex-col gap-1 border-r border-white/[0.06] bg-black/40 p-6">
        <div className="mb-6 flex items-center gap-2 px-2 text-[11px] font-medium uppercase tracking-[0.24em] text-white/50">
          <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--accent)" }} />
          Configurações
        </div>
        {NAV.map((n) => {
          const active = section === n.id
          return (
            <button
              key={n.id}
              onClick={() => setSection(n.id)}
              className="relative flex items-center gap-3 rounded-xl px-4 py-3 text-left text-[15px] transition-colors"
              style={{
                color: active ? "#ffffff" : "rgba(255,255,255,0.45)",
                background: active ? "rgba(255,255,255,0.06)" : "transparent",
                fontWeight: active ? 500 : 400,
              }}
            >
              {active && (
                <span className="absolute left-0 top-1/2 h-5 w-0.5 -translate-y-1/2 rounded-full" style={{ background: "var(--accent)" }} />
              )}
              <span className="opacity-80">{n.icon}</span>
              {n.label}
            </button>
          )
        })}
        <div className="mt-auto">
          <button
            onClick={onClose}
            className="w-full rounded-xl border border-white/10 px-4 py-3 text-sm font-medium text-white/60 transition-colors hover:bg-white/5 hover:text-white"
          >
            Fechar (Esc)
          </button>
        </div>
      </aside>

      {/* Conteúdo */}
      <main className="flex-1 overflow-y-auto p-10">
        {section === "temas" && (
          <ThemeSection
            scale={cfg.console_ui_scale ?? 1}
            cardScale={cfg.card_scale ?? 1}
            accent={cfg.accent ?? "#00a8ff"}
            onScale={(z) => {
              setCfg((c) => ({ ...c, console_ui_scale: z }))
              window.launcherAPI?.setConfig({ console_ui_scale: z })
              window.launcherAPI?.setZoom(z)
            }}
            onCardScale={(z) => {
              setCfg((c) => ({ ...c, card_scale: z }))
              window.launcherAPI?.setConfig({ card_scale: z })
              onUiChange?.({ card_scale: z, accent: cfg.accent ?? "#00a8ff" })
            }}
            onAccent={(hex) => {
              setCfg((c) => ({ ...c, accent: hex }))
              window.launcherAPI?.setConfig({ accent: hex })
              onUiChange?.({ card_scale: cfg.card_scale ?? 1, accent: hex })
            }}
          />
        )}
      </main>
    </div>
  )
}


/* --------------------------------------------------------------------- */
/* Integrações                                                           */
/* --------------------------------------------------------------------- */
function Toggle({
  on,
  onChange,
}: {
  on: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <button
      onClick={() => onChange(!on)}
      className="relative w-11 h-6 rounded-full transition-colors shrink-0"
      style={{ background: on ? "var(--accent)" : "rgba(255,255,255,0.15)" }}
      title={on ? "Ativado" : "Desativado"}
    >
      <span
        className="absolute top-0.5 w-5 h-5 rounded-full bg-white transition-all"
        style={{ left: on ? "22px" : "2px" }}
      />
    </button>
  )
}

export function IntegrationsSection({
  cfg,
  status,
  onSaveKey,
  onToggle,
  onSlsPath,
}: {
  cfg: AppConfig
  status: IntegrationsStatus | null
  onSaveKey: (key: string, id: string) => void
  onToggle: (name: "steam" | "heroic" | "lutris" | "slssteam", val: boolean) => void
  onSlsPath: (path: string) => void
}) {
  const [apiKey, setApiKey] = useState(cfg.steam_api_key ?? "")
  const [steamId, setSteamId] = useState(cfg.steam_id64 ?? "")
  const [slsPath, setSlsPath] = useState(cfg.slssteam_path ?? "")
  const [saved, setSaved] = useState(false)
  const [pathSaved, setPathSaved] = useState(false)
  const [legendary, setLegendary] = useState<{ installed: boolean; logged: boolean; user?: string } | null>(null)
  const [legendaryBusy, setLegendaryBusy] = useState(false)
  const [legendaryErr, setLegendaryErr] = useState("")
  const src = cfg.sources ?? {}
  const on = (k: "steam" | "heroic" | "lutris" | "slssteam") => src[k] !== false

  useEffect(() => {
    setApiKey(cfg.steam_api_key ?? "")
    setSteamId(cfg.steam_id64 ?? "")
    setSlsPath(cfg.slssteam_path ?? "")
    window.launcherAPI?.legendaryStatus().then(setLegendary)
  }, [cfg.steam_api_key, cfg.steam_id64, cfg.slssteam_path])

  return (
    <div className="max-w-3xl">
      <h2 className="text-3xl font-light tracking-wide text-white mb-1">Integrações</h2>
      <p className="text-sm text-[#8a93a6] mb-8">
        Fontes de jogos. O que estiver desativado não aparece na biblioteca.
      </p>

      {/* Steam */}
      <IntegrationCard
        title="Steam"
        connected={status?.steam ?? Boolean(cfg.steam_api_key)}
        enabled={on("steam")}
        onToggle={(v) => onToggle("steam", v)}
      >
        <p className="text-xs text-[#8a93a6] mb-3">
          Chave da Steam Web API — carrega a biblioteca inteira, inclusive
          jogos não instalados. Obtenha em steamcommunity.com/dev/apikey.
        </p>
        <input
          type="password"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          placeholder="Chave da Steam Web API"
          spellCheck={false}
          className="w-full px-4 py-2.5 rounded-xl text-white text-sm outline-none transition-colors focus:border-[color:var(--accent)] mb-2"
          style={{ background: "rgba(0,0,0,0.4)", border: "1px solid rgba(255,255,255,0.12)" }}
        />
        <input
          value={steamId}
          onChange={(e) => setSteamId(e.target.value)}
          placeholder="SteamID64 (opcional — detectado automaticamente)"
          spellCheck={false}
          className="w-full px-4 py-2.5 rounded-xl text-white text-sm outline-none transition-colors focus:border-[color:var(--accent)] mb-3"
          style={{ background: "rgba(0,0,0,0.4)", border: "1px solid rgba(255,255,255,0.12)" }}
        />
        <button
          onClick={() => {
            onSaveKey(apiKey.trim(), steamId.trim())
            setSaved(true)
            setTimeout(() => setSaved(false), 1500)
          }}
          className="px-5 py-2 rounded-xl bg-white text-sm font-semibold text-black transition-transform hover:scale-[1.03]"
        >
          {saved ? "Salvo!" : "Salvar e sincronizar"}
        </button>
      </IntegrationCard>

      {/* Epic (via Legendary) */}
      <IntegrationCard
        title="Epic Games (via Legendary)"
        connected={Boolean(legendary?.logged)}
        enabled={on("heroic")}
        onToggle={(v) => onToggle("heroic", v)}
      >
        <p className="mb-3 text-xs text-[#8a93a6]">
          O <b>Legendary</b> é o motor open-source da Epic (o mesmo do Heroic).
          O Arcadia baixa o binário oficial e o login abre num terminal — depois
          disso seus jogos Epic aparecem na biblioteca ao atualizar.
          {legendary?.logged && legendary.user ? (
            <>
              {" "}Logado como <span className="text-white">{legendary.user}</span>.
            </>
          ) : null}
        </p>
        <button
          onClick={async () => {
            setLegendaryBusy(true)
            setLegendaryErr("")
            const r = await window.launcherAPI?.legendarySetup()
            setLegendaryBusy(false)
            if (r?.ok) {
              window.launcherAPI?.legendaryStatus().then(setLegendary)
            } else {
              setLegendaryErr(r?.error || "falha desconhecida")
            }
          }}
          disabled={legendaryBusy}
          className="rounded-xl bg-white px-5 py-2 text-sm font-semibold text-black transition-transform hover:scale-[1.03] disabled:opacity-60"
        >
          {legendaryBusy
            ? "Preparando…"
            : legendary?.installed
              ? legendary.logged
                ? "Refazer login Epic"
                : "Fazer login Epic (terminal)"
              : "Baixar Legendary e fazer login"}
        </button>
        {legendaryErr && <p className="mt-2 text-xs text-[#ff6b81]">{legendaryErr}</p>}
        <p className="mt-2 text-[11px] text-[#6b7280]">
          O Legendary usa a mesma sessão do Heroic (~/.config/legendary). Se já
          estiver logado lá, não precisa logar de novo.
        </p>
      </IntegrationCard>

      {/* SLSsteam */}
      <IntegrationCard
        title="SLSsteam"
        connected={(status?.slssteam ?? 0) > 0}
        enabled={on("slssteam")}
        onToggle={(v) => onToggle("slssteam", v)}
      >
        <p className="text-xs text-[#8a93a6] mb-3">
          {status
            ? `${status.slssteam} jogo(s) injetado(s) detectado(s).`
            : "Lendo configuração…"}{" "}
          O SLSsteam injeta jogos na Steam pelo bloco{" "}
          <span className="text-white">AdditionalApps</span> do arquivo{" "}
          <span className="text-white">config.yaml</span>. Tudo que você adicionar
          via LuaTools entra na biblioteca ao atualizar.
        </p>
        <div
          className="rounded-lg p-3 mb-3 text-xs text-[#8a93a6]"
          style={{ background: "rgba(0,0,0,0.3)", border: "1px solid rgba(255,255,255,0.08)" }}
        >
          <div className="text-[#a8b3cc] font-semibold mb-1">
            Onde fica o config.yaml?
          </div>
          Por padrão em{" "}
          <span className="text-white">~/.config/SLSsteam/config.yaml</span>. Se o
          seu SLSsteam guarda esse arquivo em{" "}
          <span className="text-white">outra pasta</span>, cole o caminho completo
          abaixo para o Arcadia ler de lá.
        </div>
        <label className="block text-[11px] font-semibold text-[#8a93a6] mb-1.5 uppercase tracking-wider">
          Caminho do config.yaml (opcional)
        </label>
        <div className="flex gap-2">
          <input
            value={slsPath}
            onChange={(e) => setSlsPath(e.target.value)}
            placeholder="/home/voce/.config/SLSsteam/config.yaml"
            spellCheck={false}
            className="flex-1 px-4 py-2.5 rounded-xl text-white text-sm outline-none transition-colors focus:border-[color:var(--accent)]"
            style={{ background: "rgba(0,0,0,0.4)", border: "1px solid rgba(255,255,255,0.12)" }}
          />
          <button
            onClick={() => {
              onSlsPath(slsPath.trim())
              setPathSaved(true)
              setTimeout(() => setPathSaved(false), 1500)
            }}
            className="px-4 py-2 rounded-xl bg-white text-sm font-semibold text-black transition-transform hover:scale-[1.03] shrink-0"
          >
            {pathSaved ? "Salvo!" : "Salvar"}
          </button>
        </div>
        <p className="text-[11px] text-[#6b7280] mt-2">
          Deixe vazio para usar o caminho padrão.
        </p>
      </IntegrationCard>
    </div>
  )
}

function IntegrationCard({
  title,
  connected,
  enabled,
  onToggle,
  children,
}: {
  title: string
  connected: boolean
  enabled: boolean
  onToggle: (v: boolean) => void
  children: React.ReactNode
}) {
  return (
    <div
      className="mb-4 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5"
    >
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-base font-semibold text-white">{title}</h3>
        <div className="flex items-center gap-3">
          {/* Status como texto: a bolinha da marca e o fundo do badge não
              carregavam informação nenhuma que a palavra já não desse. */}
          <span
            className="text-xs font-medium"
            style={{ color: connected ? "#4adf9a" : "#8a93a6" }}
          >
            {connected ? "Conectado" : "Não conectado"}
          </span>
          <Toggle on={enabled} onChange={onToggle} />
        </div>
      </div>
      <div style={{ opacity: enabled ? 1 : 0.45 }}>{children}</div>
    </div>
  )
}

/* --------------------------------------------------------------------- */
/* Metadados                                                             */
/* --------------------------------------------------------------------- */
const ACCENTS = [
  { name: "Azul PS", hex: "#00a8ff" },
  { name: "Roxo", hex: "#a06bff" },
  { name: "Verde", hex: "#3ddc84" },
  { name: "Vermelho", hex: "#ff5d5d" },
  { name: "Laranja", hex: "#ff9f1c" },
  { name: "Rosa", hex: "#ff5da2" },
  { name: "Ciano", hex: "#22d3ee" },
  { name: "Dourado", hex: "#ffd23f" },
]

function ScaleControl({
  label,
  value,
  onChange,
  presets,
}: {
  label: string
  value: number
  onChange: (z: number) => void
  presets: { label: string; z: number }[]
}) {
  return (
    <div className="mb-10">
      <div className="flex items-center justify-between mb-3">
        <span className="text-sm font-semibold text-[#a8b3cc]">{label}</span>
        <span className="text-lg font-bold text-white tabular-nums">
          {Math.round(value * 100)}%
        </span>
      </div>
      <input
        type="range"
        min={0.8}
        max={1.6}
        step={0.05}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="w-full mb-4"
        style={{ accentColor: "var(--accent)" }}
      />
      <div className="grid grid-cols-5 gap-3">
        {presets.map((p) => {
          const active = Math.abs(p.z - value) < 0.03
          return (
            <button
              key={p.label}
              onClick={() => onChange(p.z)}
              className="flex flex-col items-center py-2.5 rounded-xl transition-colors"
              style={{
                background: active ? "rgba(255,255,255,0.10)" : "rgba(255,255,255,0.04)",
                border: `1px solid ${active ? "var(--accent)" : "rgba(255,255,255,0.08)"}`,
              }}
            >
              <span className="text-sm font-semibold text-white">{p.label}</span>
              <span className="text-xs text-[#8a93a6]">{Math.round(p.z * 100)}%</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

export function ThemeSection({
  scale,
  cardScale,
  accent,
  onScale,
  onCardScale,
  onAccent,
}: {
  scale: number
  cardScale: number
  accent: string
  onScale: (z: number) => void
  onCardScale: (z: number) => void
  onAccent: (hex: string) => void
}) {
  return (
    <div className="max-w-2xl">
      <h2 className="text-3xl font-light tracking-wide text-white mb-1">Temas</h2>
      <p className="text-sm text-[#8a93a6] mb-8">
        Escala e cores da interface.
      </p>

      <ScaleControl
        label="Escala da interface (tudo)"
        value={scale}
        onChange={onScale}
        presets={[
          { label: "Pequeno", z: 0.9 },
          { label: "Padrão", z: 1.0 },
          { label: "Grande", z: 1.15 },
          { label: "Enorme", z: 1.3 },
          { label: "Gigante", z: 1.5 },
        ]}
      />

      <ScaleControl
        label="Tamanho das capas"
        value={cardScale}
        onChange={onCardScale}
        presets={[
          { label: "Compacto", z: 0.85 },
          { label: "Padrão", z: 1.0 },
          { label: "Médio", z: 1.2 },
          { label: "Grande", z: 1.4 },
          { label: "Enorme", z: 1.6 },
        ]}
      />

      {/* Cor de destaque */}
      <div className="mb-4">
        <span className="text-sm font-semibold text-[#a8b3cc]">Cor de destaque</span>
        <div className="flex flex-wrap gap-3 mt-3">
          {ACCENTS.map((a) => {
            const active = a.hex.toLowerCase() === accent.toLowerCase()
            return (
              <button
                key={a.hex}
                onClick={() => onAccent(a.hex)}
                title={a.name}
                className="w-11 h-11 rounded-full transition-transform hover:scale-110"
                style={{
                  background: a.hex,
                  border: active ? "3px solid #ffffff" : "3px solid rgba(255,255,255,0.15)",
                  boxShadow: active ? `0 0 14px ${a.hex}` : "none",
                }}
              />
            )
          })}
        </div>
      </div>

      <p className="text-xs text-[#6b7280] mt-6">
        Aplicado na hora e salvo automaticamente. Em 4K, escala e capas em
        <span className="text-white"> Grande</span>.
      </p>
    </div>
  )
}

export function MetadataSection({ onSaved }: { onSaved: () => void }) {
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState(false)
  const [sgdbKey, setSgdbKey] = useState("")
  const [keySaved, setKeySaved] = useState(false)
  const [igdbId, setIgdbId] = useState("")
  const [igdbSecret, setIgdbSecret] = useState("")
  const [igdbSaved, setIgdbSaved] = useState(false)
  const [trailerAuto, setTrailerAuto] = useState(true)
  const [cookies, setCookies] = useState("")
  const [dlAll, setDlAll] = useState<{ done: number; total: number; title: string } | null>(null)
  const [scheevo, setScheevo] = useState<{ installed: boolean; schemas: number } | null>(null)
  const [scheevoBusy, setScheevoBusy] = useState(false)
  const [scheevoErr, setScheevoErr] = useState("")

  // As chaves vivem no config.json; aqui só editamos.
  useEffect(() => {
    window.launcherAPI?.getConfig().then((c) => {
      setSgdbKey(c?.steamgriddb_api_key ?? "")
      setIgdbId(c?.igdb_client_id ?? "")
      setIgdbSecret(c?.igdb_client_secret ?? "")
      setTrailerAuto(c?.trailer_auto !== false)
      setCookies(c?.youtube_cookies ?? "")
    })
    window.launcherAPI?.slscheevoStatus().then(setScheevo)
  }, [])

  // Progresso do "baixar todos os trailers".
  useEffect(() => {
    return window.launcherAPI?.onTrailerProgress((d) => {
      setDlAll(d.total && d.done < d.total ? d : null)
    })
  }, [])

  const baixarTodos = async () => {
    setDlAll({ done: 0, total: 0, title: "iniciando…" })
    const r = await window.launcherAPI?.trailerDownloadAll()
    setDlAll(null)
    setDone(true)
    setTimeout(() => setDone(false), 3000)
    return r
  }

  const saveKey = async () => {
    await window.launcherAPI?.setConfig({ steamgriddb_api_key: sgdbKey.trim() })
    setKeySaved(true)
    setTimeout(() => setKeySaved(false), 1500)
  }

  const saveIgdb = async () => {
    await window.launcherAPI?.setConfig({
      igdb_client_id: igdbId.trim(),
      igdb_client_secret: igdbSecret.trim(),
    })
    setIgdbSaved(true)
    setTimeout(() => setIgdbSaved(false), 1500)
  }

  const rebuild = async () => {
    setBusy(true)
    setDone(false)
    await window.launcherAPI?.rebuildMeta()
    onSaved()
    setBusy(false)
    setDone(true)
    setTimeout(() => setDone(false), 2500)
  }

  const items = [
    { label: "Capa vertical", on: true },
    { label: "Banner / imagem-tema (hero)", on: true },
    { label: "Logo transparente", on: true },
    { label: "Descrição curta", on: true },
    { label: "Nota (Metacritic → estrelas)", on: true },
    { label: "Gênero e ano", on: true },
    { label: "Trailers (do YouTube)", on: true },
  ]

  return (
    <div className="max-w-3xl">
      <h2 className="text-3xl font-light tracking-wide text-white mb-1">Metadados</h2>
      <p className="text-sm text-[#8a93a6] mb-8">
        Capas, descrições e conquistas, puxados da Steam Store e cacheados.
      </p>

      {/* SLScheevo: conquistas funcionais nos jogos injetados via SLSsteam */}
      <div className="mb-8 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5">
        <div className="mb-3 flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <span className="h-2.5 w-2.5 rounded-full" style={{ background: "#3ddc84" }} />
            <h3 className="text-base font-semibold text-white">Conquistas em jogos injetados</h3>
          </div>
          <span
            className="rounded-full px-2.5 py-1 text-xs font-semibold"
            style={{
              color: scheevo && scheevo.schemas > 0 ? "#4adf9a" : "#8a93a6",
              background: scheevo && scheevo.schemas > 0 ? "rgba(74,223,154,0.12)" : "rgba(255,255,255,0.05)",
            }}
          >
            {scheevo == null
              ? "Verificando…"
              : scheevo.schemas > 0
                ? `Ativo (${scheevo.schemas} jogos)`
                : scheevo.installed
                  ? "Instalado — rode 1x"
                  : "Não configurado"}
          </span>
        </div>
        <p className="mb-3 text-xs text-[#8a93a6]">
          Jogos adicionados via <b>SLSsteam</b> não têm progresso de conquistas na
          Steam Web. O <b>SLScheevo</b> resolve isso: ele gera os arquivos de
          stats locais e o Arcadia passa a mostrar suas conquistas desbloqueadas
          de verdade no overview. É um app separado (código aberto) que o Arcadia
          baixa para você — ele abre num terminal e pede seu <b>login Steam</b>
          uma única vez.
        </p>
        <button
          onClick={async () => {
            setScheevoBusy(true)
            setScheevoErr("")
            const r = await window.launcherAPI?.slscheevoSetup()
            setScheevoBusy(false)
            if (r?.ok) {
              onSaved() // reindexa já aplicando os schemas existentes
              window.launcherAPI?.slscheevoStatus().then(setScheevo)
            } else {
              setScheevoErr(r?.error || "falha desconhecida")
            }
          }}
          disabled={scheevoBusy}
          className="rounded-xl bg-white px-5 py-2 text-sm font-semibold text-black transition-transform hover:scale-[1.03] disabled:opacity-60"
        >
          {scheevoBusy
            ? "Preparando…"
            : scheevo?.installed
              ? "Abrir SLScheevo (terminal)"
              : "Baixar e configurar SLScheevo"}
        </button>
        {scheevoErr && <p className="mt-2 text-xs text-[#ff6b81]">{scheevoErr}</p>}
        <p className="mt-2 text-[11px] text-[#6b7280]">
          Depois de logar e gerar os schemas, rode “Reconstruir metadados” ou
          reabra o Arcadia.
        </p>
      </div>

      {/* Chave do SteamGridDB: libera a busca de arte em "Editar metadados" */}
      <div
        className="mb-8 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5"
      >
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2.5">
            <span className="w-2.5 h-2.5 rounded-full" style={{ background: "#9b6bff" }} />
            <h3 className="text-base font-semibold text-white">SteamGridDB</h3>
          </div>
          <span
            className="text-xs font-semibold px-2.5 py-1 rounded-full"
            style={{
              color: sgdbKey ? "#4adf9a" : "#8a93a6",
              background: sgdbKey ? "rgba(74,223,154,0.12)" : "rgba(255,255,255,0.05)",
            }}
          >
            {sgdbKey ? "Conectado" : "Sem chave"}
          </span>
        </div>
        <p className="text-xs text-[#8a93a6] mb-3">
          Libera o <b>Buscar online</b> em “Editar metadados”: capas, fundos
          (inclusive <b>animados</b>) e logos feitos pela comunidade, para
          qualquer jogo — não só os da Steam. A chave é gratuita: pegue em
          steamgriddb.com/profile/preferences/api.
        </p>
        <input
          type="password"
          value={sgdbKey}
          onChange={(e) => setSgdbKey(e.target.value)}
          placeholder="Chave da API do SteamGridDB"
          spellCheck={false}
          className="w-full px-4 py-2.5 rounded-xl text-white text-sm outline-none transition-colors focus:border-[color:var(--accent)] mb-3"
          style={{ background: "rgba(0,0,0,0.4)", border: "1px solid rgba(255,255,255,0.12)" }}
        />
        <button
          onClick={saveKey}
          className="px-5 py-2 rounded-xl bg-white text-sm font-semibold text-black transition-transform hover:scale-[1.03]"
        >
          {keySaved ? "Salvo!" : "Salvar chave"}
        </button>
      </div>

      {/* Trailers: baixados do YouTube (yt-dlp) e tocados no fundo (estilo PS5) */}
      <div
        className="mb-8 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5"
      >
        <div className="flex items-center gap-2.5 mb-3">
          <span className="w-2.5 h-2.5 rounded-full" style={{ background: "#ff4d6d" }} />
          <h3 className="text-base font-semibold text-white">Trailers</h3>
        </div>
        <p className="text-xs text-[#8a93a6] mb-4">
          Os trailers vêm do <b>YouTube</b> e ficam salvos localmente. Ao focar um
          jogo por um instante, o trailer toca no fundo — como no PS5.
        </p>

        <div className="flex items-center justify-between mb-4">
          <span className="text-sm text-white">Tocar trailer no fundo automaticamente</span>
          <Toggle
            on={trailerAuto}
            onChange={(v) => {
              setTrailerAuto(v)
              window.launcherAPI?.setConfig({ trailer_auto: v })
            }}
          />
        </div>

        <button
          onClick={baixarTodos}
          disabled={Boolean(dlAll)}
          className="px-5 py-2 rounded-xl bg-white text-sm font-semibold text-black transition-transform hover:scale-[1.03] disabled:opacity-60"
        >
          {dlAll ? "Baixando…" : "Baixar todos os trailers"}
        </button>
        {dlAll && (
          <p className="text-xs text-[#8a93a6] mt-3">
            {dlAll.total ? `${dlAll.done}/${dlAll.total}` : ""}{" "}
            {dlAll.title && `— ${dlAll.title}`}
          </p>
        )}
        <p className="text-[11px] text-[#6b7280] mt-2">
          Baixa apenas os que faltam. Cada trailer tem alguns MB.
        </p>

        {/* Cookies do YouTube: só para vídeos com restrição de idade */}
        <div className="mt-5 pt-5" style={{ borderTop: "1px solid rgba(255,255,255,0.08)" }}>
          <div className="flex items-center justify-between gap-3 mb-2">
            <span className="text-sm text-white">Cookies do YouTube (restrição de idade)</span>
            <span
              className="text-xs font-semibold px-2.5 py-1 rounded-full shrink-0"
              style={{
                color: cookies ? "#4adf9a" : "#8a93a6",
                background: cookies ? "rgba(74,223,154,0.12)" : "rgba(255,255,255,0.05)",
              }}
            >
              {cookies ? "Configurado" : "Não usado"}
            </span>
          </div>
          <p className="text-xs text-[#8a93a6] mb-3">
            Alguns trailers oficiais têm <b>restrição de idade</b> e o YouTube exige
            login. Para esses, exporte um arquivo <b>cookies.txt</b> e selecione aqui.
            A maioria dos vídeos <b>não</b> precisa disso.
          </p>
          <div
            className="rounded-lg p-3 mb-3 text-xs text-[#8a93a6]"
            style={{ background: "rgba(0,0,0,0.3)", border: "1px solid rgba(255,255,255,0.08)" }}
          >
            <div className="text-[#a8b3cc] font-semibold mb-1">Como pegar (2 min)</div>
            1. No seu navegador logado no YouTube, instale a extensão{" "}
            <span className="text-white">“Get cookies.txt LOCALLY”</span>.
            <br />
            2. Abra <span className="text-white">youtube.com</span> e clique na extensão
            → <span className="text-white">Export</span> (salva um{" "}
            <span className="text-white">cookies.txt</span>).
            <br />
            3. Clique em “Escolher arquivo” abaixo e aponte para esse arquivo.
          </div>
          <div className="flex items-center gap-3">
            <button
              onClick={async () => {
                const r = await window.launcherAPI?.trailerPickCookies()
                if (r?.ok && r.path) setCookies(r.path)
              }}
              className="rounded-xl border border-white/10 bg-white/[0.06] px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-white/10"
            >
              Escolher arquivo…
            </button>
            {cookies && (
              <>
                <span className="text-xs text-[#8a93a6] truncate flex-1">{cookies}</span>
                <button
                  onClick={() => {
                    setCookies("")
                    window.launcherAPI?.setConfig({ youtube_cookies: "" })
                  }}
                  className="text-xs text-[#ff6b81] shrink-0"
                >
                  Remover
                </button>
              </>
            )}
          </div>
        </div>
      </div>

      {/* Credenciais da IGDB: arte E descrição, para qualquer plataforma */}
      <div
        className="mb-8 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5"
      >
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2.5">
            <span className="w-2.5 h-2.5 rounded-full" style={{ background: "#9147ff" }} />
            <h3 className="text-base font-semibold text-white">IGDB</h3>
          </div>
          <span
            className="text-xs font-semibold px-2.5 py-1 rounded-full"
            style={{
              color: igdbId && igdbSecret ? "#4adf9a" : "#8a93a6",
              background: igdbId && igdbSecret ? "rgba(74,223,154,0.12)" : "rgba(255,255,255,0.05)",
            }}
          >
            {igdbId && igdbSecret ? "Conectado" : "Sem credenciais"}
          </span>
        </div>
        <p className="text-xs text-[#8a93a6] mb-3">
          A base que o Playnite usa por padrão: <b>descrição, capa e fundos</b>{" "}
          de qualquer plataforma — inclusive jogos de Heroic e Lutris, que a
          Steam não conhece. É gratuita: crie uma aplicação em
          dev.twitch.tv/console/apps e use o Client ID e o Client Secret dela.
        </p>
        <input
          type="password"
          value={igdbId}
          onChange={(e) => setIgdbId(e.target.value)}
          placeholder="Client ID (Twitch)"
          spellCheck={false}
          className="w-full px-4 py-2.5 rounded-xl text-white text-sm outline-none transition-colors focus:border-[color:var(--accent)] mb-2"
          style={{ background: "rgba(0,0,0,0.4)", border: "1px solid rgba(255,255,255,0.12)" }}
        />
        <input
          value={igdbSecret}
          onChange={(e) => setIgdbSecret(e.target.value)}
          placeholder="Client Secret (Twitch)"
          type="password"
          spellCheck={false}
          className="w-full px-4 py-2.5 rounded-xl text-white text-sm outline-none transition-colors focus:border-[color:var(--accent)] mb-3"
          style={{ background: "rgba(0,0,0,0.4)", border: "1px solid rgba(255,255,255,0.12)" }}
        />
        <button
          onClick={saveIgdb}
          className="px-5 py-2 rounded-xl bg-white text-sm font-semibold text-black transition-transform hover:scale-[1.03]"
        >
          {igdbSaved ? "Salvo!" : "Salvar credenciais"}
        </button>
      </div>

      <h3 className="text-base font-semibold text-white mb-1">O que é buscado</h3>
      <p className="text-xs text-[#8a93a6] mb-4">
        Coletados automaticamente para cada jogo.
      </p>
      <div className="grid grid-cols-2 gap-2.5 mb-8">
        {items.map((it) => (
          <div
            key={it.label}
            className="flex items-center gap-3 px-4 py-3 rounded-xl"
            style={{
              background: "rgba(255,255,255,0.035)",
              border: "1px solid rgba(255,255,255,0.06)",
            }}
          >
            <span className="text-sm text-white flex-1 leading-tight">{it.label}</span>
            <span
              className="flex items-center gap-1.5 text-[11px] font-semibold shrink-0"
              style={{ color: "#4adf9a" }}
            >
              <span className="w-1.5 h-1.5 rounded-full" style={{ background: "#4adf9a" }} />
              Ativo
            </span>
          </div>
        ))}
      </div>

      <button
        onClick={rebuild}
        disabled={busy}
        className="px-6 py-2.5 rounded-xl bg-white text-sm font-semibold text-black transition-transform hover:scale-[1.03] disabled:opacity-60"
      >
        {busy ? "Reconstruindo…" : done ? "Metadados atualizados!" : "Reconstruir metadados"}
      </button>
      <p className="text-xs text-[#6b7280] mt-3">
        Limpa o cache e busca tudo de novo. Leva cerca de 30s.
      </p>
    </div>
  )
}

/* --------------------------------------------------------------------- */
/* Ícones                                                                */
/* --------------------------------------------------------------------- */
function IconUser() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z" />
    </svg>
  )
}
function IconPlug() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
      <path d="M16 7V3h-2v4h-4V3H8v4H7a1 1 0 00-1 1v4a6 6 0 005 5.91V22h2v-4.09A6 6 0 0018 12V8a1 1 0 00-1-1h-1z" />
    </svg>
  )
}
function IconTheme() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 3a9 9 0 000 18c.83 0 1.5-.67 1.5-1.5 0-.39-.15-.74-.39-1.01-.23-.26-.38-.61-.38-.99 0-.83.67-1.5 1.5-1.5H16a5 5 0 005-5c0-4.42-4.03-8-9-8zm-5.5 9a1.5 1.5 0 110-3 1.5 1.5 0 010 3zm3-4a1.5 1.5 0 110-3 1.5 1.5 0 010 3zm5 0a1.5 1.5 0 110-3 1.5 1.5 0 010 3zm3.5 4a1.5 1.5 0 110-3 1.5 1.5 0 010 3z" />
    </svg>
  )
}

function IconTag() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
      <path d="M21.41 11.58l-9-9A2 2 0 0011 2H4a2 2 0 00-2 2v7a2 2 0 00.59 1.42l9 9a2 2 0 002.82 0l7-7a2 2 0 000-2.84zM6.5 8A1.5 1.5 0 118 6.5 1.5 1.5 0 016.5 8z" />
    </svg>
  )
}

function IconWine() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M8 22h8" />
      <path d="M12 15v7" />
      <path d="M5 3h14l-1.5 7.5a5.5 5.5 0 0 1-11 0L5 3z" />
    </svg>
  )
}
