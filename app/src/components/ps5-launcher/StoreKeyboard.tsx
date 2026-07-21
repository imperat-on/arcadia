"use client"

import { useEffect, useRef, useState } from "react"
import { useGamepadNav } from "./useGamepadNav"
import { useI18n } from "../../i18n/I18nContext"

interface StoreKeyboardProps {
  aberto: boolean
  inicial?: string
  onConfirmar: (texto: string) => void
  onFechar: () => void
}

// Teclado na tela. No Big Picture não há teclado físico à mão, e o overlay da
// Steam só aparece dentro da Steam — sem isto a busca da loja seria inalcançável
// com o controle. As teclas são botões comuns, então o `useGamepadNav`, que faz
// navegação espacial pelo foco do DOM, move entre elas sem código extra.
const LINHAS = [
  ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
  ["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
  ["a", "s", "d", "f", "g", "h", "j", "k", "l", "-"],
  ["z", "x", "c", "v", "b", "n", "m", ":", "'", "."],
]

export function StoreKeyboard({ aberto, inicial = "", onConfirmar, onFechar }: StoreKeyboardProps) {
  const ref = useRef<HTMLDivElement>(null)
  const { t } = useI18n()
  const [texto, setTexto] = useState(inicial)
  useGamepadNav(ref, aberto, onFechar)

  useEffect(() => {
    if (aberto) setTexto(inicial)
  }, [aberto, inicial])

  // Teclado físico continua valendo: quem estiver no desktop ou com um teclado
  // conectado digita direto, sem passar pelas teclas da tela.
  useEffect(() => {
    if (!aberto) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") return onFechar()
      if (e.key === "Enter") return onConfirmar(texto.trim())
      if (e.key === "Backspace") return setTexto((t) => t.slice(0, -1))
      if (e.key.length === 1) setTexto((t) => t + e.key)
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [aberto, texto, onConfirmar, onFechar])

  if (!aberto) return null

  return (
    <div className="gp-scope fixed inset-0 z-[80] flex items-center justify-center bg-black/85 backdrop-blur-md">
      <div ref={ref} className="w-[720px] max-w-[92vw]">
        <div className="mb-5 rounded-xl border border-white/12 bg-white/[0.04] px-5 py-4">
          <div className="text-[11px] uppercase tracking-[0.2em] text-white/35">{t("store.buscar_titulo")}</div>
          <div className="mt-1 min-h-[32px] text-2xl font-light text-white">
            {texto || <span className="text-white/25">{t("store.keyboard.digite")}</span>}
            <span className="ml-0.5 animate-pulse text-[color:var(--accent)]">|</span>
          </div>
        </div>

        <div className="flex flex-col items-center gap-2">
          {LINHAS.map((linha, i) => (
            <div key={i} className="flex gap-2">
              {linha.map((t) => (
                <button
                  key={t}
                  onClick={() => setTexto((v) => v + t)}
                  className="h-12 w-12 rounded-lg border border-white/10 bg-white/[0.04] text-lg text-white/85 outline-none transition-colors hover:bg-white/[0.1] focus:bg-[color:var(--accent)] focus:text-black"
                >
                  {t}
                </button>
              ))}
            </div>
          ))}

          <div className="mt-2 flex gap-2">
            <button
              onClick={() => setTexto((t) => t + " ")}
              className="h-12 w-56 rounded-lg border border-white/10 bg-white/[0.04] text-sm text-white/75 outline-none transition-colors hover:bg-white/[0.1] focus:bg-[color:var(--accent)] focus:text-black"
            >
              {t("store.keyboard.espaco")}
            </button>
            <button
              onClick={() => setTexto((t) => t.slice(0, -1))}
              className="h-12 w-32 rounded-lg border border-white/10 bg-white/[0.04] text-sm text-white/75 outline-none transition-colors hover:bg-white/[0.1] focus:bg-[color:var(--accent)] focus:text-black"
            >
              {t("store.keyboard.apagar")}
            </button>
            <button
              onClick={() => onConfirmar(texto.trim())}
              className="h-12 w-32 rounded-lg text-sm font-bold text-black outline-none transition-transform hover:scale-[1.03] focus:ring-2 focus:ring-white"
              style={{ background: "var(--accent)" }}
            >
              {t("store.buscar")}
            </button>
          </div>
        </div>

        <p className="mt-5 text-center text-xs text-white/35">{t("store.keyboard.rodape")}</p>
      </div>
    </div>
  )
}
