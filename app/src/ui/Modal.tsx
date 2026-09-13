"use client"

import { useCallback, useEffect, useId, useRef, type ReactNode } from "react"
import { createPortal } from "react-dom"
import { useMode } from "../components/ModeContext"
import { useGamepadNav } from "../components/ps5-launcher/useGamepadNav"
import { useI18n } from "../i18n/I18nContext"

// Modal: o ÚNICO overlay do app.
//
// Motivo de existir: 22 arquivos montavam o próprio overlay, cada um do seu
// jeito. O resultado medido antes deste primitivo:
//   backdrop reimplementado 22/22, role="dialog" 10/22, aria-modal 5/22,
//   Escape 7/22, foco no diálogo 2/22, scroll lock 0/22, createPortal 0/22.
// Ou seja: 15 modais não fechavam no Esc, 20 deixavam o Tab atrás do overlay e
// 12 não se anunciavam como diálogo. Também não havia portal, então cada um
// dependia de z-index alto para não ser recortado pelo pai (8 usavam z-80+).
//
// Aqui isso deixa de ser responsabilidade de quem chama: portal, backdrop, papel
// ARIA, Esc, foco preso + restaurado, travamento de scroll, camada z e a cor
// certa por skin. Quem chama cuida só do CONTEÚDO.
//
// A estética usa o vocabulário .ui-* que já existia no index.css (e que quase
// ninguém consumia), então o overlay passa a parecer da mesma peça que o resto.

export type ModalSize = "sm" | "md" | "lg" | "xl"

const LARGURA: Record<ModalSize, string> = {
  sm: "480px",
  md: "560px",
  lg: "620px",
  xl: "760px",
}

// Contador de modais abertos: dois modais empilhados não podem liberar o scroll
// do body quando o de baixo fecha.
let travas = 0

function travarScroll() {
  travas += 1
  if (travas === 1) {
    const anterior = document.body.style.overflow
    document.body.dataset.scrollAnterior = anterior
    document.body.style.overflow = "hidden"
  }
}

function liberarScroll() {
  travas = Math.max(0, travas - 1)
  if (travas === 0) {
    document.body.style.overflow = document.body.dataset.scrollAnterior || ""
    delete document.body.dataset.scrollAnterior
  }
}

export function Modal({
  open = true,
  onClose,
  title,
  description,
  size = "md",
  footer,
  children,
  /** Seletor do primeiro elemento a receber foco dentro do painel. */
  initialFocus,
  focusKey,
  /** Clicar no fundo fecha (padrão). Desligue em fluxos destrutivos. */
  closeOnBackdrop = true,
  /** Navegação por controle (B volta). Padrão: ligada no modo console. */
  gamepad,
  /** Botão X no canto. Padrão: aparece no desktop (no console o B/voltar basta). */
  showClose,
  className = "",
}: {
  open?: boolean
  onClose: () => void
  title?: ReactNode
  description?: ReactNode
  size?: ModalSize
  footer?: ReactNode
  children: ReactNode
  initialFocus?: string
  /** Reexecuta o foco inicial quando este valor muda (ex.: lista que chega depois). */
  focusKey?: unknown
  closeOnBackdrop?: boolean
  gamepad?: boolean
  showClose?: boolean
  className?: string
}) {
  const { isConsole } = useMode()
  const { t } = useI18n()
  const tituloId = useId()
  const descId = useId()
  const painelRef = useRef<HTMLDivElement>(null)
  const focoAnterior = useRef<HTMLElement | null>(null)
  const navControle = gamepad ?? isConsole
  const botaoFechar = showClose ?? !isConsole

  useGamepadNav(painelRef, open && navControle, onClose)

  const fechar = useCallback(
    (evento: React.SyntheticEvent) => {
      evento.stopPropagation()
      onClose()
    },
    [onClose],
  )

  // Scroll travado enquanto estiver aberto; foco guardado para devolver depois.
  useEffect(() => {
    if (!open) return
    travarScroll()
    focoAnterior.current = document.activeElement as HTMLElement | null
    return () => {
      liberarScroll()
      focoAnterior.current?.focus?.()
    }
  }, [open])

  // Foco entra no painel ao abrir (primeiro elemento indicado, senão o painel).
  useEffect(() => {
    if (!open) return
    const painel = painelRef.current
    if (!painel) return
    const alvo = initialFocus ? painel.querySelector<HTMLElement>(initialFocus) : null
    ;(alvo || painel).focus({ preventScroll: true })
  }, [open, initialFocus, focusKey])

  // Esc fecha e Tab circula dentro do painel — sem isso o foco vaza para a tela
  // atrás (o que acontecia em 20 dos 22 overlays antigos).
  useEffect(() => {
    if (!open) return
    const onKey = (evento: KeyboardEvent) => {
      if (evento.key === "Escape") {
        evento.stopPropagation()
        onClose()
        return
      }
      if (evento.key !== "Tab") return
      const painel = painelRef.current
      if (!painel) return
      const focaveis = painel.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
      )
      if (!focaveis.length) {
        evento.preventDefault()
        painel.focus({ preventScroll: true })
        return
      }
      const primeiro = focaveis[0]
      const ultimo = focaveis[focaveis.length - 1]
      const ativo = document.activeElement
      if (evento.shiftKey && (ativo === primeiro || ativo === painel)) {
        evento.preventDefault()
        ultimo.focus()
      } else if (!evento.shiftKey && ativo === ultimo) {
        evento.preventDefault()
        primeiro.focus()
      }
    }
    document.addEventListener("keydown", onKey, true)
    return () => document.removeEventListener("keydown", onKey, true)
  }, [open, onClose])

  if (!open) return null

  return createPortal(
    <div
      className={`gp-scope fixed inset-0 flex items-center justify-center backdrop-blur-sm ${
        isConsole ? "bg-black/85" : "bg-black/70"
      }`}
      style={{ zIndex: "var(--z-modal)" }}
      onMouseDown={closeOnBackdrop ? fechar : undefined}
    >
      <div
        ref={painelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={title ? tituloId : undefined}
        aria-describedby={description ? descId : undefined}
        tabIndex={-1}
        onMouseDown={(evento) => evento.stopPropagation()}
        className={`flex max-h-[90vh] w-full max-w-[92vw] flex-col rounded-2xl border border-white/10 bg-[color:var(--surface-1)] outline-none ${
          isConsole ? "p-7" : "p-6 shadow-2xl"
        } ${className}`}
        style={{ width: LARGURA[size] }}
      >
        {(title || description || botaoFechar) && (
          <header className={`flex shrink-0 items-start justify-between gap-4 ${title || description ? "mb-4" : ""}`}>
            <div className="min-w-0">
              {title && (
                <h2
                  id={tituloId}
                  className={
                    isConsole ? "text-[22px] font-semibold text-white" : "text-lg font-semibold text-white"
                  }
                >
                  {title}
                </h2>
              )}
              {description && (
                <p
                  id={descId}
                  className={`text-[13px] text-[color:var(--text-2)] ${title ? "mt-1" : ""}`}
                >
                  {description}
                </p>
              )}
            </div>
            {botaoFechar && (
              <button
                type="button"
                onClick={onClose}
                aria-label={t("common.fechar")}
                className="rounded-md p-1 text-white/40 transition-colors hover:bg-white/[0.06] hover:text-white"
              >
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                >
                  <line x1="18" y1="6" x2="6" y2="18" />
                  <line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            )}
          </header>
        )}

        <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>

        {footer && <div className="mt-5 shrink-0">{footer}</div>}
      </div>
    </div>,
    document.body,
  )
}
