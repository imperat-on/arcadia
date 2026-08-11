"use client"

import { useEffect, useRef, useState } from "react"
import type { Profile } from "../../global"
import type { Game } from "./types"
import { useGamepadNav } from "./useGamepadNav"
import { useI18n } from "../../i18n/I18nContext"
import { useAccountOptional, OWNER_USERNAME } from "../account/AccountContext"
import { AvatarCrop } from "./AvatarCrop"

interface EditProfileProps {
  open: boolean
  profile: Profile
  games: Game[]
  onClose: () => void
  onChange: (p: Profile | ((atual: Profile) => Profile)) => void
}

type Section = "geral" | "avatar" | "fundo"

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
  const ehDono = conta?.perfil?.username === OWNER_USERNAME
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

  // Último campo digitado com debounce pendente — o fechar flusha (senão
  // digitar e fechar rápido PERDE o campo silenciosamente).
  const pendenteField = useRef<{ k: keyof typeof fields; v: string } | null>(null)

  // Fecha salvando o campo digitado com debounce pendente (o timeout de
  // 450ms pode não ter disparado antes do fechar).
  const fecharRef = useRef<() => void>(() => {})
  fecharRef.current = () => {
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
  const subirBackground = async (path: string) => {
    const r = await window.launcherAPI?.accountSetBackground(path)
    if (r?.ok && r.background_url) {
      await window.launcherAPI?.setConfig({ profile: { background: r.background_url } })
      onChange((atual) => ({ ...atual, background: r.background_url }))
    } else if (r?.error) {
      console.error("[perfil] background falhou:", r.error)
    }
  }

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
      if (kind === "background" && conta?.status === "logado") {
        await subirBackground(r.path)
        return
      }
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
  }

  return (
    <div ref={rootRef} className="gp-scope fixed inset-0 z-[60] flex flex-col overflow-hidden bg-black">
      {profile.background && (
        <img
          src={profile.background}
          alt=""
          className="absolute inset-0 h-full w-full object-cover opacity-20 blur-sm"
        />
      )}
      <div className="absolute inset-0 pointer-events-none bg-[radial-gradient(ellipse_at_top,rgba(75,40,120,0.38),transparent_55%),linear-gradient(135deg,rgba(0,0,0,0.92),rgba(0,0,0,0.72))]" />

      {/* Header com abas horizontais */}
      <header className="relative z-10 border-b border-white/[0.08] bg-black/40 backdrop-blur-xl">
        <div className="mx-auto max-w-7xl px-8 py-6">
          <div className="mb-6 flex items-center justify-between">
            {/* Voltar */}
            <button
              onClick={() => fecharRef.current()}
              className="group flex items-center gap-2 rounded-full border border-white/10 bg-white/[0.03] px-4 py-2 text-sm text-[#8a93a6] transition-all hover:border-[#0072ce]/40 hover:text-white"
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" className="transition-transform group-hover:-translate-x-0.5">
                <path d="M15 18l-6-6 6-6" />
              </svg>
              {t("editprofile.voltar_perfil")}
            </button>

            {/* Card compacto do usuário */}
            <div className="flex items-center gap-3">
              <div className="flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-xl bg-gradient-to-br from-[#0072ce] to-[#7c3aed] p-[2px] shadow-[0_4px_16px_rgba(0,114,206,0.35)]">
                <div className="flex h-full w-full items-center justify-center overflow-hidden rounded-[9px] bg-[#12121a] text-base font-bold text-white">
                  {profile.avatar ? (
                    <img src={profile.avatar} alt="" className="h-full w-full object-cover" />
                  ) : (
                    (fields.name?.[0] || "J").toUpperCase()
                  )}
                </div>
              </div>
              <div className="flex flex-col">
                <div className="text-sm font-semibold text-white">{fields.name || t("profile.jogador")}</div>
                {ehDono && (
                  <span className="inline-flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wider text-[#4db8ff]">
                    <svg viewBox="0 0 24 24" fill="currentColor" className="h-2.5 w-2.5">
                      <path d="M12 2l2.4 4.9 5.4.8-3.9 3.8.9 5.4-4.8-2.5-4.8 2.5.9-5.4L4.2 7.7l5.4-.8z" />
                    </svg>
                    {t("profile.dono")}
                  </span>
                )}
              </div>
            </div>
          </div>

          {/* Abas horizontais */}
          <nav className="flex gap-2">
            {NAV.map((n) => {
              const active = section === n.id
              const Icone = NAV_ICONES[n.id]
              return (
                <button
                  key={n.id}
                  onClick={() => setSection(n.id)}
                  className="group relative flex items-center gap-2.5 rounded-xl px-5 py-2.5 text-sm font-medium transition-all duration-200"
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
                    className={`flex h-7 w-7 items-center justify-center rounded-lg transition-colors ${
                      active ? "bg-[#0072ce]/25 text-[#4db8ff]" : "bg-white/[0.04] text-[#8a93a6] group-hover:text-white"
                    }`}
                  >
                    {Icone}
                  </span>
                  {n.label}
                  {active && (
                    <span className="absolute bottom-0 left-1/2 h-0.5 w-12 -translate-x-1/2 translate-y-full rounded-full bg-gradient-to-r from-[#0072ce] to-[#7c3aed]" />
                  )}
                </button>
              )
            })}
          </nav>
        </div>
      </header>

      {/* Conteúdo */}
      <main className="relative flex-1 overflow-y-auto px-8 py-10">
        <div className="mx-auto max-w-7xl">
          {section === "geral" && (
            <div className="grid gap-6 lg:grid-cols-[1fr_380px]">
              {/* Formulário */}
              <div className="space-y-6">
                <div className="rounded-2xl border border-white/[0.08] bg-white/[0.04] p-6 shadow-2xl shadow-black/30 backdrop-blur-xl">
                  <div className="mb-6">
                    <h3 className="mb-1 text-lg font-bold text-white">{t("editprofile.informacoes_basicas")}</h3>
                    <p className="text-xs text-[#8a93a6]">{t("editprofile.geral_desc")}</p>
                  </div>
                  <div className="space-y-4">
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
                  </div>
                </div>

                <div className="rounded-2xl border border-white/[0.08] bg-white/[0.04] p-6 shadow-2xl shadow-black/30 backdrop-blur-xl">
                  <div className="mb-6">
                    <h3 className="mb-1 text-lg font-bold text-white">{t("editprofile.localizacao")}</h3>
                    <p className="text-xs text-[#8a93a6]">{t("editprofile.localizacao_desc")}</p>
                  </div>
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
                </div>

                <div className="rounded-2xl border border-white/[0.08] bg-white/[0.04] p-6 shadow-2xl shadow-black/30 backdrop-blur-xl">
                  <div className="mb-6">
                    <h3 className="mb-1 text-lg font-bold text-white">{t("editprofile.sobre")}</h3>
                    <p className="text-xs text-[#8a93a6]">{t("editprofile.sobre_desc")}</p>
                  </div>
                  <Field label={t("editprofile.resumo")}>
                    <textarea
                      value={fields.summary}
                      onChange={(e) => setField("summary", e.target.value)}
                      placeholder={t("editprofile.resumo_placeholder")}
                      rows={6}
                      className={INPUT_CLS + " resize-none"}
                      style={INPUT_STYLE}
                    />
                  </Field>
                </div>
              </div>

              {/* Preview do perfil */}
              <div className="sticky top-6 h-fit">
                <div className="overflow-hidden rounded-2xl border border-white/[0.08] bg-white/[0.04] shadow-2xl shadow-black/30 backdrop-blur-xl">
                  <div className="relative h-32 overflow-hidden bg-gradient-to-br from-[#2b165c] via-[#101024] to-black">
                    {profile.background && (
                      <img
                        src={profile.background}
                        alt=""
                        className="h-full w-full object-cover opacity-80"
                      />
                    )}
                    <div className="absolute inset-0 bg-gradient-to-t from-black/60 to-transparent" />
                  </div>
                  <div className="px-6 pb-6">
                    <div className="relative z-10 -mt-12 mb-4 flex h-24 w-24 shrink-0 items-center justify-center overflow-hidden rounded-2xl bg-gradient-to-br from-[#0072ce] to-[#003791] text-3xl font-bold text-white ring-4 ring-black/70 shadow-xl">
                      {profile.avatar ? (
                        <img src={profile.avatar} alt="" className="h-full w-full object-cover" />
                      ) : (
                        (fields.name?.[0] || "J").toUpperCase()
                      )}
                    </div>
                    <div className="space-y-1">
                      <div className="truncate text-xl font-bold text-white">
                        {fields.name || t("profile.jogador")}
                      </div>
                      {fields.realName && (
                        <div className="text-sm text-[#8a93a6]">{fields.realName}</div>
                      )}
                      <div className="flex items-center gap-1.5 text-xs text-[#8a93a6]">
                        {(fields.city || fields.country) ? (
                          <>
                            <svg viewBox="0 0 24 24" fill="currentColor" className="h-3 w-3">
                              <path d="M12 2C8.13 2 5 5.13 5 9c0 5.25 7 13 7 13s7-7.75 7-13c0-3.87-3.13-7-7-7zm0 9.5c-1.38 0-2.5-1.12-2.5-2.5s1.12-2.5 2.5-2.5 2.5 1.12 2.5 2.5-1.12 2.5-2.5 2.5z"/>
                            </svg>
                            {[fields.city, fields.country && t(fields.country)].filter(Boolean).join(", ")}
                          </>
                        ) : (
                          t("profile.dono")
                        )}
                      </div>
                    </div>
                    {fields.summary && (
                      <div className="mt-5 space-y-2">
                        <div className="text-xs font-semibold uppercase tracking-wider text-[#8a93a6]">
                          {t("editprofile.sobre")}
                        </div>
                        <p className="line-clamp-6 text-sm leading-relaxed text-white/70">
                          {fields.summary}
                        </p>
                      </div>
                    )}
                  </div>
                </div>
              </div>
            </div>
          )}

        {section === "avatar" && (
          <div className="mx-auto max-w-3xl">
            <div className="rounded-2xl border border-white/[0.08] bg-white/[0.04] p-8 shadow-2xl shadow-black/30 backdrop-blur-xl">
              <div className="mb-8">
                <h3 className="mb-2 text-2xl font-bold text-white">{t("editprofile.nav.avatar")}</h3>
                <p className="text-sm text-[#8a93a6]">{t("editprofile.avatar_desc")}</p>
              </div>

              <div className="flex flex-col items-center gap-8 sm:flex-row sm:items-start">
                {/* Avatar atual */}
                <div className="relative">
                  <div
                    className="flex h-40 w-40 items-center justify-center overflow-hidden rounded-2xl text-5xl font-bold text-white shadow-2xl"
                    style={{
                      background: "linear-gradient(135deg, #0072ce, #003791)",
                      border: "2px solid rgba(255,255,255,0.15)",
                    }}
                  >
                    {profile.avatar ? (
                      <img src={profile.avatar} alt="" className="h-full w-full object-cover" />
                    ) : (
                      (profile.name?.[0] || "J").toUpperCase()
                    )}
                  </div>
                  {profile.avatar && (
                    <div className="absolute -bottom-2 -right-2 rounded-xl bg-gradient-to-br from-[#0072ce] to-[#7c3aed] p-2 shadow-lg">
                      <svg viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2" className="h-5 w-5">
                        <path d="M9 12l2 2 4-4m6 2a9 9 0 1 1-18 0 9 9 0 0 1 18 0z" />
                      </svg>
                    </div>
                  )}
                </div>

                {/* Ações */}
                <div className="flex flex-1 flex-col gap-4">
                  <div className="rounded-xl border border-white/[0.08] bg-white/[0.02] p-5">
                    <div className="mb-4 flex items-start gap-3">
                      <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-[#0072ce]/15">
                        <svg viewBox="0 0 24 24" fill="none" stroke="#4db8ff" strokeWidth="2" className="h-5 w-5">
                          <circle cx="12" cy="12" r="10" />
                          <path d="M12 16v-4m0-4h.01" />
                        </svg>
                      </div>
                      <div className="space-y-1">
                        <div className="text-sm font-semibold text-white">{t("editprofile.avatar_requisitos")}</div>
                        <ul className="space-y-1 text-xs text-[#8a93a6]">
                          <li>• {t("editprofile.avatar_tamanho")}</li>
                          <li>• {t("editprofile.avatar_formatos")}</li>
                          <li>• {t("editprofile.avatar_recomendacao")}</li>
                        </ul>
                      </div>
                    </div>
                  </div>

                  <button
                    onClick={() => pick("avatar")}
                    className="group relative overflow-hidden rounded-xl px-6 py-3 font-semibold text-white shadow-lg transition-all hover:scale-[1.02] hover:shadow-xl"
                    style={{ background: "linear-gradient(135deg, #0072ce, #005fa8)" }}
                  >
                    <div className="absolute inset-0 bg-gradient-to-r from-transparent via-white/10 to-transparent opacity-0 transition-opacity group-hover:opacity-100" />
                    <div className="relative flex items-center justify-center gap-2">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-5 w-5">
                        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                        <polyline points="17 8 12 3 7 8" />
                        <line x1="12" y1="3" x2="12" y2="15" />
                      </svg>
                      {t("editprofile.escolher_imagem")}
                    </div>
                  </button>

                  {profile.avatar && (
                    <button
                      onClick={() => patch({ avatar: "" })}
                      className="rounded-xl border border-white/[0.12] px-6 py-2.5 text-sm font-medium text-[#c8d0e0] transition-all hover:border-red-500/30 hover:bg-red-500/5 hover:text-red-400"
                    >
                      {t("editprofile.remover")}
                    </button>
                  )}
                </div>
              </div>
            </div>
          </div>
        )}

        {section === "fundo" && (
          <div className="mx-auto max-w-4xl">
            <div className="rounded-2xl border border-white/[0.08] bg-white/[0.04] p-8 shadow-2xl shadow-black/30 backdrop-blur-xl">
              <div className="mb-8">
                <h3 className="mb-2 text-2xl font-bold text-white">{t("editprofile.nav.fundo")}</h3>
                <p className="text-sm text-[#8a93a6]">{t("editprofile.fundo_desc")}</p>
              </div>

              {/* Preview do fundo */}
              <div
                className="relative mb-6 flex items-center justify-center overflow-hidden rounded-2xl"
                style={{
                  aspectRatio: "21/9",
                  background: profile.background
                    ? undefined
                    : "linear-gradient(135deg, #12121a, #05050a)",
                  border: "1px solid rgba(255,255,255,0.1)",
                }}
              >
                {profile.background ? (
                  <>
                    <img src={profile.background} alt="" className="h-full w-full object-cover" />
                    <div className="absolute inset-0 bg-gradient-to-t from-black/60 via-transparent to-transparent" />
                    {/* Miniatura do avatar sobreposto */}
                    <div className="absolute bottom-6 left-6 flex items-center gap-4">
                      <div className="flex h-20 w-20 items-center justify-center overflow-hidden rounded-xl bg-gradient-to-br from-[#0072ce] to-[#003791] text-2xl font-bold text-white ring-4 ring-black/50">
                        {profile.avatar ? (
                          <img src={profile.avatar} alt="" className="h-full w-full object-cover" />
                        ) : (
                          (fields.name?.[0] || "J").toUpperCase()
                        )}
                      </div>
                      <div>
                        <div className="text-lg font-bold text-white drop-shadow-lg">{fields.name || t("profile.jogador")}</div>
                        <div className="text-xs text-white/80 drop-shadow">{t("profile.dono")}</div>
                      </div>
                    </div>
                  </>
                ) : (
                  <div className="flex flex-col items-center gap-3 py-12">
                    <div className="flex h-16 w-16 items-center justify-center rounded-full bg-white/[0.04]">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-8 w-8 text-[#6b7280]">
                        <rect x="3" y="3" width="18" height="18" rx="3" />
                        <circle cx="9" cy="9" r="2" />
                        <path d="m21 15-4.5-4.5L7 20" />
                      </svg>
                    </div>
                    <span className="text-sm text-[#6b7280]">{t("editprofile.sem_fundo")}</span>
                  </div>
                )}
              </div>

              <div className="flex flex-wrap gap-3">
                <button
                  onClick={() => pick("background")}
                  className="group relative overflow-hidden rounded-xl px-6 py-3 font-semibold text-white shadow-lg transition-all hover:scale-[1.02] hover:shadow-xl"
                  style={{ background: "linear-gradient(135deg, #0072ce, #005fa8)" }}
                >
                  <div className="absolute inset-0 bg-gradient-to-r from-transparent via-white/10 to-transparent opacity-0 transition-opacity group-hover:opacity-100" />
                  <div className="relative flex items-center gap-2">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-5 w-5">
                      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                      <polyline points="17 8 12 3 7 8" />
                      <line x1="12" y1="3" x2="12" y2="15" />
                    </svg>
                    {t("editprofile.escolher_imagem")}
                  </div>
                </button>
                {profile.background && (
                  <button
                    onClick={() => patch({ background: "" })}
                    className="rounded-xl border border-white/[0.12] px-6 py-2.5 text-sm font-medium text-[#c8d0e0] transition-all hover:border-red-500/30 hover:bg-red-500/5 hover:text-red-400"
                  >
                    {t("editprofile.remover")}
                  </button>
                )}
              </div>
            </div>
          </div>
        )}
        </div>
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
