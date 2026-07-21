"use client"

import { useEffect, useRef, useState } from "react"
import type { UpdateEtapa, UpdateInfo } from "../global"
import { useI18n } from "../i18n/I18nContext"
import { useGamepadNav } from "./ps5-launcher/useGamepadNav"

// Aviso de atualização do Arcadia, compartilhado pelos dois modos.
//
// O conteúdo é o mesmo nos dois — a lista de commits novos e dois botões — e
// duplicá-lo deixaria as duas telas divergindo com o tempo. O que muda é só o
// fundo (o console é mais escuro e opaco) e a navegação por controle, que só
// faz sentido no Big Picture.

interface UpdateDialogProps {
  info: UpdateInfo
  /** Big Picture: liga a navegação por controle e escurece mais o fundo. */
  console?: boolean
  onDepois: () => void
}

export function UpdateDialog({ info, console: modoConsole = false, onDepois }: UpdateDialogProps) {
  const { t } = useI18n()
  const ref = useRef<HTMLDivElement>(null)
  const [etapa, setEtapa] = useState<UpdateEtapa | null>(null)
  const [erro, setErro] = useState("")
  // "Ocultar novidades ao iniciar": quem ligou isso quer o aviso, não a lista.
  const [semLista, setSemLista] = useState(false)

  useEffect(() => {
    window.launcherAPI?.getConfig().then((c) => setSemLista(c?.hide_changelog_on_start === true))
  }, [])

  const aplicando = etapa !== null
  useGamepadNav(ref, modoConsole && !aplicando, aplicando ? () => {} : onDepois)

  useEffect(() => {
    return window.launcherAPI?.onUpdateProgress?.((p) => setEtapa(p.etapa))
  }, [])

  const aplicar = async () => {
    setErro("")
    setEtapa("pull")
    const r = await window.launcherAPI?.updateApply({ depsMudaram: info.depsMudaram })
    // Deu certo? O processo está sendo substituído — deixa a tela como está,
    // senão o diálogo pisca "pronto" e some antes de o novo app subir.
    if (r?.ok) return
    setEtapa(null)
    setErro(r?.error || t("update.erro_generico"))
  }

  const rotuloEtapa =
    etapa === "deps"
      ? t("update.etapa.deps")
      : etapa === "build"
        ? t("update.etapa.build")
        : etapa === "pronto"
          ? t("update.etapa.pronto")
          : t("update.etapa.pull")

  return (
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center backdrop-blur-sm"
      style={{ background: modoConsole ? "rgba(0,0,0,0.85)" : "rgba(0,0,0,0.6)" }}
    >
      <div
        ref={ref}
        className="gp-scope w-[460px] max-w-[92vw] rounded-2xl border border-white/[0.08] p-6 shadow-2xl"
        style={{ background: modoConsole ? "rgba(10,12,20,0.98)" : "#0d0d10" }}
        role="dialog"
        aria-label={t("update.titulo")}
      >
        <h3 className="mb-1 text-lg font-semibold text-white">{t("update.titulo")}</h3>
        <p className="mb-4 text-[13px] text-white/60">
          {t("update.subtitulo", { n: info.atrasado ?? 0 })}
        </p>

        {/* As mensagens dos commits são o changelog: sem elas, o usuário
            aceitaria uma atualização às cegas. */}
        {!semLista && (info.commits || []).length > 0 && (
          <ul className="mb-4 max-h-[220px] overflow-y-auto rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
            {(info.commits || []).map((c) => (
              <li key={c.sha} className="flex gap-2 py-1 text-[13px] leading-snug text-white/80">
                <span className="shrink-0 font-mono text-[11px] text-white/35">{c.sha}</span>
                <span>{c.titulo}</span>
              </li>
            ))}
          </ul>
        )}

        {info.depsMudaram && (
          <p className="mb-3 text-[12px] text-white/45">{t("update.deps_aviso")}</p>
        )}

        {erro && <p className="mb-3 text-[12px] text-[#ffa07a]">{erro}</p>}

        {aplicando ? (
          <p className="py-1 text-[13px] text-white/70">{rotuloEtapa}</p>
        ) : (
          <div className="flex justify-end gap-2.5">
            <button
              onClick={onDepois}
              className="rounded-lg border border-white/15 px-5 py-2.5 text-[12px] font-semibold text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white"
            >
              {t("update.depois")}
            </button>
            <button
              onClick={aplicar}
              className="rounded-lg px-5 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03]"
              style={{ background: "var(--accent)" }}
            >
              {t("update.aplicar")}
            </button>
          </div>
        )}
      </div>
    </div>
  )
}

/** Assina o aviso do main. Os dois modos montam o diálogo com isto. */
export function useAtualizacao() {
  const [info, setInfo] = useState<UpdateInfo | null>(null)
  useEffect(() => {
    return window.launcherAPI?.onUpdateAvailable?.((i) => setInfo(i))
  }, [])
  return { info, dispensar: () => setInfo(null) }
}
