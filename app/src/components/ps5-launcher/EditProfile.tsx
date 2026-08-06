"use client"

import { useEffect, useRef, useState } from "react"
import type { Profile } from "../../global"
import type { Game } from "./types"
import { useGamepadNav } from "./useGamepadNav"
import { useI18n } from "../../i18n/I18nContext"
import { useAccountOptional } from "../account/AccountContext"
import { AvatarCrop } from "./AvatarCrop"

interface EditProfileProps {
  open: boolean
  profile: Profile
  games: Game[]
  onClose: () => void
  onChange: (p: Profile | ((atual: Profile) => Profile)) => void
}

type Section = "geral" | "avatar" | "fundo" | "destaques"

export const MAX_SHOWCASE = 8

const INPUT_CLS = "w-full px-4 py-2.5 rounded-xl text-white text-sm outline-none"
const INPUT_STYLE = {
  background: "rgba(255,255,255,0.05)",
  border: "1px solid rgba(255,255,255,0.12)",
} as const
// O valor gravado no perfil continua sendo a chave — o rótulo é traduzido na
// hora de desenhar, senão o país salvo mudaria de nome ao trocar de idioma.
const COUNTRIES = [
  "pais.brasil",
  "pais.portugal",
  "pais.estados_unidos",
  "pais.argentina",
  "pais.chile",
  "pais.mexico",
  "pais.espanha",
  "pais.reino_unido",
  "pais.alemanha",
  "pais.franca",
  "pais.japao",
  "pais.canada",
]

export function EditProfile({ open, profile, games, onClose, onChange }: EditProfileProps) {
  const { t } = useI18n()
  const conta = useAccountOptional() // null fora do provider (modo console)
  const [section, setSection] = useState<Section>("geral")
  // Recorte de avatar — hook tem que ficar ANTES do `if (!open) return null`
  // (senão o nº de hooks muda entre renders e o React derruba tudo → tela preta)
  const [cropSrc, setCropSrc] = useState<string | null>(null)
  const [fields, setFields] = useState({
    name: "",
    realName: "",
    country: "",
    city: "",
    summary: "",
  })
  const timer = useRef<number | null>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  useGamepadNav(rootRef, open, onClose)

  // ── Vitrine (destaques) ── hooks SEMPRE antes do early return ─────────
  // Fonte da verdade LOCAL durante a edição: a prop profile só reflete o
  // round-trip assíncrono do patch e pode estar stale (perfil online buscado
  // no login). Ler dela em cliques rápidos fazia a contagem bugar, perder
  // seleção e até zerar a vitrine. Persistência é DEBOUNCED: N cliques
  // rápidos = UMA chamada com a lista final.
  const [showcaseSel, setShowcaseSel] = useState<string[]>(() => profile.showcase ?? [])
  const showcaseRef = useRef<string[]>(showcaseSel)
  const showcaseTimer = useRef<number | null>(null)
  // Último campo digitado com debounce pendente — o fechar flusha (senão
  // digitar e fechar rápido PERDE o campo silenciosamente).
  const pendenteField = useRef<{ k: keyof typeof fields; v: string } | null>(null)
  // Guarda a versão MAIS RECENTE do salvar: closures de renders antigos teriam
  // profile/onChange stale — o flush do fechar usa a atual.
  const salvarShowcaseRef = useRef<(lista: string[]) => void>(() => {})
  salvarShowcaseRef.current = (lista) => {
    onChange((atual) => ({ ...atual, showcase: lista }))
    window.launcherAPI?.setConfig({ profile: { showcase: lista } }).catch(() => {})
    if (conta?.status === "logado") {
      conta.updatePerfil({ showcase: lista }).catch(() => {})
    }
  }

  // Fecha salvando o que estiver pendente (o debounce de 400ms pode não ter
  // disparado ainda — sem isso o ÚLTIMO clique se perdia).
  const fecharRef = useRef<() => void>(() => {})
  fecharRef.current = () => {
    if (showcaseTimer.current) {
      window.clearTimeout(showcaseTimer.current)
      showcaseTimer.current = null
      salvarShowcaseRef.current(showcaseRef.current)
    }
    // Flush do campo digitado com debounce pendente (mesmo bug do showcase:
    // o timeout de 450ms pode não ter disparado antes do fechar).
    if (timer.current) {
      window.clearTimeout(timer.current)
      timer.current = null
      const pend = pendenteField.current
      if (pend) {
        pendenteField.current = null
        const { k, v } = pend
        patch({ [k]: k === "name" ? v.trim() || t("profile.padrao_jogador") : v })
      }
    }
    onClose()
  }

  const toggleShowcase = (id: string) => {
    const atual = showcaseRef.current
    let next: string[]
    if (atual.includes(id)) {
      next = atual.filter((x) => x !== id)
    } else if (atual.length >= MAX_SHOWCASE) {
      return // limite atingido
    } else {
      next = [...atual, id]
    }
    // UI imediata (ref + state) — nunca fica fora de sincronia
    showcaseRef.current = next
    setShowcaseSel(next)
    if (showcaseTimer.current) window.clearTimeout(showcaseTimer.current)
    showcaseTimer.current = window.setTimeout(() => {
      showcaseTimer.current = null
      salvarShowcaseRef.current(showcaseRef.current)
    }, 400)
  }

  useEffect(() => {
    if (!open) return
    setFields({
      name: profile.name ?? t("profile.padrao_jogador"),
      realName: profile.realName ?? "",
      country: profile.country ?? "",
      city: profile.city ?? "",
      summary: profile.summary ?? "",
    })
  }, [open]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && fecharRef.current()
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, onClose])

  if (!open) return null

  const patch = async (p: Partial<Profile>) => {
    // Forma FUNCIONAL: mergeia sobre o estado MAIS RECENTE do parent — com
    // `{...profile, ...p}` (closure) um patch atrasado REVERTIA campos já
    // salvos por outro patch (ex.: digitar nome e mexer na vitrine em rajada).
    onChange((atual) => ({ ...atual, ...p }))
    await window.launcherAPI?.setConfig({ profile: p })
    // Logado: sincroniza os campos aprovados pro servidor (perfil único).
    if (conta?.status === "logado") {
      await conta.updatePerfil({
        display_name: p.name,
        summary: p.summary,
        country: p.country,
        city: p.city,
        showcase: p.showcase,
      })
    }
  }

  const setField = (k: keyof typeof fields, v: string) => {
    setFields((f) => ({ ...f, [k]: v }))
    // Guarda o pendente: fechar o modal com o debounce ainda correndo (450ms)
    // perdia o campo silenciosamente — o fechar faz o flush deste ref.
    pendenteField.current = { k, v }
    if (timer.current) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => {
      pendenteField.current = null
      patch({ [k]: k === "name" ? v.trim() || t("profile.padrao_jogador") : v })
    }, 450)
  }

  // Upload do avatar pra conta (comum aos fluxos direto e recortado)
  const subirAvatar = async (pathOuBytes: string | Uint8Array, mime?: string, ext?: string) => {
    if (!conta) return
    const up =
      typeof pathOuBytes === "string"
        ? await conta.setAvatar(pathOuBytes)
        : await conta.setAvatarBytes(pathOuBytes, mime || "image/png", ext || ".png")
    if (up.ok && up.avatar_url) {
      onChange((atual) => ({ ...atual, avatar: up.avatar_url }))
      await window.launcherAPI?.setConfig({ profile: { avatar: up.avatar_url } })
      return true
    }
    const msgs: Record<string, string> = {
      avatar_grande: t("editprofile.avatar_grande"),
      avatar_dimensoes: t("editprofile.avatar_dimensoes"),
    }
    window.alert(msgs[up.error || ""] || t("editprofile.avatar_erro") + (up.error ? ` (${up.error})` : ""))
    return false
  }

  const pick = async (kind: "avatar" | "background") => {
    const r = await window.launcherAPI?.pickImage(kind)
    if (r?.ok && r.path) {
      const key = kind === "avatar" ? "avatar" : "background"
      if (kind === "avatar" && conta?.status === "logado") {
        // GIF: vai direto (o main valida tamanho/dimensão e preserva animação)
        // ATENÇÃO: o path tem ?t= no final (cache-buster) — o $ não casa
        if (/\.gif(\?|$)/i.test(r.path)) {
          await subirAvatar(r.path)
          return
        }
        // Estático: se for grande, abre o RECORTE interativo; senão, direto
        const img = new Image()
        img.onload = async () => {
          if (img.naturalWidth > 256 || img.naturalHeight > 256) {
            setCropSrc(r.path)
          } else {
            await subirAvatar(r.path)
          }
        }
        img.onerror = () => {
          // não conseguiu ler dimensões — deixa o main decidir
          subirAvatar(r.path)
        }
        img.src = r.path
        return
      }
      // O main já salvou o caminho limpo no config; aqui só atualizamos a
      // visualização (com ?t= para refletir na hora).
      onChange((atual) => ({ ...atual, [key]: r.path }))
    }
  }

  const NAV: { id: Section; label: string }[] = [
    { id: "geral", label: t("editprofile.nav.geral") },
    { id: "avatar", label: t("editprofile.nav.avatar") },
    { id: "fundo", label: t("editprofile.nav.fundo") },
    { id: "destaques", label: t("editprofile.nav.destaques") },
  ]

  // Ícones por seção da sidebar (estilo consistente com o tema azul/roxo)
  const NAV_ICONES: Record<Section, React.ReactNode> = {
    geral: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
      </svg>
    ),
    avatar: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
        <circle cx="12" cy="8" r="4" />
        <path d="M4 20a8 8 0 0 1 16 0" />
      </svg>
    ),
    fundo: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
        <rect x="3" y="3" width="18" height="18" rx="3" />
        <circle cx="9" cy="9" r="2" />
        <path d="m21 15-4.5-4.5L7 20" />
      </svg>
    ),
    destaques: (
      <svg viewBox="0 0 24 24" fill="currentColor" className="h-4 w-4">
        <path d="M12 2l2.4 4.9 5.4.8-3.9 3.8.9 5.4-4.8-2.5-4.8 2.5.9-5.4L4.2 7.7l5.4-.8z" />
      </svg>
    ),
  }

  return (
    <div ref={rootRef} className="gp-scope fixed inset-0 z-[60] flex overflow-hidden bg-black">
      {profile.background && (
        <img
          src={profile.background}
          alt=""
          className="absolute inset-0 h-full w-full object-cover opacity-20 blur-sm"
        />
      )}
      <div className="absolute inset-0 pointer-events-none bg-[radial-gradient(ellipse_at_top,rgba(75,40,120,0.38),transparent_55%),linear-gradient(135deg,rgba(0,0,0,0.92),rgba(0,0,0,0.72))]" />

      {/* Sidebar */}
      <aside className="relative m-4 flex w-72 shrink-0 flex-col rounded-3xl border border-white/[0.08] bg-white/[0.04] p-5 shadow-2xl shadow-black/50 backdrop-blur-xl">
        {/* Voltar */}
        <button
          onClick={() => fecharRef.current()}
          className="group mb-5 flex items-center gap-2 self-start rounded-full border border-white/10 bg-white/[0.03] px-3.5 py-1.5 text-sm text-[#8a93a6] transition-all hover:border-[#0072ce]/40 hover:text-white"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" className="transition-transform group-hover:-translate-x-0.5">
            <path d="M15 18l-6-6 6-6" />
          </svg>
          {t("editprofile.voltar_perfil")}
        </button>

        {/* Título com ícone */}
        <div className="mb-5 flex items-center gap-3 px-1">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-gradient-to-br from-[#0072ce] to-[#7c3aed] text-white shadow-[0_0_20px_rgba(0,114,206,0.35)]">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
              <path d="M12 20h9" />
              <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
            </svg>
          </div>
          <h1 className="text-xl font-bold text-white">{t("profile.editar_perfil")}</h1>
        </div>

        {/* Card do usuário com anel gradiente */}
        <div className="relative mx-1 mb-5 overflow-hidden rounded-2xl border border-white/[0.08] bg-black/25 p-4">
          <div className="pointer-events-none absolute -right-8 -top-10 h-28 w-28 rounded-full bg-[#0072ce]/20 blur-2xl" />
          <div className="pointer-events-none absolute -bottom-12 -left-8 h-24 w-24 rounded-full bg-[#7c3aed]/15 blur-2xl" />
          <div className="relative flex items-center gap-3">
            <div className="flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded-2xl bg-gradient-to-br from-[#0072ce] to-[#7c3aed] p-[2px] shadow-[0_4px_16px_rgba(0,114,206,0.35)]">
              <div className="flex h-full w-full items-center justify-center overflow-hidden rounded-[14px] bg-[#12121a] text-xl font-bold text-white">
                {profile.avatar ? (
                  <img src={profile.avatar} alt="" className="h-full w-full object-cover" />
                ) : (
                  (fields.name?.[0] || "J").toUpperCase()
                )}
              </div>
            </div>
            <div className="min-w-0">
              <div className="truncate text-sm font-semibold text-white">{fields.name || t("profile.jogador")}</div>
              <span className="mt-1 inline-flex items-center gap-1 rounded-full border border-[#0072ce]/30 bg-[#0072ce]/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-[#4db8ff]">
                <svg viewBox="0 0 24 24" fill="currentColor" className="h-2.5 w-2.5">
                  <path d="M12 2l2.4 4.9 5.4.8-3.9 3.8.9 5.4-4.8-2.5-4.8 2.5.9-5.4L4.2 7.7l5.4-.8z" />
                </svg>
                {t("profile.dono")}
              </span>
            </div>
          </div>
        </div>

        {/* Nav com ícones + barra indicadora */}
        {NAV.map((n) => {
          const active = section === n.id
          const Icone = NAV_ICONES[n.id]
          return (
            <button
              key={n.id}
              onClick={() => setSection(n.id)}
              className="group relative flex items-center gap-3 overflow-hidden rounded-2xl px-4 py-3 text-left text-[15px] font-medium transition-all duration-200"
              style={{
                color: active ? "#fff" : "#8a93a6",
                background: active
                  ? "linear-gradient(135deg, rgba(0,114,206,0.28), rgba(124,58,237,0.16))"
                  : "transparent",
                border: active ? "1px solid rgba(0,114,206,0.35)" : "1px solid transparent",
                boxShadow: active ? "0 4px 20px rgba(0,114,206,0.2)" : "none",
              }}
            >
              <span
                className={`absolute left-0 top-1/2 h-6 w-1 -translate-y-1/2 rounded-full transition-all duration-200 ${
                  active ? "bg-gradient-to-b from-[#0072ce] to-[#7c3aed] opacity-100" : "opacity-0"
                }`}
              />
              <span
                className={`flex h-8 w-8 items-center justify-center rounded-xl transition-colors ${
                  active ? "bg-[#0072ce]/25 text-[#4db8ff]" : "bg-white/[0.04] text-[#8a93a6] group-hover:text-white"
                }`}
              >
                {Icone}
              </span>
              {n.label}
            </button>
          )
        })}
      </aside>

      {/* Conteúdo */}
      <main className="relative flex-1 overflow-y-auto px-10 py-8">
        {section === "geral" && (
          <div className="grid max-w-6xl gap-8 lg:grid-cols-[minmax(0,680px)_320px]">
            <div className="rounded-3xl border border-white/[0.08] bg-white/[0.04] p-7 shadow-2xl shadow-black/30 backdrop-blur-xl">
              <div className="mb-7">
                <h2 className="mb-1 text-3xl font-bold text-white">{t("editprofile.nav.geral")}</h2>
                <p className="text-sm text-[#8a93a6]">{t("editprofile.geral_desc")}</p>
              </div>
              <div className="space-y-5">
                <Field label={t("editprofile.nome_perfil")}>
                  <input
                    value={fields.name}
                    onChange={(e) => setField("name", e.target.value)}
                    className={INPUT_CLS}
                    style={INPUT_STYLE}
                  />
                </Field>
                <Field label={t("editprofile.nome_real")}>
                  <input
                    value={fields.realName}
                    onChange={(e) => setField("realName", e.target.value)}
                    placeholder={t("editprofile.opcional")}
                    className={INPUT_CLS}
                    style={INPUT_STYLE}
                  />
                </Field>
                <div className="grid grid-cols-2 gap-4">
                  <Field label={t("editprofile.pais")}>
                    <select
                      value={fields.country}
                      onChange={(e) => setField("country", e.target.value)}
                      className={INPUT_CLS}
                      style={INPUT_STYLE}
                    >
                      <option value="">{t("editprofile.nao_exibir")}</option>
                      {COUNTRIES.map((c) => (
                        <option key={c} value={c} style={{ background: "#0d0d0f" }}>
                          {t(c)}
                        </option>
                      ))}
                    </select>
                  </Field>
                  <Field label={t("editprofile.cidade")}>
                    <input
                      value={fields.city}
                      onChange={(e) => setField("city", e.target.value)}
                      placeholder={t("editprofile.opcional")}
                      className={INPUT_CLS}
                      style={INPUT_STYLE}
                    />
                  </Field>
                </div>
                <Field label={t("editprofile.resumo")}>
                  <textarea
                    value={fields.summary}
                    onChange={(e) => setField("summary", e.target.value)}
                    placeholder={t("editprofile.resumo_placeholder")}
                    rows={5}
                    className={INPUT_CLS + " resize-none"}
                    style={INPUT_STYLE}
                  />
                </Field>
              </div>
            </div>
            <div className="sticky top-0 h-fit overflow-hidden rounded-3xl border border-white/[0.08] bg-white/[0.04] shadow-2xl shadow-black/30 backdrop-blur-xl">
              <div className="h-28 bg-gradient-to-br from-[#2b165c] via-[#101024] to-black">
                {profile.background && (
                  <img
                    src={profile.background}
                    alt=""
                    className="h-full w-full object-cover opacity-80"
                  />
                )}
              </div>
              <div className="px-6 pb-6">
                <div className="-mt-10 mb-4 flex h-20 w-20 items-center justify-center overflow-hidden rounded-3xl bg-gradient-to-br from-[#0072ce] to-[#003791] text-3xl font-bold text-white ring-4 ring-black/70">
                  {profile.avatar ? (
                    <img src={profile.avatar} alt="" className="h-full w-full object-cover" />
                  ) : (
                    (fields.name?.[0] || "J").toUpperCase()
                  )}
                </div>
                <div className="truncate text-xl font-bold text-white">
                  {fields.name || t("profile.jogador")}
                </div>
                <div className="mt-1 text-xs text-[#8a93a6]">
                  {fields.city || fields.country
                    ? [fields.city, fields.country && t(fields.country)].filter(Boolean).join(" · ")
                    : t("profile.dono")}
                </div>
                {fields.summary && (
                  <p className="mt-4 line-clamp-5 text-sm leading-relaxed text-white/60">
                    {fields.summary}
                  </p>
                )}
              </div>
            </div>
          </div>
        )}

        {section === "avatar" && (
          <div className="max-w-2xl space-y-6">
            <div>
              <h2 className="text-2xl font-bold text-white mb-1">{t("editprofile.nav.avatar")}</h2>
              <p className="text-sm text-[#8a93a6]">{t("editprofile.avatar_desc")}</p>
            </div>
            <div className="flex items-center gap-6">
              <div
                className="w-32 h-32 rounded-2xl overflow-hidden flex items-center justify-center text-4xl font-bold text-white"
                style={{
                  background: "linear-gradient(135deg, #0072ce, #003791)",
                  border: "2px solid rgba(255,255,255,0.15)",
                }}
              >
                {profile.avatar ? (
                  <img src={profile.avatar} alt="" className="w-full h-full object-cover" />
                ) : (
                  (profile.name?.[0] || "J").toUpperCase()
                )}
              </div>
              <div className="flex flex-col gap-2">
                <button
                  onClick={() => pick("avatar")}
                  className="px-5 py-2.5 rounded-xl text-sm font-semibold text-white"
                  style={{ background: "linear-gradient(135deg, #0072ce, #005fa8)" }}
                >
                  {t("editprofile.escolher_imagem")}
                </button>
                {profile.avatar && (
                  <button
                    onClick={() => patch({ avatar: "" })}
                    className="px-5 py-2 rounded-xl text-sm font-medium text-[#c8d0e0] hover:bg-white/5"
                    style={{ border: "1px solid rgba(255,255,255,0.12)" }}
                  >
                    {t("editprofile.remover")}
                  </button>
                )}
              </div>
            </div>
          </div>
        )}

        {section === "fundo" && (
          <div className="max-w-3xl space-y-6">
            <div>
              <h2 className="text-2xl font-bold text-white mb-1">{t("editprofile.nav.fundo")}</h2>
              <p className="text-sm text-[#8a93a6]">{t("editprofile.fundo_desc")}</p>
            </div>
            <div
              className="w-full rounded-2xl overflow-hidden flex items-center justify-center"
              style={{
                aspectRatio: "16/6",
                background: profile.background
                  ? undefined
                  : "linear-gradient(135deg, #12121a, #05050a)",
                border: "1px solid rgba(255,255,255,0.1)",
              }}
            >
              {profile.background ? (
                <img src={profile.background} alt="" className="w-full h-full object-cover" />
              ) : (
                <span className="text-sm text-[#6b7280]">{t("editprofile.sem_fundo")}</span>
              )}
            </div>
            <div className="flex gap-3">
              <button
                onClick={() => pick("background")}
                className="px-5 py-2.5 rounded-xl text-sm font-semibold text-white"
                style={{ background: "linear-gradient(135deg, #0072ce, #005fa8)" }}
              >
                {t("editprofile.escolher_imagem")}
              </button>
              {profile.background && (
                <button
                  onClick={() => patch({ background: "" })}
                  className="px-5 py-2 rounded-xl text-sm font-medium text-[#c8d0e0] hover:bg-white/5"
                  style={{ border: "1px solid rgba(255,255,255,0.12)" }}
                >
                  {t("editprofile.remover")}
                </button>
              )}
            </div>
          </div>
        )}

        {section === "destaques" && (
          <div className="max-w-5xl">
            <div className="mb-6">
              <h2 className="text-2xl font-bold text-white mb-1">
                {t("editprofile.nav.destaques")}
              </h2>
              <p className="text-sm text-[#8a93a6]">
                {t("editprofile.destaques_desc", { max: String(MAX_SHOWCASE) })}{" "}
                <span className="text-white font-medium">
                  {t("editprofile.destaques_selecionados", {
                    selected: String(showcaseSel.length),
                    max: String(MAX_SHOWCASE),
                  })}
                </span>
                . {t("editprofile.destaques_click_hint")}
              </p>
            </div>
            <div className="grid grid-cols-6 gap-3">
              {games
                .filter((g) => g.cover)
                .map((g) => {
                  const idx = showcaseSel.indexOf(g.id)
                  const selected = idx !== -1
                  const full = !selected && showcaseSel.length >= MAX_SHOWCASE
                  return (
                    <button
                      key={g.id}
                      onClick={() => toggleShowcase(g.id)}
                      title={g.title}
                      className="relative rounded-lg overflow-hidden transition-transform"
                      style={{
                        aspectRatio: "2/3",
                        outline: selected ? "3px solid #00a8ff" : "none",
                        outlineOffset: "-3px",
                        opacity: full ? 0.4 : 1,
                        transform: selected ? "scale(0.97)" : "scale(1)",
                      }}
                    >
                      <img src={g.cover} alt={g.title} className="w-full h-full object-cover" />
                      {selected && (
                        <span
                          className="absolute top-1.5 left-1.5 w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold text-white"
                          style={{
                            background: "#00a8ff",
                            boxShadow: "0 0 8px rgba(0,168,255,0.6)",
                          }}
                        >
                          {idx + 1}
                        </span>
                      )}
                    </button>
                  )
                })}
            </div>
          </div>
        )}
      </main>

      {/* Recorte interativo de avatar (imagem estática grande) */}
      {cropSrc && (
        <AvatarCrop
          src={cropSrc}
          t={t}
          onCancel={() => setCropSrc(null)}
          onConfirm={async (bytes, mime, ext) => {
            setCropSrc(null)
            await subirAvatar(bytes, mime, ext)
          }}
        />
      )}
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="block text-xs font-semibold text-[#8a93a6] mb-2 uppercase tracking-wider">
        {label}
      </label>
      {children}
    </div>
  )
}
