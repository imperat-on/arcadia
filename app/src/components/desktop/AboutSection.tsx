"use client"

import { useEffect, useState } from "react"
import type { UpdateInfo, UpdatePackagedState } from "../../global"
import { useI18n } from "../../i18n/I18nContext"
import { UpdateDialog } from "../UpdateDialog"

const REPO_URL = "https://github.com/imperat-on/arcadia"
const RELEASES_URL = "https://github.com/imperat-on/arcadia/releases"

// Canal do updater → chave do rótulo legível.
const CANAIS: Record<string, string> = {
  appimage: "about.canal.appimage",
  nsis: "about.canal.nsis",
  portable: "about.canal.portable",
  zip: "about.canal.zip",
  fonte: "about.canal.fonte",
}

// Sobre: versão visível, canal, estado do updater, links e créditos.
// O mesmo componente serve o Big Picture (via SettingsPanel) — lá dentro de um
// modal, sem o cabeçalho.
export function AboutSection({ console: modoConsole = false }: { console?: boolean }) {
  const { t } = useI18n()
  const [estado, setEstado] = useState<UpdatePackagedState | null>(null)
  const [msg, setMsg] = useState("")
  const [busy, setBusy] = useState(false)
  const [info, setInfo] = useState<UpdateInfo | null>(null)
  // Canal git: estado real (bloqueio local ou atraso) para a linha.
  const [gitEstado, setGitEstado] = useState("")
  // Versão de reserva: o fallback do estado empacotado pode vir sem versão.
  const [versaoDiag, setVersaoDiag] = useState("")

  // Canal git: bloqueio local (updateState) ou atraso real (updateCheck).
  // Devolve o UpdateInfo junto para o diálogo do botão.
  const lerGit = async (): Promise<{ bloqueio?: string; erro?: string; info?: UpdateInfo }> => {
    const st = await window.launcherAPI?.updateState()
    if (st && !st.podeAtualizar) {
      return { bloqueio: t(`update.bloqueado.${st.motivo}`, { detalhe: st.detalhe || "" }) }
    }
    const r = await window.launcherAPI?.updateCheck()
    if (!r?.ok) return { erro: r?.error || t("update.erro_generico") }
    return { info: r }
  }

  useEffect(() => {
    let vivo = true
    window.launcherAPI
      ?.updatePackagedState?.()
      .then((e) => {
        if (vivo && e) setEstado(e)
      })
      .catch(() => {})
    const off = window.launcherAPI?.onUpdatePackagedChanged?.((e) => setEstado(e))
    return () => {
      vivo = false
      off?.()
    }
  }, [])

  // O fallback ESTADO_EMPACOTADO_FONTE vem com versaoAtual vazia; sem isto a
  // tela ficaria num travessão. diagnostics() é o outro caminho com a versão.
  useEffect(() => {
    if (!estado || estado.versaoAtual) return
    let vivo = true
    window.launcherAPI
      ?.diagnostics?.()
      .then((d) => {
        if (vivo && d?.app?.version) setVersaoDiag(d.app.version)
      })
      .catch(() => {})
    return () => {
      vivo = false
    }
  }, [estado?.versaoAtual])

  // Canal fonte: sem isto a linha ficava "Em dia" num clone atrasado até o
  // usuário apertar o botão. updateState/updateCheck dão o estado de verdade.
  useEffect(() => {
    if (estado?.canal !== "fonte") return
    let vivo = true
    void (async () => {
      const g = await lerGit()
      if (!vivo) return
      if (g.bloqueio) setGitEstado(g.bloqueio)
      else if (g.info?.atrasado) setGitEstado(t("update.subtitulo", { n: g.info.atrasado }))
    })()
    return () => {
      vivo = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [estado?.canal])

  const versao = estado?.versaoAtual || versaoDiag
  const canal = estado ? t(CANAIS[estado.canal] || "about.canal.sem_suporte") : "—"

  const linhaEstado = (e: UpdatePackagedState): string => {
    switch (e.fase) {
      case "disponivel":
        return t("update.packaged.disponivel", { versao: e.versaoNova || "" })
      case "baixando":
        return t("update.packaged.baixando", { pct: e.progresso })
      case "pronto":
        return t("update.packaged.pronto")
      case "sem_suporte":
        return t("update.packaged.sem_suporte")
      case "erro":
        return e.erro || t("update.packaged.erro")
      default:
        return t("about.em_dia")
    }
  }

  // Mesma ramificação do botão das Configurações Gerais: canal empacotado usa
  // o updatePackagedCheck; `fonte` segue o fluxo do Git (as mensagens já
  // existem — nada de texto duplicado aqui).
  const procurar = async () => {
    setBusy(true)
    setMsg("")
    const canalAtual = estado?.canal ?? (await window.launcherAPI?.updatePackagedState())?.canal
    if (canalAtual && canalAtual !== "fonte") {
      const r = await window.launcherAPI?.updatePackagedCheck({ manual: true })
      setBusy(false)
      if (r?.motivo === "canal_nao_suportado") return setMsg(t("update.packaged.sem_suporte"))
      if (!r?.ok) return setMsg(r?.erro || t("update.erro_generico"))
      if (!r.disponivel) return setMsg(t("update.packaged.em_dia"))
      return
    }
    const g = await lerGit()
    setBusy(false)
    if (g.erro) return setMsg(g.erro)
    if (g.bloqueio) {
      setGitEstado(g.bloqueio)
      return setMsg(g.bloqueio)
    }
    if (!g.info?.atrasado) {
      setGitEstado("")
      return setMsg(t("update.em_dia", { sha: g.info?.local || "" }))
    }
    setGitEstado(t("update.subtitulo", { n: g.info.atrasado }))
    setInfo(g.info)
  }

  return (
    <div className="max-w-2xl">
      {!modoConsole && (
        <>
          <h2 className="text-3xl font-light tracking-wide text-white mb-1">
            {t("about.titulo")}
          </h2>
          <p className="text-sm text-[color:var(--text-2)] mb-8">{t("about.desc")}</p>
        </>
      )}

      <div className="mb-8 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5">
        <div className="text-2xl font-light tracking-wide text-white">
          {versao ? t("about.versao", { versao }) : t("about.versao_indisponivel")}
        </div>
        <div className="mt-1 text-sm text-[color:var(--text-2)]">
          {t("about.canal_label")}: {canal}
        </div>
        <div className="mt-5 flex items-center justify-between gap-3">
          <span className="text-sm text-white/80">
            {estado
              ? estado.canal === "fonte"
                ? gitEstado || t("about.em_dia")
                : linhaEstado(estado)
              : ""}
          </span>
          <button
            onClick={procurar}
            disabled={busy}
            className="shrink-0 rounded-xl border border-white/10 bg-[color:var(--surface-2)] px-3.5 py-1.5 text-[12px] text-white/70 outline-none transition-colors hover:border-white/25 hover:text-white disabled:opacity-60"
          >
            {busy ? t("update.procurando") : t("about.procurar")}
          </button>
        </div>
        {msg && <p className="mt-3 text-xs leading-snug text-[color:var(--text-2)]">{msg}</p>}
      </div>

      <div className="mb-8 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5">
        <h3 className="mb-3 text-base font-semibold text-white">{t("about.links")}</h3>
        <div className="flex flex-col gap-2">
          {[
            { url: REPO_URL, label: t("about.repositorio") },
            { url: RELEASES_URL, label: t("about.releases") },
          ].map((l) => (
            <button
              key={l.url}
              onClick={() => window.launcherAPI?.openExternal(l.url)}
              className="flex items-center gap-2 rounded-xl border border-white/10 bg-[color:var(--surface-2)] px-3.5 py-2 text-left text-sm text-white/80 transition-colors hover:border-white/25 hover:text-white"
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinecap="round"
                strokeLinejoin="round"
                className="shrink-0"
              >
                <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
                <polyline points="15 3 21 3 21 9" />
                <line x1="10" y1="14" x2="21" y2="3" />
              </svg>
              {l.label}
            </button>
          ))}
        </div>
      </div>

      <p className="text-xs text-[color:var(--text-3)]">{t("about.creditos")}</p>

      {info && <UpdateDialog info={info} console={modoConsole} onDepois={() => setInfo(null)} />}
    </div>
  )
}
