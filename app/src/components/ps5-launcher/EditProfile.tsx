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
  onChange: (p: Profile) => void
}

type Section = "geral" | "avatar" | "fundo" | "destaques"

const MAX_SHOWCASE = 8

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
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose()
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, onClose])

  if (!open) return null

  const patch = async (p: Partial<Profile>) => {
    onChange({ ...profile, ...p })
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
    if (timer.current) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(
      () => patch({ [k]: k === "name" ? v.trim() || t("profile.padrao_jogador") : v }),
      450,
    )
  }

  const [cropSrc, setCropSrc] = useState<string | null>(null)

  // Upload do avatar pra conta (comum aos fluxos direto e recortado)
  const subirAvatar = async (pathOuBytes: string | Uint8Array, mime?: string, ext?: string) => {
    if (!conta) return
    const up =
      typeof pathOuBytes === "string"
        ? await conta.setAvatar(pathOuBytes)
        : await conta.setAvatarBytes(pathOuBytes, mime || "image/png", ext || ".png")
    if (up.ok && up.avatar_url) {
      onChange({ ...profile, avatar: up.avatar_url })
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
        if (/\.gif$/i.test(r.path)) {
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
      onChange({ ...profile, [key]: r.path })
    }
  }

  const NAV: { id: Section; label: string }[] = [
    { id: "geral", label: t("editprofile.nav.geral") },
    { id: "avatar", label: t("editprofile.nav.avatar") },
    { id: "fundo", label: t("editprofile.nav.fundo") },
    { id: "destaques", label: t("editprofile.nav.destaques") },
  ]

  const showcase = profile.showcase ?? []
  const toggleShowcase = (id: string) => {
    let next: string[]
    if (showcase.includes(id)) {
      next = showcase.filter((x) => x !== id)
    } else if (showcase.length >= MAX_SHOWCASE) {
      return // limite atingido
    } else {
      next = [...showcase, id]
    }
    patch({ showcase: next })
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
      <aside className="relative m-4 flex w-72 shrink-0 flex-col gap-1 rounded-3xl border border-white/[0.08] bg-white/[0.04] p-5 shadow-2xl shadow-black/50 backdrop-blur-xl">
        <button
          onClick={onClose}
          className="text-sm text-[#8a93a6] hover:text-white transition-colors mb-4 text-left flex items-center gap-2"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
            <path d="M15 18l-6-6 6-6" />
          </svg>
          {t("editprofile.voltar_perfil")}
        </button>
        <h1 className="px-2 text-xl font-bold text-white">{t("profile.editar_perfil")}</h1>
        <div className="mx-1 my-4 rounded-2xl border border-white/[0.08] bg-black/25 p-4">
          <div className="flex items-center gap-3">
            <div className="flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded-2xl bg-gradient-to-br from-[#0072ce] to-[#003791] text-xl font-bold text-white ring-1 ring-white/15">
              {profile.avatar ? (
                <img src={profile.avatar} alt="" className="h-full w-full object-cover" />
              ) : (
                (fields.name?.[0] || "J").toUpperCase()
              )}
            </div>
            <div className="min-w-0">
              <div className="truncate text-sm font-semibold text-white">
                {fields.name || t("profile.jogador")}
              </div>
              <div className="text-xs text-[#8a93a6]">{t("profile.dono")}</div>
            </div>
          </div>
        </div>
        {NAV.map((n) => {
          const active = section === n.id
          return (
            <button
              key={n.id}
              onClick={() => setSection(n.id)}
              className="rounded-2xl px-4 py-3 text-left text-[15px] font-medium transition-colors"
              style={{
                color: active ? "#fff" : "#8a93a6",
                background: active
                  ? "linear-gradient(135deg, rgba(255,255,255,0.13), rgba(255,255,255,0.06))"
                  : "transparent",
                border: active ? "1px solid rgba(255,255,255,0.1)" : "1px solid transparent",
              }}
            >
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
                    selected: String(showcase.length),
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
                  const idx = showcase.indexOf(g.id)
                  const selected = idx !== -1
                  const full = !selected && showcase.length >= MAX_SHOWCASE
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
