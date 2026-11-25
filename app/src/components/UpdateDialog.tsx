"use client"

import { useEffect, useRef, useState } from "react"
import type { UpdateEtapa, UpdateInfo, UpdatePackagedState } from "../global"
import { useI18n } from "../i18n/I18nContext"
import { useGamepadNav } from "./ps5-launcher/useGamepadNav"
import { fmtBytes } from "./tamanho"
import { Modal } from "../ui/Modal"

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
        style={{ background: modoConsole ? "rgba(10,12,20,0.98)" : "var(--surface-1)" }}
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

        {erro && <p className="mb-3 text-[12px] text-[color:var(--state-danger-soft)]">{erro}</p>}

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

const RELEASES_URL = "https://github.com/imperat-on/arcadia/releases"
const BOTAO_PRIMARIO =
  "rounded-lg px-5 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03]"
const BOTAO_SECUNDARIO =
  "rounded-lg border border-white/15 px-5 py-2.5 text-[12px] font-semibold text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white"

function BotaoPrimario({ onClick, children }: { onClick: () => void; children: string }) {
  return (
    <button onClick={onClick} className={BOTAO_PRIMARIO} style={{ background: "var(--accent)" }}>
      {children}
    </button>
  )
}

interface UpdatePackagedDialogProps {
  estado: UpdatePackagedState
  /** Big Picture: liga a navegação por controle e escurece mais o fundo. */
  console?: boolean
  onBaixar: () => void
  onInstalar: () => void
  onTentar: () => void
  onDepois: () => void
}

// Diálogo do canal empacotado, montado sobre o primitivo `src/ui/Modal.tsx`
// (portal, Esc, foco preso, scroll lock e gamepad saem de graça — N6). As fases
// são as do electron-updater: disponivel/baixando/pronto/erro/sem_suporte.
export function UpdatePackagedDialog({
  estado,
  console: modoConsole = false,
  onBaixar,
  onInstalar,
  onTentar,
  onDepois,
}: UpdatePackagedDialogProps) {
  const { t } = useI18n()
  const baixando = estado.fase === "baixando"
  const erro = estado.fase === "erro"
  const semSuporte = estado.fase === "sem_suporte"
  // N8: erro sem versão para baixar (ex.: checagem manual que falhou) não pode
  // oferecer "Baixar" — `baixar()` devolveria `sem_versao` e repetiria o erro.
  // N7: erro de instalação re-tenta o check (o cache pendente volta a "pronto"
  // e o usuário escolhe Reiniciar de novo), com o link sempre à mão.
  const retry =
    erro && estado.erroAcao === "baixar"
      ? "baixar"
      : erro
        ? "tentar"
        : semSuporte
          ? "release"
          : estado.fase === "pronto"
            ? "instalar"
            : "baixar"

  const versao = estado.versaoNova || estado.versaoAtual
  const subtitulo =
    semSuporte
      ? t("update.packaged.sem_suporte")
      : estado.fase === "disponivel"
        ? estado.tamanho
          ? t("update.packaged.disponivel_tamanho", { versao, tamanho: fmtBytes(estado.tamanho) })
          : t("update.packaged.disponivel", { versao })
        : baixando
          ? t("update.packaged.baixando", { pct: estado.progresso })
          : estado.fase === "pronto"
            ? t("update.packaged.pronto")
            : estado.erroAcao === "instalar"
              ? t("update.packaged.erro_instalar")
              : t("update.packaged.erro")

  const primario =
    retry === "release" ? (
      <BotaoPrimario onClick={() => window.launcherAPI?.openExternal(RELEASES_URL)}>
        {t("update.packaged.abrir_release")}
      </BotaoPrimario>
    ) : retry === "instalar" ? (
      <BotaoPrimario onClick={onInstalar}>{t("update.packaged.reiniciar")}</BotaoPrimario>
    ) : retry === "tentar" ? (
      <BotaoPrimario onClick={onTentar}>{t("update.packaged.tentar")}</BotaoPrimario>
    ) : (
      <BotaoPrimario onClick={onBaixar}>{t("update.packaged.baixar")}</BotaoPrimario>
    )

  return (
    <Modal
      open
      onClose={baixando ? () => {} : onDepois}
      title={t("update.packaged.titulo")}
      description={subtitulo}
      size="sm"
      gamepad={modoConsole}
      closeOnBackdrop={!baixando}
      showClose={!baixando}
      footer={
        baixando ? null : (
          <div className="flex justify-end gap-2.5">
            <button onClick={onDepois} className={BOTAO_SECUNDARIO}>
              {t("update.packaged.depois")}
            </button>
            {erro && (
              <button
                onClick={() => window.launcherAPI?.openExternal(RELEASES_URL)}
                className={BOTAO_SECUNDARIO}
              >
                {t("update.packaged.abrir_release")}
              </button>
            )}
            {primario}
          </div>
        )
      }
    >
      {baixando && (
        <div className="h-2 overflow-hidden rounded-full bg-white/[0.08]">
          <div
            className="h-full rounded-full transition-[width] duration-300"
            style={{ width: `${estado.progresso}%`, background: "var(--accent)" }}
          />
        </div>
      )}
      {erro && estado.erro && <p className="text-[12px] text-white/40">{estado.erro}</p>}
    </Modal>
  )
}

/**
 * Assina o canal empacotado e decide quando o diálogo abre:
 * - abre: `disponivel` (se `!jaAvisado`), `pronto` (reaviso do boot, D6),
 *   `sem_suporte` (portable/zip/AppImage extraído, no boot) e `erro` de ação
 *   do usuário (`erroDeFundo === false`);
 * - silencia: `ocioso`, `baixando` (só continua um diálogo já aberto) e erro
 *   de fundo (sem rede no ciclo de 6h não pode virar aviso).
 * "Depois" grava a versão no config (D7) e não reabre nesta sessão.
 */
export function useAtualizacaoEmpacotada() {
  const [estado, setEstado] = useState<UpdatePackagedState | null>(null)
  const dispensados = useRef<Set<string>>(new Set())

  useEffect(() => {
    let vivo = true
    const chave = (e: UpdatePackagedState) => `${e.fase}:${e.versaoNova ?? e.versaoAtual}`
    const decidir = (e: UpdatePackagedState): UpdatePackagedState | null => {
      if (e.canal === "fonte") return null
      if (e.fase === "sem_suporte") return dispensados.current.has(chave(e)) ? null : e
      if (e.fase === "disponivel") return e.jaAvisado || dispensados.current.has(chave(e)) ? null : e
      if (e.fase === "pronto") return dispensados.current.has(chave(e)) ? null : e
      if (e.fase === "erro") return e.erroDeFundo ? null : e
      return null
    }
    window.launcherAPI
      ?.updatePackagedState?.()
      .then((e) => {
        if (!vivo || !e) return
        const aberto = decidir(e)
        if (aberto) setEstado(aberto)
      })
    const off = window.launcherAPI?.onUpdatePackagedChanged?.((e) => {
      if (!vivo || !e) return
      setEstado((atual) => {
        if (e.fase === "baixando") return atual ? { ...atual, ...e } : atual
        if (e.fase === "ocioso") return null
        return decidir(e)
      })
    })
    return () => {
      vivo = false
      off?.()
    }
  }, [])

  const dispensar = () => {
    setEstado((atual) => {
      if (atual) {
        dispensados.current.add(`${atual.fase}:${atual.versaoNova ?? atual.versaoAtual}`)
        if (atual.fase === "disponivel" && atual.versaoNova) {
          void window.launcherAPI?.updatePackagedJaAvisado?.(atual.versaoNova)
        }
      }
      return null
    })
  }

  const baixar = async () => {
    setEstado((atual) =>
      atual ? { ...atual, fase: "baixando", progresso: 0, erro: null, erroAcao: null } : atual,
    )
    const r = await window.launcherAPI?.updatePackagedDownload?.()
    if (!r?.ok) {
      setEstado((atual) =>
        atual
          ? { ...atual, fase: "erro", erro: r?.erro || "erro", erroDeFundo: false, erroAcao: "baixar" }
          : atual,
      )
    }
  }

  const instalar = async () => {
    const r = await window.launcherAPI?.updatePackagedInstall?.()
    if (!r?.ok) {
      setEstado((atual) =>
        atual
          ? { ...atual, fase: "erro", erro: r?.erro || "erro", erroDeFundo: false, erroAcao: "instalar" }
          : atual,
      )
    }
  }

  // "Tentar de novo" (N8): refaz a checagem manual. Se houver update, o push
  // `disponivel` reabre o diálogo; se houver pendente, a revalidação do cache
  // devolve "pronto" — o mesmo caminho vale para erro de instalação (N7).
  const tentar = async () => {
    const r = await window.launcherAPI?.updatePackagedCheck?.({ manual: true })
    if (!r?.ok) {
      setEstado((atual) =>
        atual
          ? { ...atual, fase: "erro", erro: r?.erro || "erro", erroDeFundo: false, erroAcao: "checar" }
          : atual,
      )
    }
  }

  return { estado, dispensar, baixar, instalar, tentar }
}
