"use client"

import { useEffect, useState } from "react"
import type { AppConfig } from "../../global"
import { useI18n } from "../../i18n/I18nContext"
import { Modal } from "../../ui/Modal"
import { AboutSection as AboutSectionDesktop } from "../desktop/AboutSection"


type Section = "temas"


/* --------------------------------------------------------------------- */
/* Integrações                                                           */
/* --------------------------------------------------------------------- */
function Toggle({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  const { t } = useI18n()
  return (
    <button
      onClick={() => onChange(!on)}
      className="relative w-11 h-6 rounded-full transition-colors shrink-0"
      style={{ background: on ? "var(--accent)" : "rgba(255,255,255,0.15)" }}
      title={on ? t("common.ativado") : t("common.desativado")}
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
}: {
  cfg: AppConfig
}) {
  const { t } = useI18n()
  // Um estado por debrid (todos seguem o mesmo padrão: input + salvar).
  const [debridTokens, setDebridTokens] = useState({
    realdebrid: cfg.realdebrid_token ?? "",
    torbox: cfg.torbox_token ?? "",
    alldebrid: cfg.alldebrid_token ?? "",
    premiumize: cfg.premiumize_token ?? "",
  })
  const [debridSaved, setDebridSaved] = useState<Record<string, boolean>>({})
  // Expande só o card clicado (com token salvo, também abre por padrão para
  // o usuário conferir/editar). Início: todos fechados — visual limpo.
  const [debridAberto, setDebridAberto] = useState<Record<string, boolean>>({})

  // Tokens chegam de forma assíncrona (getConfig resolve DEPOIS do mount):
  // sem isto o estado inicial ficava "" e um Salvar rápido sobrescrevia o
  // token com vazio.
  useEffect(() => {
    setDebridTokens({
      realdebrid: cfg.realdebrid_token ?? "",
      torbox: cfg.torbox_token ?? "",
      alldebrid: cfg.alldebrid_token ?? "",
      premiumize: cfg.premiumize_token ?? "",
    })
  }, [cfg.realdebrid_token, cfg.torbox_token, cfg.alldebrid_token, cfg.premiumize_token])

  return (
    <div className="max-w-3xl">
      <h2 className="text-3xl font-light tracking-wide text-white mb-1">
        {t("settings.integracoes")}
      </h2>
      <p className="text-sm text-[color:var(--text-2)] mb-8">{t("settings.integracoes.desc")}</p>

      {/* Debrid services: agrupa os 4 num bloco só, cada um começa colapsado
          (apenas nome + status). Clicar no cabeçalho expande o campo do
          token. Resolvedores gratuitos continuam com prioridade — o debrid é
          fallback para os hosters que exigem JS/captcha. */}
      <div className="mb-4 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5">
        <h3 className="mb-1 text-base font-semibold text-white">{t("settings.debrid.titulo")}</h3>
        <p className="mb-4 text-xs text-[color:var(--text-2)]">{t("settings.debrid.desc")}</p>
        <div className="flex flex-col divide-y divide-white/[0.06]">
          {DEBRIDS.map((d) => (
            <DebridItem
              key={d.chave}
              nome={d.nome}
              descKey={d.descKey}
              token={debridTokens[d.chave]}
              // Estado local é a fonte da verdade: o effect sincroniza com o
              // cfg ao carregar, e assim Salvar/Desconectar atualiza o badge
              // na hora (sem esperar reload da aba).
              conectado={Boolean(debridTokens[d.chave])}
              salvo={Boolean(debridSaved[d.chave])}
              aberto={Boolean(debridAberto[d.chave])}
              onToggle={() => setDebridAberto((s) => ({ ...s, [d.chave]: !s[d.chave] }))}
              onChange={(v) => setDebridTokens((s) => ({ ...s, [d.chave]: v }))}
              onSave={async () => {
                // Campo vazio + Salvar = DESCONECTA (apaga o token). Pedido
                // explícito do usuário; o botão muda para "Desconectar".
                await window.launcherAPI?.setConfig({ [d.configKey]: debridTokens[d.chave].trim() })
                setDebridSaved((s) => ({ ...s, [d.chave]: true }))
                setTimeout(() => setDebridSaved((s) => ({ ...s, [d.chave]: false })), 1500)
              }}
              t={t}
            />
          ))}
        </div>
      </div>

    </div>
  )
}

/* --------------------------------------------------------------------- */
/* Metadados                                                             */
/* --------------------------------------------------------------------- */


export function MetadataSection({ onSaved }: { onSaved: () => void }) {
  const { t } = useI18n()
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState(false)
  const [sgdbKey, setSgdbKey] = useState("")
  const [keySaved, setKeySaved] = useState(false)
  const [trailerAuto, setTrailerAuto] = useState(true)
  const [cookies, setCookies] = useState("")
  const [dlAll, setDlAll] = useState<{ done: number; total: number; title: string } | null>(null)

  // As chaves vivem no config.json; aqui só editamos.
  useEffect(() => {
    window.launcherAPI?.getConfig().then((c) => {
      setSgdbKey(c?.steamgriddb_api_key ?? "")
      setTrailerAuto(c?.trailer_auto !== false)
      setCookies(c?.youtube_cookies ?? "")
    })
  }, [])

  // Progresso do "baixar todos os trailers".
  useEffect(() => {
    return window.launcherAPI?.onTrailerProgress((d) => {
      setDlAll(d.total && d.done < d.total ? d : null)
    })
  }, [])

  const baixarTodos = async () => {
    setDlAll({ done: 0, total: 0, title: t("settings.trailers.iniciando") })
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
    { label: t("settings.metadados.item_capa_vertical"), on: true },
    { label: t("settings.metadados.item_banner_hero"), on: true },
    { label: t("settings.metadados.item_logo"), on: true },
    { label: t("settings.metadados.item_descricao_curta"), on: true },
    { label: t("settings.metadados.item_nota_metacritic"), on: true },
    { label: t("settings.metadados.item_genero_ano"), on: true },
    { label: t("settings.metadados.item_trailers"), on: true },
  ]

  return (
    <div className="max-w-3xl">
      <h2 className="text-3xl font-light tracking-wide text-white mb-1">
        {t("settings.metadados.titulo")}
      </h2>
      <p className="text-sm text-[color:var(--text-2)] mb-8">{t("settings.metadados.desc")}</p>

      {/* Chave do SteamGridDB: libera a busca de arte em "Editar metadados" */}
      <div className="mb-8 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2.5">
            <span className="w-2.5 h-2.5 rounded-full" style={{ background: "#9b6bff" }} />
            <h3 className="text-base font-semibold text-white">{t("settings.steamgriddb")}</h3>
          </div>
          <span
            className="text-xs font-semibold px-2.5 py-1 rounded-full"
            style={{
              color: sgdbKey ? "var(--state-success)" : "var(--text-2)",
              background: sgdbKey ? "rgba(74,223,154,0.12)" : "rgba(255,255,255,0.05)",
            }}
          >
            {sgdbKey ? t("common.conectado") : t("common.sem_chave")}
          </span>
        </div>
        <p className="text-xs text-[color:var(--text-2)] mb-3">{t("settings.steamgriddb.desc")}</p>
        <input
          type="password"
          value={sgdbKey}
          onChange={(e) => setSgdbKey(e.target.value)}
          placeholder={t("settings.steamgriddb.placeholder")}
          spellCheck={false}
          className="w-full px-4 py-2.5 rounded-xl text-white text-sm outline-none transition-colors focus:border-[color:var(--accent)] mb-3"
          style={{ background: "rgba(0,0,0,0.4)", border: "1px solid rgba(255,255,255,0.12)" }}
        />
        <button
          onClick={saveKey}
          className="px-5 py-2 rounded-xl bg-white text-sm font-semibold text-black transition-transform hover:scale-[1.03]"
        >
          {keySaved ? t("common.salvo") : t("settings.steamgriddb.salvar")}
        </button>
      </div>

      {/* Trailers: baixados do YouTube (yt-dlp) e tocados no fundo (estilo PS5) */}
      <div className="mb-8 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5">
        <div className="flex items-center gap-2.5 mb-3">
          <span className="w-2.5 h-2.5 rounded-full" style={{ background: "#ff4d6d" }} />
          <h3 className="text-base font-semibold text-white">{t("settings.trailers")}</h3>
        </div>
        <p className="text-xs text-[color:var(--text-2)] mb-4">{t("settings.trailers.desc")}</p>

        <div className="flex items-center justify-between mb-4">
          <span className="text-sm text-white">{t("settings.trailers.auto_tocar")}</span>
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
          {dlAll ? t("common.baixando") : t("settings.trailers.baixar_todos")}
        </button>
        {dlAll && (
          <p className="text-xs text-[color:var(--text-2)] mt-3">
            {dlAll.total ? `${dlAll.done}/${dlAll.total}` : ""} {dlAll.title && `— ${dlAll.title}`}
          </p>
        )}
        <p className="text-[11px] text-[color:var(--text-3)] mt-2">{t("settings.trailers.hint")}</p>

        {/* Cookies do YouTube: só para vídeos com restrição de idade */}
        <div className="mt-5 pt-5" style={{ borderTop: "1px solid rgba(255,255,255,0.08)" }}>
          <div className="flex items-center justify-between gap-3 mb-2">
            <span className="text-sm text-white">{t("settings.trailers.cookies_label")}</span>
            <span
              className="text-xs font-semibold px-2.5 py-1 rounded-full shrink-0"
              style={{
                color: cookies ? "var(--state-success)" : "var(--text-2)",
                background: cookies ? "rgba(74,223,154,0.12)" : "rgba(255,255,255,0.05)",
              }}
            >
              {cookies ? t("common.configurado") : t("common.nao_usado")}
            </span>
          </div>
          <p className="text-xs text-[color:var(--text-2)] mb-3">{t("settings.trailers.cookies_desc")}</p>
          <div
            className="rounded-lg p-3 mb-3 text-xs text-[color:var(--text-2)]"
            style={{ background: "rgba(0,0,0,0.3)", border: "1px solid rgba(255,255,255,0.08)" }}
          >
            <div className="text-[color:var(--text-2)] font-semibold mb-1">
              {t("settings.trailers.cookies_instrucoes")}
            </div>
            {t("settings.trailers.cookies_passo1")}
            <br />
            {t("settings.trailers.cookies_passo2")}
            <br />
            {t("settings.trailers.cookies_passo3")}
          </div>
          <div className="flex items-center gap-3">
            <button
              onClick={async () => {
                const r = await window.launcherAPI?.trailerPickCookies()
                if (r?.ok && r.path) setCookies(r.path)
              }}
              className="rounded-xl border border-white/10 bg-white/[0.06] px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-white/10"
            >
              {t("settings.trailers.escolher_arquivo")}
            </button>
            {cookies && (
              <>
                <span className="text-xs text-[color:var(--text-2)] truncate flex-1">{cookies}</span>
                <button
                  onClick={() => {
                    setCookies("")
                    window.launcherAPI?.setConfig({ youtube_cookies: "" })
                  }}
                  className="text-xs text-[color:var(--state-danger)] shrink-0"
                >
                  {t("settings.trailers.remover")}
                </button>
              </>
            )}
          </div>
        </div>
      </div>

      {/* IGDB: arte E descrição para qualquer plataforma, sem credencial */}
      <div className="mb-8 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2.5">
            <span className="w-2.5 h-2.5 rounded-full" style={{ background: "var(--brand-twitch)" }} />
            <h3 className="text-base font-semibold text-white">{t("settings.igdb")}</h3>
          </div>
          <span
            className="text-xs font-semibold px-2.5 py-1 rounded-full"
            style={{ color: "var(--state-success)", background: "rgba(74,223,154,0.12)" }}
          >
            {t("common.sem_chave")}
          </span>
        </div>
        <p className="text-xs text-[color:var(--text-2)]">{t("settings.igdb.desc")}</p>
      </div>

      <h3 className="text-base font-semibold text-white mb-1">
        {t("settings.igdb.o_que_buscado")}
      </h3>
      <p className="text-xs text-[color:var(--text-2)] mb-4">{t("settings.igdb.coletados_desc")}</p>
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
              style={{ color: "var(--state-success)" }}
            >
              <span className="w-1.5 h-1.5 rounded-full" style={{ background: "var(--state-success)" }} />
              {t("common.ativo")}
            </span>
          </div>
        ))}
      </div>

      <button
        onClick={rebuild}
        disabled={busy}
        className="px-6 py-2.5 rounded-xl bg-white text-sm font-semibold text-black transition-transform hover:scale-[1.03] disabled:opacity-60"
      >
        {busy
          ? t("settings.igdb.reconstruindo")
          : done
            ? t("settings.igdb.atualizado")
            : t("settings.igdb.reconstruir")}
      </button>
      <p className="text-xs text-[color:var(--text-3)] mt-3">{t("settings.igdb.reconstruir_hint")}</p>
    </div>
  )
}

/* --------------------------------------------------------------------- */
/* Sobre                                                                 */
/* --------------------------------------------------------------------- */
// O Big Picture não tem painel de Configurações; a seção é montada como modal
// a partir do menu do perfil (PS5Launcher).
export function AboutSection() {
  return <AboutSectionDesktop console />
}

export function AboutPanel({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useI18n()
  return (
    <Modal open={open} onClose={onClose} title={t("about.titulo")} size="md">
      <AboutSection />
    </Modal>
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

function IconTag() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
      <path d="M21.41 11.58l-9-9A2 2 0 0011 2H4a2 2 0 00-2 2v7a2 2 0 00.59 1.42l9 9a2 2 0 002.82 0l7-7a2 2 0 000-2.84zM6.5 8A1.5 1.5 0 118 6.5 1.5 1.5 0 016.5 8z" />
    </svg>
  )
}

function IconWine() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M8 22h8" />
      <path d="M12 15v7" />
      <path d="M5 3h14l-1.5 7.5a5.5 5.5 0 0 1-11 0L5 3z" />
    </svg>
  )
}

// Debrids: catálogo estático (nome, descrição i18n, chave da config).
// Ordem casa com a de tentativa no backend (RD → TorBox → AllDebrid → Premiumize).
type DebridId = "realdebrid" | "torbox" | "alldebrid" | "premiumize"
const DEBRIDS: {
  chave: DebridId
  nome: string
  descKey: string
  configKey: "realdebrid_token" | "torbox_token" | "alldebrid_token" | "premiumize_token"
}[] = [
  {
    chave: "realdebrid",
    nome: "Real-Debrid",
    descKey: "settings.realdebrid.desc",
    configKey: "realdebrid_token",
  },
  { chave: "torbox", nome: "TorBox", descKey: "settings.torbox.desc", configKey: "torbox_token" },
  {
    chave: "alldebrid",
    nome: "AllDebrid",
    descKey: "settings.alldebrid.desc",
    configKey: "alldebrid_token",
  },
  {
    chave: "premiumize",
    nome: "Premiumize",
    descKey: "settings.premiumize.desc",
    configKey: "premiumize_token",
  },
]

function DebridItem({
  nome,
  descKey,
  token,
  conectado,
  salvo,
  aberto,
  onToggle,
  onChange,
  onSave,
  t,
}: {
  nome: string
  descKey: string
  token: string
  conectado: boolean
  salvo: boolean
  aberto: boolean
  onToggle: () => void
  onChange: (v: string) => void
  onSave: () => void
  t: (k: string, vars?: Record<string, string | number>) => string
}) {
  return (
    <div className="py-3 first:pt-0 last:pb-0">
      <button
        onClick={onToggle}
        className="flex w-full items-center justify-between gap-3 py-1 text-left transition-colors hover:text-white"
      >
        <span className="flex items-center gap-2">
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeLinejoin="round"
            className="text-white/60 transition-transform"
            style={{ transform: aberto ? "rotate(90deg)" : "none" }}
          >
            <polyline points="9 18 15 12 9 6" />
          </svg>
          <span className="text-sm font-semibold text-white">{nome}</span>
          {conectado && (
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="var(--state-success)"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
              <polyline points="22 4 12 14.01 9 11.01" />
            </svg>
          )}
        </span>
        <span
          className="text-[11px] font-medium"
          style={{ color: conectado ? "var(--state-success)" : "var(--text-2)" }}
        >
          {conectado ? t("common.conectado") : t("common.nao_conectado")}
        </span>
      </button>
      {aberto && (
        <div className="mt-3">
          <p className="mb-3 text-xs text-[color:var(--text-2)]">{t(descKey)}</p>
          <div className="flex gap-2">
            <input
              type="password"
              value={token}
              onChange={(e) => onChange(e.target.value)}
              placeholder={t("settings.debrid.token_placeholder", { nome })}
              spellCheck={false}
              className="min-w-0 flex-1 rounded-xl px-4 py-2.5 text-sm text-white outline-none transition-colors focus:border-[color:var(--accent)]"
              style={{ background: "rgba(0,0,0,0.4)", border: "1px solid rgba(255,255,255,0.12)" }}
            />
            <button
              onClick={onSave}
              className="shrink-0 rounded-xl bg-white px-5 py-2 text-sm font-semibold text-black transition-transform hover:scale-[1.03]"
            >
              {salvo
                ? t("common.salvo")
                : !token.trim() && conectado
                  ? t("settings.desconectar")
                  : t("settings.salvar")}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
