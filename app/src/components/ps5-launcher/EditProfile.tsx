"use client"

import { useEffect, useRef, useState } from "react"
import type { Profile } from "../../global"
import type { Game } from "./types"
import { useGamepadNav } from "./useGamepadNav"

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
const COUNTRIES = [
  "Brasil", "Portugal", "Estados Unidos", "Argentina", "Chile", "México",
  "Espanha", "Reino Unido", "Alemanha", "França", "Japão", "Canadá",
]

export function EditProfile({ open, profile, games, onClose, onChange }: EditProfileProps) {
  const [section, setSection] = useState<Section>("geral")
  const [fields, setFields] = useState({
    name: "", realName: "", country: "", city: "", summary: "",
  })
  const timer = useRef<number | null>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  useGamepadNav(rootRef, open, onClose)

  useEffect(() => {
    if (!open) return
    setFields({
      name: profile.name ?? "Jogador",
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
  }

  const setField = (k: keyof typeof fields, v: string) => {
    setFields((f) => ({ ...f, [k]: v }))
    if (timer.current) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(
      () => patch({ [k]: k === "name" ? v.trim() || "Jogador" : v }),
      450,
    )
  }

  const pick = async (kind: "avatar" | "background") => {
    const r = await window.launcherAPI?.pickImage(kind)
    if (r?.ok && r.path) {
      // O main já salvou o caminho limpo no config; aqui só atualizamos a
      // visualização (com ?t= para refletir na hora).
      const key = kind === "avatar" ? "avatar" : "background"
      onChange({ ...profile, [key]: r.path })
    }
  }

  const NAV: { id: Section; label: string }[] = [
    { id: "geral", label: "Geral" },
    { id: "avatar", label: "Avatar" },
    { id: "fundo", label: "Plano de fundo" },
    { id: "destaques", label: "Destaques" },
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
    <div ref={rootRef} className="gp-scope fixed inset-0 z-[60] flex" style={{ background: "#000000" }}>
      {/* brilho topo */}
      <div className="absolute top-0 inset-x-0 h-80 pointer-events-none"
        style={{ background: "radial-gradient(ellipse 55% 100% at 50% 0%, rgba(75,40,120,0.3), transparent 70%)" }} />

      {/* Sidebar */}
      <aside className="w-72 flex flex-col p-6 gap-1 shrink-0 relative"
        style={{ background: "rgba(10,10,12,0.9)", borderRight: "1px solid rgba(255,255,255,0.06)" }}>
        <button onClick={onClose}
          className="text-sm text-[#8a93a6] hover:text-white transition-colors mb-4 text-left flex items-center gap-2">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M15 18l-6-6 6-6" /></svg>
          Voltar ao perfil
        </button>
        <h1 className="text-xl font-bold text-white mb-4 px-2">Editar perfil</h1>
        {NAV.map((n) => {
          const active = section === n.id
          return (
            <button key={n.id} onClick={() => setSection(n.id)}
              className="px-4 py-3 rounded-xl text-[15px] font-medium transition-colors text-left"
              style={{ color: active ? "#fff" : "#8a93a6", background: active ? "rgba(255,255,255,0.08)" : "transparent" }}>
              {n.label}
            </button>
          )
        })}
      </aside>

      {/* Conteúdo */}
      <main className="flex-1 overflow-y-auto p-10 relative">
        {section === "geral" && (
          <div className="max-w-2xl space-y-6">
            <div>
              <h2 className="text-2xl font-bold text-white mb-1">Geral</h2>
              <p className="text-sm text-[#8a93a6]">Nome e detalhes do seu perfil.</p>
            </div>
            <Field label="Nome do perfil">
              <input value={fields.name} onChange={(e) => setField("name", e.target.value)}
                className={INPUT_CLS} style={INPUT_STYLE} />
            </Field>
            <Field label="Nome real">
              <input value={fields.realName} onChange={(e) => setField("realName", e.target.value)}
                placeholder="Opcional" className={INPUT_CLS} style={INPUT_STYLE} />
            </Field>
            <div className="grid grid-cols-2 gap-4">
              <Field label="País">
                <select value={fields.country} onChange={(e) => setField("country", e.target.value)}
                  className={INPUT_CLS} style={INPUT_STYLE}>
                  <option value="">(Não exibir)</option>
                  {COUNTRIES.map((c) => <option key={c} value={c} style={{ background: "#0d0d0f" }}>{c}</option>)}
                </select>
              </Field>
              <Field label="Cidade">
                <input value={fields.city} onChange={(e) => setField("city", e.target.value)}
                  placeholder="Opcional" className={INPUT_CLS} style={INPUT_STYLE} />
              </Field>
            </div>
            <Field label="Resumo">
              <textarea value={fields.summary} onChange={(e) => setField("summary", e.target.value)}
                placeholder="Fale um pouco sobre você…" rows={4}
                className={INPUT_CLS + " resize-none"} style={INPUT_STYLE} />
            </Field>
          </div>
        )}

        {section === "avatar" && (
          <div className="max-w-2xl space-y-6">
            <div>
              <h2 className="text-2xl font-bold text-white mb-1">Avatar</h2>
              <p className="text-sm text-[#8a93a6]">Sua foto de perfil. GIFs animados funcionam.</p>
            </div>
            <div className="flex items-center gap-6">
              <div className="w-32 h-32 rounded-2xl overflow-hidden flex items-center justify-center text-4xl font-bold text-white"
                style={{ background: "linear-gradient(135deg, #0072ce, #003791)", border: "2px solid rgba(255,255,255,0.15)" }}>
                {profile.avatar
                  ? <img src={profile.avatar} alt="" className="w-full h-full object-cover" />
                  : (profile.name?.[0] || "J").toUpperCase()}
              </div>
              <div className="flex flex-col gap-2">
                <button onClick={() => pick("avatar")}
                  className="px-5 py-2.5 rounded-xl text-sm font-semibold text-white"
                  style={{ background: "linear-gradient(135deg, #0072ce, #005fa8)" }}>
                  Escolher imagem / GIF
                </button>
                {profile.avatar && (
                  <button onClick={() => patch({ avatar: "" })}
                    className="px-5 py-2 rounded-xl text-sm font-medium text-[#c8d0e0] hover:bg-white/5"
                    style={{ border: "1px solid rgba(255,255,255,0.12)" }}>
                    Remover
                  </button>
                )}
              </div>
            </div>
          </div>
        )}

        {section === "fundo" && (
          <div className="max-w-3xl space-y-6">
            <div>
              <h2 className="text-2xl font-bold text-white mb-1">Plano de fundo</h2>
              <p className="text-sm text-[#8a93a6]">
                Imagem de fundo do seu perfil — pode ser um <b>GIF animado</b>.
              </p>
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
              {profile.background
                ? <img src={profile.background} alt="" className="w-full h-full object-cover" />
                : <span className="text-sm text-[#6b7280]">Sem plano de fundo</span>}
            </div>
            <div className="flex gap-3">
              <button onClick={() => pick("background")}
                className="px-5 py-2.5 rounded-xl text-sm font-semibold text-white"
                style={{ background: "linear-gradient(135deg, #0072ce, #005fa8)" }}>
                Escolher imagem / GIF
              </button>
              {profile.background && (
                <button onClick={() => patch({ background: "" })}
                  className="px-5 py-2 rounded-xl text-sm font-medium text-[#c8d0e0] hover:bg-white/5"
                  style={{ border: "1px solid rgba(255,255,255,0.12)" }}>
                  Remover
                </button>
              )}
            </div>
          </div>
        )}

        {section === "destaques" && (
          <div className="max-w-5xl">
            <div className="mb-6">
              <h2 className="text-2xl font-bold text-white mb-1">Destaques</h2>
              <p className="text-sm text-[#8a93a6]">
                Escolha até {MAX_SHOWCASE} jogos para a vitrine do seu perfil.{" "}
                <span className="text-white font-medium">
                  {showcase.length}/{MAX_SHOWCASE} selecionados
                </span>
                . Clique para adicionar ou remover.
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
                          style={{ background: "#00a8ff", boxShadow: "0 0 8px rgba(0,168,255,0.6)" }}
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
