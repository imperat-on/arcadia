"use client"

// Sidebar do modo desktop (estilo Heroic, adaptada ao tema do Arcadia).
import { useEffect, useMemo, useState } from "react"
import type { Profile } from "../../global"
import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"
import { useFriends } from "../account/FriendsContext"
import { useAccount } from "../account/AccountContext"

export type DesktopView =
  | "inicio"
  | "biblioteca"
  | "lojas"
  | "plugins"
  | "downloads"
  | "fontes"
  | "amigos"
  | "perfil"
  | "config"
export type ConfigSub = "gerais" | "integracoes" | "metadados" | "acessibilidade" | "emulacao"

const ITENS: { id: DesktopView; label: string; icon: React.ReactNode; labelKey: string }[] = [
  { id: "inicio", label: "Início", labelKey: "sidebar.inicio", icon: <IconHome /> },
  { id: "biblioteca", label: "Biblioteca", labelKey: "sidebar.biblioteca", icon: <IconGrid /> },
  { id: "lojas", label: "Loja", labelKey: "sidebar.lojas", icon: <IconStore /> },
  { id: "plugins", label: "Componentes", labelKey: "sidebar.plugins", icon: <IconPlugin /> },
  { id: "downloads", label: "Downloads", labelKey: "sidebar.downloads", icon: <IconDownload /> },
  { id: "fontes", label: "Fontes", labelKey: "sidebar.fontes", icon: <IconFontes /> },
  { id: "amigos", label: "Amigos", labelKey: "amigos.titulo", icon: <IconUsers /> },
  { id: "config", label: "Configurações", labelKey: "settings.title", icon: <IconGear /> },
]

const CONFIG_SUBS: { id: ConfigSub; label: string; labelKey: string; groupKey?: string; icon?: React.ReactNode; badgeKey?: string }[] = [
  { id: "gerais", label: "Config. Gerais", labelKey: "settings.general" },
  { id: "integracoes", label: "Integrações", labelKey: "settings.integracoes" },
  { id: "metadados", label: "Metadados", labelKey: "settings.metadados.titulo" },
  { id: "acessibilidade", label: "Acessibilidade", labelKey: "sidebar.acessibilidade" },
  { id: "emulacao", label: "Emulação", labelKey: "settings.emulacao", groupKey: "settings.emulacao.grupo", icon: <IconGamepad />, badgeKey: "settings.emulacao.novo" },
]

export function Sidebar({
  view,
  onView,
  downloadsActive,
  onQuit,
  onBigPicture,
  configSub,
  onConfigSub,
  profile,
  onProfile,
  onLogout,
  onRefresh,
  games,
  librarySidebar,
  onToggleLibrarySidebar,
  onOpenGame,
  onAddGame,
  activeGameId,
}: {
  view: DesktopView
  onView: (v: DesktopView) => void
  downloadsActive: number
  onQuit: () => void
  onBigPicture: () => void
  configSub: ConfigSub
  onConfigSub: (s: ConfigSub) => void
  profile: Profile
  onProfile: () => void
  /** Chamado após logout (o launcher reabre a tela de login). */
  onLogout?: () => void
  onRefresh: () => void
  games: Game[]
  librarySidebar: boolean
  onToggleLibrarySidebar: () => void
  onOpenGame: (g: Game) => void
  onAddGame: () => void
  activeGameId?: string
}) {
  const { t } = useI18n()
  const { pedidos } = useFriends()
  const { status: contaStatus, session: contaSession, perfil: contaPerfil, signOut } = useAccount()
  const logado = contaStatus === "logado"
  const [profileMenu, setProfileMenu] = useState(false)
  const [buscaJogos, setBuscaJogos] = useState("")
  const [avatarErro, setAvatarErro] = useState(false)
  // Identidade: logado → conta online (display_name > username + avatar do
  // servidor); deslogado → perfil local. O avatar da conta tem prioridade.
  const nome = logado
    ? contaPerfil?.display_name || contaSession?.user?.username || contaPerfil?.username || profile.name || t("profile.jogador")
    : profile.name || t("profile.jogador")
  const avatarUrl = logado ? contaPerfil?.avatar_url || profile.avatar : profile.avatar
  // Se o avatar falhar (ex.: URL quebrada no servidor), cai na letra em vez
  // do preview de img quebrado.
  const mostraAvatar = avatarUrl && !avatarErro

  // Um novo avatar pode chegar enquanto a Sidebar continua montada. Limpa o
  // erro anterior para que a URL atual volte a ser tentada imediatamente.
  useEffect(() => {
    setAvatarErro(false)
  }, [avatarUrl])

  const jogos = useMemo(() => {
    const l = games.filter((g) => !g.hidden)
    const q = buscaJogos.trim().toLowerCase()
    const f = q ? l.filter((g) => g.title.toLowerCase().includes(q)) : l
    return [...f].sort(
      (a, b) =>
        Number(b.favorite || false) - Number(a.favorite || false) || a.title.localeCompare(b.title),
    )
  }, [games, buscaJogos])
  return (
    <aside
      className="desktop-retro-sidebar flex h-full w-[194px] shrink-0 flex-col pb-10"
      style={{ background: "var(--sidebar-bg)" }}
    >
      <button onClick={() => onView("inicio")} className="desktop-sidebar-brand flex h-[70px] shrink-0 flex-col items-center justify-center border-b text-center">
        <strong className="desktop-arcadia-logo">ARCADIA</strong>
      </button>
      <div className="desktop-sidebar-profile px-5 pb-3 pt-3">
        <button
          onClick={() => setProfileMenu((v) => !v)}
          className="group flex w-full items-center gap-3 rounded-xl p-1 text-left transition-colors hover:bg-white/[0.04]"
        >
          <div className="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-full border border-white/10 bg-white/[0.06] text-sm font-semibold text-white/80">
            {mostraAvatar ? (
              <img
                src={avatarUrl}
                alt=""
                className="h-full w-full object-cover"
                draggable={false}
                onError={() => setAvatarErro(true)}
              />
            ) : (
              nome[0]?.toUpperCase()
            )}
          </div>
          <div className="min-w-0">
            <div className="truncate text-[13px] font-medium text-white/80 group-hover:text-white">
              {nome}
            </div>
          </div>
        </button>
        {profileMenu && (
          <div className="mt-2 overflow-hidden rounded-xl border border-white/10 bg-[#16161c]/95 shadow-2xl shadow-black/50 backdrop-blur">
            <ProfileMenuItem
              label={t("profile.meu_perfil")}
              onClick={() => {
                setProfileMenu(false)
                onProfile()
              }}
            />
            <ProfileMenuItem
              label={t("profile.atualizar_biblioteca")}
              onClick={() => {
                setProfileMenu(false)
                onRefresh()
              }}
            />
            <ProfileMenuItem
              label={t("settings.title")}
              onClick={() => {
                setProfileMenu(false)
                onConfigSub("gerais")
                onView("config")
              }}
            />
            <ProfileMenuItem
              danger
              label={logado ? t("account.sair") : t("profile.sair")}
              onClick={() => {
                setProfileMenu(false)
                if (logado) {
                  signOut().then(() => onLogout?.())
                } else {
                  onQuit()
                }
              }}
            />
          </div>
        )}
      </div>

      {/* Itens */}
      <nav className="desktop-sidebar-nav flex flex-col gap-0.5 px-2">
        {ITENS.map((it) => {
          const active = view === it.id
          return (
            <div key={it.id}>
              <button
                onClick={() => onView(it.id)}
                className={`desktop-sidebar-item relative flex w-full items-center gap-3 rounded-lg px-4 py-2.5 text-left text-[12px] transition-colors ${
                  active
                    ? "bg-white/[0.07] text-white"
                    : "text-white/50 hover:bg-white/[0.04] hover:text-white/85"
                }`}
              >
                {active && (
                  <span
                    className="absolute left-0 top-1/2 h-5 w-0.5 -translate-y-1/2 rounded-full"
                    style={{ background: "var(--accent)" }}
                  />
                )}
                {it.icon}
                {t(it.labelKey)}
                {it.id === "downloads" && downloadsActive > 0 && (
                  <span
                    className="ml-auto flex h-5 min-w-5 items-center justify-center rounded-full px-1.5 text-[11px] font-bold text-black"
                    style={{ background: "var(--accent)" }}
                  >
                    {downloadsActive}
                  </span>
                )}
                {it.id === "amigos" && pedidos > 0 && (
                  <span
                    className="ml-auto flex h-5 min-w-5 items-center justify-center rounded-full px-1.5 text-[11px] font-bold text-black"
                    style={{ background: "#f5a623" }}
                  >
                    {pedidos}
                  </span>
                )}
              </button>

              {/* Sub-itens de Configurações: aparecem logo abaixo do item pai */}
              {it.id === "config" && view === "config" && (
                <div className="ml-9 mt-1 flex flex-col gap-0.5">
                  {CONFIG_SUBS.map((s) => {
                    const ativo = configSub === s.id
                    return (
                      <div key={s.id}>
                        {s.groupKey && <p className="mb-1 mt-2 px-3 text-[10px] font-semibold uppercase tracking-[0.18em] text-white/30">{t(s.groupKey)}</p>}
                        <button
                          onClick={() => {
                            onConfigSub(s.id)
                            onView("config")
                          }}
                          className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-1.5 text-left text-[14px] transition-colors ${
                            ativo
                              ? "bg-white/[0.06] text-white"
                              : "text-white/45 hover:bg-white/[0.03] hover:text-white/85"
                          }`}
                        >
                          {s.icon || <span className="h-1 w-1 rounded-full" style={{ background: ativo ? "var(--accent)" : "rgba(255,255,255,0.25)" }} />}
                          <span className="min-w-0 flex-1">{t(s.labelKey)}</span>
                          {s.badgeKey && <span className="rounded-full border border-[color:var(--accent)]/60 px-1.5 py-0.5 text-[9px] font-bold tracking-wide text-[color:var(--accent)]">{t(s.badgeKey)}</span>}
                        </button>
                      </div>
                    )
                  })}
                </div>
              )}
            </div>
          )
        })}
      </nav>

      {/* Lista de jogos estilo Hydra (permanente em todas as abas) */}
      {
        <div className="desktop-sidebar-games mt-3 flex min-h-0 flex-1 flex-col border-t border-white/[0.06] pt-2">
          <div className="flex items-center gap-1 px-4 pb-1">
            <button
              onClick={onToggleLibrarySidebar}
              className="flex flex-1 items-center gap-1.5 text-left text-[11px] font-semibold uppercase tracking-wider text-white/50 transition-colors hover:text-white/80"
              title={librarySidebar ? t("sidebar.ocultar_jogos") : t("sidebar.mostrar_jogos")}
            >
              <svg
                width="12"
                height="12"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.5"
                strokeLinecap="round"
                strokeLinejoin="round"
                className={`transition-transform ${librarySidebar ? "" : "-rotate-90"}`}
              >
                <polyline points="6 9 12 15 18 9" />
              </svg>
              {t("sidebar.jogos")}
              <span className="ml-0.5 rounded bg-white/10 px-1.5 py-0.5 text-[10px] font-bold text-white/60">
                {jogos.length}
              </span>
            </button>
            <button
              onClick={onAddGame}
              title={t("sidebar.adicionar_jogo")}
              className="rounded p-1 text-white/45 transition-colors hover:bg-white/[0.06] hover:text-white"
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.2"
                strokeLinecap="round"
              >
                <line x1="12" y1="5" x2="12" y2="19" />
                <line x1="5" y1="12" x2="19" y2="12" />
              </svg>
            </button>
            <button
              onClick={onRefresh}
              title={t("sidebar.atualizar")}
              className="rounded p-1 text-white/45 transition-colors hover:bg-white/[0.06] hover:text-white"
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M23 4v6h-6" />
                <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
              </svg>
            </button>
          </div>

          {librarySidebar && (
            <>
              <div className="px-3 pb-2 pt-1">
                <input
                  value={buscaJogos}
                  onChange={(e) => setBuscaJogos(e.target.value)}
                  placeholder={t("library.buscar")}
                  spellCheck={false}
                  className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3 py-1.5 text-[12px] text-white outline-none transition-colors placeholder:text-white/30 focus:border-[color:var(--accent)]"
                />
              </div>
              <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
                {jogos.map((g) => (
                  <button
                    key={g.id}
                    onClick={() => onOpenGame(g)}
                    className={`relative flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left transition-colors ${
                      activeGameId === g.id ? "bg-white/[0.07]" : "hover:bg-white/[0.06]"
                    }`}
                    title={g.title}
                  >
                    {activeGameId === g.id && (
                      <span
                        className="absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-full"
                        style={{ background: "var(--accent)" }}
                      />
                    )}
                    <GameIcon game={g} />
                    <span
                      className={`truncate text-[13px] ${activeGameId === g.id ? "text-white" : "text-white/75"}`}
                    >
                      {g.title}
                    </span>
                  </button>
                ))}
                {jogos.length === 0 && (
                  <p className="px-2 py-4 text-center text-[12px] text-white/30">
                    {t("library.vazio")}
                  </p>
                )}
              </div>
            </>
          )}
        </div>
      }

      {/* Rodapé */}
      <div className="desktop-sidebar-footer mt-auto px-4 py-3">
        <button
          onClick={onBigPicture}
          className="mb-3 flex w-full items-center gap-2.5 rounded-xl border border-white/10 px-3 py-2.5 text-left text-[13px] font-semibold text-white/70 transition-colors hover:border-[color:var(--accent)] hover:text-white"
          title={t("sidebar.modo_tela_cheia")}
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.7"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <rect x="2" y="4" width="20" height="13" rx="2" />
            <path d="M8 21h8m-4-4v4" />
          </svg>
          {t("sidebar.bigpicture")}
        </button>
        <div className="flex items-center justify-between text-xs text-white/35">
          <span>
            {t("app.name")} · {t("sidebar.modo_desktop")}
          </span>
          <button
            onClick={onQuit}
            className="flex items-center gap-1.5 text-white/50 transition-colors hover:text-white"
            title={t("sidebar.sair")}
          >
            <svg
              width="15"
              height="15"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.8"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M18.36 6.64a9 9 0 1 1-12.73 0" />
              <line x1="12" x2="12" y1="2" y2="12" />
            </svg>
            {t("sidebar.sair")}
          </button>
        </div>
      </div>
    </aside>
  )
}

function GameIcon({ game }: { game: Game }) {
  // Retrôs não possuem ícone separado confiável entre máquinas: a capa
  // sincronizada é a fonte canônica. Steam continua preferindo o ícone quadrado.
  const retro = game.retro === true || game.launcher === "retro" || String(game.id).startsWith("retro:")
  const fontes = (retro ? [game.cover, game.icon] : [game.icon, game.cover]).filter(Boolean) as string[]
  const [fase, setFase] = useState(0)
  const src = fontes[fase]
  if (src) {
    return (
      <img
        src={src}
        alt=""
        onError={() => setFase((f) => f + 1)}
        draggable={false}
        className="h-7 w-7 shrink-0 rounded-md object-cover"
      />
    )
  }
  return (
    <span
      className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-[12px] font-bold text-white/70"
      style={{ background: "color-mix(in srgb, var(--accent) 22%, transparent)" }}
    >
      {game.title[0]?.toUpperCase() || "?"}
    </span>
  )
}

function ProfileMenuItem({
  label,
  onClick,
  danger,
}: {
  label: string
  onClick: () => void
  danger?: boolean
}) {
  return (
    <button
      onClick={onClick}
      className={`flex w-full items-center px-4 py-2.5 text-left text-[13px] transition-colors hover:bg-white/[0.07] ${danger ? "text-[#ff6b81]" : "text-white/80"}`}
    >
      {label}
    </button>
  )
}

/* ─── Iconografia própria do Arcadia ───────────────────────────────────────
   Set autoral, não Lucide. Uma gramática só: grade 24, cantos vivos, um
   acento diagonal a 45° que reaparece em cada símbolo (o "corte" do Arcadia).
   Traço fino e geométrico para casar com o wordmark Michroma sobre o OLED. */
const s = {
  width: 18,
  height: 18,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round",
  strokeLinejoin: "round",
} as const

// Início: telhado chanfrado + núcleo, o corte diagonal marca a "porta".
function IconHome() {
  return (
    <svg {...s}>
      <path d="M4 10.5 12 4l8 6.5V19a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1z" />
      <path d="M12 20v-5.5" />
      <path d="m9.5 12 2.5-2.5 2.5 2.5" />
    </svg>
  )
}

// Biblioteca: pilha de cartuchos/lombadas, um puxado adiante (o selecionado).
function IconGrid() {
  return (
    <svg {...s}>
      <rect x="4" y="4" width="5" height="16" rx="1" />
      <rect x="11" y="4" width="5" height="16" rx="1" />
      <path d="m17.6 5 2.5.7-3.2 12-2.4-.7" />
    </svg>
  )
}

// Loja: toldo de barraca com o vinco diagonal, base aberta.
function IconStore() {
  return (
    <svg {...s}>
      <path d="M4 9 6 4h12l2 5" />
      <path d="M4 9h16" />
      <path d="M4 9v10a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1V9" />
      <path d="M9.5 20v-4.5a2.5 2.5 0 0 1 5 0V20" />
    </svg>
  )
}

// Componentes: bloco modular encaixando por uma aba diagonal (plug abstrato).
function IconPlugin() {
  return (
    <svg {...s}>
      <path d="M6 6h8l4 4v8a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1z" />
      <path d="M14 6v4h4" />
      <path d="M9 13.5h4.5" />
      <path d="M9 16.5h6" />
    </svg>
  )
}

// Downloads: bandeja com seta que desce cortando na diagonal.
function IconDownload() {
  return (
    <svg {...s}>
      <path d="M12 4v9" />
      <path d="m8 10 4 4 4-4" />
      <path d="M5 16v2a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-2" />
    </svg>
  )
}

// Amigos: dois usuários (cabeça + ombros) lado a lado.
function IconUsers() {
  return (
    <svg {...s}>
      <circle cx="9" cy="8" r="3.2" />
      <path d="M3.5 19a5.5 5.5 0 0 1 11 0" />
      <path d="M16 5.5a3.2 3.2 0 0 1 0 6" />
      <path d="M17 13.6a5.5 5.5 0 0 1 3.5 5.4" />
    </svg>
  )
}

// Fontes: elo de corrente (link) — catálogos JSON apontados por URL.
function IconFontes() {
  return (
    <svg {...s}>
      <path d="M10 14a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1.5 1.5" />
      <path d="M14 10a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7L12.5 19" />
    </svg>
  )
}

// Configurações: engrenagem hexagonal (6 dentes) com núcleo — mais firme que a
// engrenagem redonda genérica, alinhada à gramática geométrica.
function IconGear() {
  return (
    <svg {...s}>
      <path d="M12 3.2 18.6 7v10L12 20.8 5.4 17V7z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  )
}

// Emulação: gamepad simples para a seção de clássicos.
function IconGamepad() {
  return (
    <svg {...s}>
      <path d="M7.5 8.5h9a4 4 0 0 1 3.8 3l1.2 4.5a2.5 2.5 0 0 1-4.6 1.7l-1.4-2.2h-7l-1.4 2.2a2.5 2.5 0 0 1-4.6-1.7l1.2-4.5a4 4 0 0 1 3.8-3Z" />
      <path d="M7 12v4M5 14h4M16 13h.01M19 15h.01" />
    </svg>
  )
}

// Acessibilidade: figura centrada dentro de um anel de foco — inclusão + alvo.
function IconA11y() {
  return (
    <svg {...s}>
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="8" r="1.4" />
      <path d="M7.5 10.5c3 1 6 1 9 0" />
      <path d="M12 11v3.5L10 18" />
      <path d="M12 14.5 14 18" />
    </svg>
  )
}
