"use client"

import { useEffect, useRef, useState } from "react"
import { useGamepadNav } from "./useGamepadNav"
import { useI18n } from "../../i18n/I18nContext"

export interface SugestaoLoja {
  appid: string
  title: string
  preco?: string
  img?: string
}

interface StoreKeyboardProps {
  aberto: boolean
  inicial?: string
  onConfirmar: (texto: string) => void
  onFechar: () => void
  // Sugestões espelhadas do autocomplete da Steam (extraídas pelo preload).
  sugestoes?: SugestaoLoja[]
  // Toda vez que o texto do teclado muda, o host propaga pra Steam via IPC
  // (arcadia:tecla) — é o que alimenta o autocomplete deles.
  onTexto?: (v: string) => void
  // Usuário escolheu um item da tira → abre a página do jogo direto.
  onEscolherSugestao?: (appid: string) => void
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

export function StoreKeyboard({
  aberto,
  inicial = "",
  onConfirmar,
  onFechar,
  sugestoes = [],
  onTexto,
  onEscolherSugestao,
}: StoreKeyboardProps) {
  const ref = useRef<HTMLDivElement>(null)
  const { t } = useI18n()
  const [texto, setTexto] = useState(inicial)
  useGamepadNav(ref, aberto, onFechar)

  useEffect(() => {
    if (aberto) setTexto(inicial)
  }, [aberto, inicial])

  // Propaga o texto pro host a cada mudança (o host repassa pra Steam via IPC).
  useEffect(() => {
    if (aberto) onTexto?.(texto)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [aberto, texto])

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
    <div className="gp-scope fixed inset-0 z-[80] flex flex-col bg-black/85 backdrop-blur-md">
      {/* Layout Big Picture: busca + sugestões ancorados NO TOPO (visíveis
          sem forçar o olhar pra baixo), teclado ancorado no RODAPÉ. O ref
          único envolve tudo pra o useGamepadNav navegar entre as duas zonas
          com D-pad (subir/descer entre teclas e sugestões). */}
      <div ref={ref} className="flex h-full w-full flex-col">
        <div className="mx-auto w-[1100px] max-w-[95vw] pt-10">
          <div className="rounded-xl border border-white/12 bg-white/[0.04] px-6 py-4">
            <div className="text-[11px] uppercase tracking-[0.2em] text-white/35">{t("store.buscar_titulo")}</div>
            <div className="mt-1 min-h-[36px] text-3xl font-light text-white">
              {texto || <span className="text-white/25">{t("store.keyboard.digite")}</span>}
              <span className="ml-0.5 animate-pulse text-[color:var(--accent)]">|</span>
            </div>
          </div>

          {/* Sugestões coladas na barra, empilhadas verticalmente (padrão Steam):
              cada resultado é uma linha largura-total com capa à esquerda e
              título/preço à direita. Ocupa menos espaço horizontal e caber
              4 resultados sem scroll. */}
          <div className="mt-3 rounded-2xl border border-white/10 bg-white/[0.03] p-3">
            {sugestoes.length > 0 ? (
              <div className="flex flex-col gap-2">
                {sugestoes.slice(0, 5).map((s) => (
                  <button
                    key={s.appid}
                    onClick={() => onEscolherSugestao?.(s.appid)}
                    className="group flex w-full items-center gap-4 rounded-lg border border-transparent bg-white/[0.03] p-2 text-left outline-none transition-all hover:bg-white/[0.08] focus:border-[color:var(--accent)] focus:bg-white/[0.08]"
                    title={s.title}
                  >
                    {s.img ? (
                      <img
                        src={s.img}
                        alt=""
                        className="h-[64px] w-[138px] shrink-0 rounded object-cover"
                        draggable={false}
                      />
                    ) : (
                      <div className="h-[64px] w-[138px] shrink-0 rounded bg-white/10" />
                    )}
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-base font-semibold text-white/95">{s.title}</div>
                      {s.preco && <div className="mt-1 truncate text-sm text-white/60">{s.preco}</div>}
                    </div>
                  </button>
                ))}
              </div>
            ) : (
              <div className="flex h-[108px] items-center justify-center text-sm text-white/40">
                {texto ? t("store.keyboard.buscando") : t("store.keyboard.digite_pra_buscar")}
              </div>
            )}
          </div>
        </div>

        {/* Teclado ancorado no rodapé, com centralização horizontal. */}
        <div className="mt-auto pb-10">
          <div className="mx-auto flex w-[960px] max-w-[95vw] flex-col items-center gap-2">
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

            <p className="mt-4 text-center text-xs text-white/35">{t("store.keyboard.rodape")}</p>
          </div>
        </div>
      </div>
    </div>
  )
}
