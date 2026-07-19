"use client"

import { useEffect, useState } from "react"
import type { AppConfig } from "../../global"
import { aplicarA11y } from "./AccessibilityView"
import { TEMAS } from "../../themes"

// Configurações Globais → Config. Gerais (idioma, caminhos, comportamento,
// atalhos, biblioteca e downloads). Salva tudo no config.json via setConfig.

const LANGS = [
  { id: "pt-BR", label: "Português (Brasil)" },
  { id: "en-US", label: "English (US)" },
  { id: "es-ES", label: "Español" },
]

const FEATURED_OPTS = [
  { id: "disabled", label: "Desabilitada" },
  { id: "recent", label: "Jogados recentemente" },
  { id: "favorites", label: "Favoritos" },
  { id: "most-played", label: "Mais jogados" },
] as const

export function GeneralSection({ onSaved }: { onSaved: () => void }) {
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

  return (
    <div className="max-w-2xl">
      <h2 className="text-3xl font-light tracking-wide text-white mb-1">Config. Gerais</h2>
      <p className="text-sm text-[#8a93a6] mb-8">
        Preferências globais do Arcadia — idioma, pastas padrão, comportamento do app e integrações com o sistema.
      </p>

      <Group title="Idioma e aparência">
        <Select
          label="Selecione o idioma do app"
          desc="Altera o idioma de toda a interface do Arcadia."
          value={cfg.language ?? "pt-BR"}
          options={LANGS}
          onChange={(v) => set("language", v)}
        />
        <Select
          label="Selecione um tema"
          desc="Altera as cores de toda a interface — fundo, sidebar, cards e destaques. Aplicado na hora."
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
          label="Caminho para temas personalizados"
          desc="Pasta com arquivos CSS customizados (temas de terceiros)."
          value={cfg.custom_css_path ?? ""}
          placeholder="Nenhuma pasta selecionada"
          onPick={() => pickFolder("custom_css_path")}
        />
      </Group>

      <Group title="Caminhos padrão">
        <Path
          label="Local de instalação padrão"
          desc="Pasta onde os jogos serão baixados e instalados por padrão."
          value={cfg.default_install_path ?? `${home}/Games/Arcadia`}
          onPick={() => pickFolder("default_install_path")}
        />
        <Path
          label="Pasta padrão para novos prefixos Wine"
          desc="Onde o Arcadia criará os prefixos Wine necessários para rodar jogos de Windows."
          value={cfg.default_wine_prefix_path ?? `${home}/Games/Arcadia/Prefixes`}
          onPick={() => pickFolder("default_wine_prefix_path")}
        />
        <Path
          label="Caminho padrão do Steam"
          desc="Localização dos arquivos do seu Steam local (útil para sincronizações)."
          value={cfg.steam_path ?? `${home}/.steam/steam`}
          onPick={() => pickFolder("steam_path")}
        />
        <Path
          label="Sincronizar com a Epic Games (prefixo onde a EGS está instalada)"
          desc="Mapeia onde o inicializador oficial da Epic está instalado, se necessário."
          value={cfg.epic_egs_prefix ?? ""}
          placeholder="Nenhum prefixo selecionado"
          onPick={() => pickFolder("epic_egs_prefix")}
        />
      </Group>

      <Group title="Comportamento do aplicativo">
        <Toggle
          label="Verificar se há atualizações do Arcadia ao iniciar"
          desc="Procura automaticamente novas versões do programa ao abrir."
          value={cfg.check_updates_on_start !== false}
          onChange={(v) => set("check_updates_on_start", v)}
        />
        <Toggle
          label="Atualizar jogos automaticamente"
          desc="Faz o download e instalação automática de patches dos jogos instalados."
          value={cfg.auto_update_games === true}
          onChange={(v) => set("auto_update_games", v)}
        />
        <Toggle
          label="Não mostrar listas de mudanças ao inicializar"
          desc="Oculta a janela de novidades (changelog) após atualizações."
          value={cfg.hide_changelog_on_start === true}
          onChange={(v) => set("hide_changelog_on_start", v)}
        />
        <Toggle
          label="Iniciar em modo console"
          desc="Abre o Arcadia diretamente na interface otimizada para telas grandes e controle. Para voltar ao desktop: rode arcadia-desktop.sh --force-desktop."
          value={cfg.start_in_console_mode === true}
          onChange={(v) => set("start_in_console_mode", v)}
        />
        <Toggle
          label="Esconder ícone do indicador de aplicativo"
          desc="Oculta o ícone do Arcadia da bandeja do sistema (requer reiniciar o app)."
          value={cfg.hide_tray_icon === true}
          onChange={(v) => set("hide_tray_icon", v)}
        />
        <Toggle
          label="Fechar para o indicador de aplicativo"
          desc="Ao clicar no X, o Arcadia continua rodando em segundo plano na bandeja."
          value={cfg.close_to_tray === true}
          onChange={(v) => set("close_to_tray", v)}
        />
        <Toggle
          label="Iniciar minimizado"
          desc="Abre o Arcadia oculto na bandeja ao ligar o computador."
          value={cfg.start_minimized === true}
          disabled
          onChange={(v) => set("start_minimized", v)}
        />
        <Toggle
          label="Minimizar Arcadia ao iniciar um jogo"
          desc="Esconde a janela do Arcadia assim que o jogo é lançado."
          value={cfg.minimize_on_game_launch === true}
          onChange={(v) => set("minimize_on_game_launch", v)}
        />
        <Toggle
          label="Usar o ícone escuro no indicador de aplicativo"
          desc="Altera a cor do ícone da bandeja para um tom mais escuro."
          value={cfg.dark_tray_icon === true}
          onChange={(v) => set("dark_tray_icon", v)}
        />
        <Toggle
          label="Usar janela sem moldura"
          desc="Remove as bordas padrão de janela do sistema (requer reiniciar o app)."
          value={cfg.frameless_window === true}
          onChange={(v) => set("frameless_window", v)}
        />
      </Group>

      <Group title="Atalhos e integrações">
        <Toggle
          label="Adicionar atalhos na área de trabalho automaticamente"
          desc="Cria um ícone de acesso rápido no Desktop após instalar um jogo."
          value={cfg.auto_desktop_shortcuts === true}
          onChange={(v) => set("auto_desktop_shortcuts", v)}
        />
        <Toggle
          label="Adicionar atalhos dos jogos no menu iniciar automaticamente"
          desc="Cria uma entrada para o jogo no menu de aplicativos do sistema."
          value={cfg.auto_start_menu_shortcuts === true}
          onChange={(v) => set("auto_start_menu_shortcuts", v)}
        />
        <Toggle
          label="Adicionar jogos ao Steam automaticamente"
          desc="Adiciona o jogo instalado como 'não-Steam' na sua biblioteca (ideal p/ Big Picture e Steam Input)."
          value={cfg.auto_add_to_steam !== false}
          onChange={(v) => set("auto_add_to_steam", v)}
        />
        <Toggle
          label="Desativar a sincronização do tempo de jogo"
          desc="Desativa a contagem e o salvamento das horas jogadas."
          value={cfg.disable_playtime_tracking === true}
          onChange={(v) => set("disable_playtime_tracking", v)}
        />
        <Toggle
          label="Habilitar o Discord Rich Presence"
          desc="Mostra no seu perfil do Discord qual jogo você está jogando pelo Arcadia."
          value={cfg.discord_rich_presence === true}
          onChange={(v) => set("discord_rich_presence", v)}
        />
      </Group>

      <Group title="Biblioteca e downloads">
        <Select
          label="Coluna de destaque da biblioteca"
          desc="Se e como uma coluna com jogos em destaque aparece na biblioteca."
          value={cfg.library_featured_column ?? "disabled"}
          options={FEATURED_OPTS}
          onChange={(v) => set("library_featured_column", v as AppConfig["library_featured_column"])}
        />
        <NumberField
          label="Máximo de jogos em Jogados recentemente"
          desc="Quantos jogos aparecem na sua lista de acessos recentes."
          value={cfg.recent_games_max ?? 5}
          min={1}
          max={30}
          onChange={(v) => set("recent_games_max", v)}
        />
        <NumberField
          label="Máximo de núcleos da CPU durante downloads"
          desc="Limite o uso do processador enquanto baixa jogos (0 = usar o máximo disponível)."
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
          <span className="truncate">{value || placeholder || "Selecionar…"}</span>
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
