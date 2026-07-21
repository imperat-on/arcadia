"use client"

import { useEffect, useRef, useState } from "react"
import type { Game } from "./types"
import { useGamepadNav } from "./useGamepadNav"
import { ArtSearch } from "./ArtSearch"
import { TextSearch } from "./TextSearch"
import { useI18n } from "../../i18n/I18nContext"

interface EditMetadataProps {
  game: Game | null
  onClose: () => void
  onSave: (patch: Record<string, unknown>) => void
}

// Estado de um campo de arte:
//   undefined = intocado · null = restaurar o original · string = novo caminho
type ArtDraft = string | null | undefined

// Tema por modo: no console (Big Picture) o editor fica no azul PS5 clássico;
// no desktop segue o tema aplicado. (Antes era um fundo só para os dois.)
const CONSOLE = typeof window !== "undefined" && window.launcherMode !== "desktop"
const BG_DIALOG = CONSOLE ? "rgba(10,22,54,0.98)" : "var(--sidebar-bg)"
const BG_HEADER = CONSOLE ? "rgba(0,0,0,0.5)" : "var(--bg)"
// Cores fixas do PS5 no console (o tema do desktop NÃO vaza pra cá — senão
// accent claro vira botão branco com texto branco).
const ACCENT = CONSOLE ? "#00a8ff" : "var(--accent)"
const ACCENT_TEXT = CONSOLE ? "#ffffff" : "#000000"
const MUTED = CONSOLE ? "#8a93a6" : "var(--muted)"

export function EditMetadata({ game, onClose, onSave }: EditMetadataProps) {
  const { t } = useI18n()
  const ref = useRef<HTMLDivElement>(null)
  const open = Boolean(game)
  useGamepadNav(ref, open, onClose)

  const [title, setTitle] = useState("")
  const [description, setDescription] = useState("")
  const [cover, setCover] = useState<ArtDraft>(undefined)
  const [hero, setHero] = useState<ArtDraft>(undefined)
  const [logo, setLogo] = useState<ArtDraft>(undefined)
  // Qual campo está com a busca online aberta.
  const [buscando, setBuscando] = useState<"cover" | "hero" | "logo" | null>(null)
  const [buscandoTexto, setBuscandoTexto] = useState(false)

  // Recarrega o rascunho sempre que abrir para outro jogo.
  useEffect(() => {
    if (!game) return
    setTitle(game.title ?? "")
    setDescription(game.description ?? "")
    setCover(undefined)
    setHero(undefined)
    setLogo(undefined)
  }, [game])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, onClose])

  if (!game) return null

  const aplicar = (kind: "cover" | "hero" | "logo", path: string) => {
    if (kind === "cover") setCover(path)
    else if (kind === "hero") setHero(path)
    else setLogo(path)
  }

  const pick = async (kind: "cover" | "hero" | "logo") => {
    const res = await window.launcherAPI?.pickArt(game.id, kind)
    if (!res?.ok || !res.path) return
    aplicar(kind, res.path)
  }

  const save = () => {
    const patch: Record<string, unknown> = {}
    if (title.trim() && title !== game.title) patch.title = title.trim()
    if (description !== (game.description ?? "")) patch.description = description
    if (cover !== undefined) patch.cover = cover
    if (hero !== undefined) patch.hero = hero
    if (logo !== undefined) patch.logo = logo
    onSave(patch)
    onClose()
  }

  return (
    <>
      <div className="fixed inset-0 z-40" style={{ background: "rgba(0,0,0,0.65)" }} />

      <div
        ref={ref}
        className="gp-scope fixed z-50 left-1/2 top-1/2 w-[720px] max-h-[86vh] overflow-y-auto rounded-2xl"
        style={{
          transform: "translate(-50%, -50%)",
          background: BG_DIALOG,
          border: "1px solid rgba(255,255,255,0.14)",
          boxShadow: "0 24px 70px rgba(0,0,0,0.7)",
          backdropFilter: "blur(16px)",
          color: CONSOLE ? "#fff" : "var(--text)",
        }}
        role="dialog"
        aria-label={t("editmetadata.editar_metadados", { title: game.title })}
      >
        <div
          className="px-6 py-4 text-[15px] font-semibold tracking-wide uppercase truncate sticky top-0 z-10"
          style={{ background: BG_HEADER, color: CONSOLE ? "#fff" : "var(--text)", backdropFilter: "blur(8px)" }}
        >
          {t("editmetadata.titulo")} — {game.title}
        </div>

        <div className="p-6 flex flex-col gap-5">
          <Field label={t("editmetadata.titulo")}>
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              className="w-full px-3 py-2.5 rounded-lg text-white text-[15px] outline-none"
              style={{
                background: "var(--card-bg)",
                border: "1px solid rgba(255,255,255,0.12)",
              }}
            />
          </Field>

          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <span className="text-[13px] font-semibold tracking-wider uppercase text-white/60">
                {t("editmetadata.descricao")}
              </span>
              <button
                onClick={() => setBuscandoTexto(true)}
                className="px-3 py-1 rounded-md text-[12px] font-semibold text-white transition-transform hover:scale-[1.03]"
                style={{ background: ACCENT, color: ACCENT_TEXT }}
              >
                {t("editmetadata.buscar_online")}
              </button>
            </div>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={4}
              className="w-full px-3 py-2.5 rounded-lg text-white text-[15px] outline-none resize-none leading-relaxed"
              style={{
                background: "var(--card-bg)",
                border: "1px solid rgba(255,255,255,0.12)",
              }}
            />
          </div>

          <div className="grid grid-cols-3 gap-4">
            <ArtField
              kind="cover"
              draft={cover}
              atual={game.cover}
              aspect="2 / 3"
              onPick={() => pick("cover")}
              onSearch={() => setBuscando("cover")}
              onReset={() => setCover(null)}
            />
            <ArtField
              kind="hero"
              draft={hero}
              atual={game.hero}
              aspect="16 / 9"
              onPick={() => pick("hero")}
              onSearch={() => setBuscando("hero")}
              onReset={() => setHero(null)}
            />
            <ArtField
              kind="logo"
              draft={logo}
              atual={game.logo}
              aspect="16 / 9"
              onPick={() => pick("logo")}
              onSearch={() => setBuscando("logo")}
              onReset={() => setLogo(null)}
            />
          </div>

          <div className="flex items-center justify-end gap-3 pt-1">
            <button
              onClick={onClose}
              className="px-6 py-2.5 rounded-lg text-[15px] font-medium text-white transition-colors hover:bg-white/15"
              style={{ background: "rgba(255,255,255,0.08)" }}
            >
              {t("common.cancelar")}
            </button>
            <button
              onClick={save}
              className="px-7 py-2.5 rounded-lg text-[15px] font-semibold text-white transition-transform hover:scale-[1.03]"
              style={{ background: ACCENT, color: ACCENT_TEXT }}
            >
              {t("common.salvar")}
            </button>
          </div>
        </div>
      </div>

      {/* Busca online, por cima do editor */}
      {buscando && (
        <ArtSearch
          gameId={game.id}
          titulo={game.title}
          kind={buscando}
          onClose={() => setBuscando(null)}
          onPicked={(path) => aplicar(buscando, path)}
        />
      )}
      {buscandoTexto && (
        <TextSearch
          gameId={game.id}
          titulo={game.title}
          onClose={() => setBuscandoTexto(false)}
          onPicked={setDescription}
        />
      )}
    </>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-2">
      <span className="text-[13px] font-semibold tracking-wider uppercase text-white/60">
        {label}
      </span>
      {children}
    </label>
  )
}

function ArtField({
  kind,
  draft,
  atual,
  aspect,
  onPick,
  onSearch,
  onReset,
}: {
  kind: "cover" | "hero" | "logo"
  draft: ArtDraft
  atual?: string
  aspect: string
  onPick: () => void
  onSearch: () => void
  onReset: () => void
}) {
  const { t } = useI18n()
  // Rascunho vence a arte atual. O caminho vem cru do Electron: vira file://
  // só para a prévia (o que é salvo no disco continua sem prefixo).
  const preview =
    draft === null ? null : draft ? "file://" + draft : atual || null

  const labelMap: Record<string, string> = {
    cover: t("editmetadata.capa"),
    hero: t("editmetadata.fundo"),
    logo: t("editmetadata.logo"),
  }

  return (
    <div className="flex flex-col gap-2">
      <span className="text-[13px] font-semibold tracking-wider uppercase text-white/60">
        {labelMap[kind]}
      </span>
      <div
        className="w-full rounded-lg overflow-hidden flex items-center justify-center text-center"
        style={{
          aspectRatio: aspect,
          background: "var(--card-bg)",
          border: "1px solid rgba(255,255,255,0.10)",
        }}
      >
        {preview ? (
          <img src={preview} alt="" className="w-full h-full object-cover" />
        ) : (
          <span className="text-[11px] text-white/60 px-2 leading-tight">
            {draft === null ? t("editmetadata.volta_original") : t("editmetadata.sem_imagem")}
          </span>
        )}
      </div>
      <button
        onClick={onSearch}
        className="w-full px-2 py-1.5 rounded-md text-[12px] font-semibold text-white transition-transform hover:scale-[1.03]"
        style={{ background: ACCENT, color: ACCENT_TEXT }}
      >
        {t("editmetadata.buscar_online")}
      </button>
      <div className="flex gap-2">
        <button
          onClick={onPick}
          title={t("editmetadata.usar_do_disco")}
          className="flex-1 px-2 py-1.5 rounded-md text-[12px] font-medium text-white transition-colors hover:bg-white/15"
          style={{ background: "rgba(255,255,255,0.08)" }}
        >
          {t("editmetadata.do_disco")}
        </button>
        <button
          onClick={onReset}
          title={t("editmetadata.voltar_arte_original")}
          className="px-2 py-1.5 rounded-md text-[12px] font-medium text-white/60 transition-colors hover:bg-white/10 hover:text-white"
          style={{ background: "rgba(255,255,255,0.04)" }}
        >
          {t("editmetadata.restaurar")}
        </button>
      </div>
    </div>
  )
}
