"use client"

import { useRef } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { CapaLoja } from "./CapaLoja"
import { rolagemSuave } from "./rolagem"
import type { JogoLinha } from "./types"

interface StoreTileProps {
  jogo: JogoLinha
  /** Atraso da animação de entrada, em ordem de aparição. */
  indice?: number
  /** Largura do ladrilho. A altura vem da proporção 3:4 mais o bloco de meta. */
  largura?: number
  /** Já está na biblioteca — some o "+" e o card ganha o selo. */
  naBiblioteca?: boolean
  onFocar: (j: JogoLinha) => void
  onAbrir: (j: JogoLinha) => void
  onAdicionar?: (j: JogoLinha) => void
}

// Capa retrato com bloco de meta abaixo: título numa linha e preço na outra,
// com o valor cheio riscado quando há desconto. O selo de desconto fica sobre a
// arte, no canto superior esquerdo, e o "+" de adicionar no direito.
//
// Ao receber foco o ladrilho se centraliza no scroller do trilho: o
// useGamepadNav escolhe o vizinho pela geometria e alcança cards fora da área
// visível, então sem isto o foco sumiria da tela.
export function StoreTile({
  jogo,
  indice = 0,
  largura = 200,
  naBiblioteca,
  onFocar,
  onAbrir,
  onAdicionar,
}: StoreTileProps) {
  const { t } = useI18n()
  const ref = useRef<HTMLButtonElement | null>(null)
  const semManifesto = jogo.manifest === false

  return (
    <button
      ref={ref}
      onClick={() => onAbrir(jogo)}
      onFocus={() => {
        onFocar(jogo)
        ref.current?.scrollIntoView({ block: "nearest", inline: "center", behavior: rolagemSuave() })
      }}
      data-appid={jogo.appid}
      aria-label={jogo.title}
      className="loja-tile shrink-0 text-left outline-none"
      style={{ width: largura, animationDelay: `${Math.min(indice * 20, 300)}ms` }}
    >
      <div
        className="loja-tile__capa relative overflow-hidden rounded-[10px] bg-[var(--loja-sup-2)]"
        style={{ aspectRatio: "3 / 4" }}
      >
        <CapaLoja jogo={jogo} />

        {jogo.desconto ? (
          <span
            className="absolute left-2 top-2 rounded px-1.5 py-0.5 text-[11px] font-bold text-black"
            style={{ background: "var(--loja-cor)" }}
          >
            -{jogo.desconto}%
          </span>
        ) : null}

        {/* Atalho de mouse para adicionar. No controle isso é o Y, então o
            botão fica fora da navegação: focável, ele viraria uma parada extra
            entre uma capa e a seguinte. */}
        {onAdicionar && !naBiblioteca && !semManifesto && (
          <span
            role="button"
            tabIndex={-1}
            aria-label={t("store.adicionar")}
            onClick={(e) => {
              e.stopPropagation()
              onAdicionar(jogo)
            }}
            className="loja-tile__acao absolute right-2 top-2 grid h-8 w-8 place-items-center rounded-full bg-black/60 text-white backdrop-blur-md hover:bg-black/80"
          >
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2">
              <path d="M12 5v14M5 12h14" />
            </svg>
          </span>
        )}

        {naBiblioteca && (
          <span className="absolute bottom-2 left-2 rounded bg-black/70 px-1.5 py-0.5 text-[10px] font-semibold text-white backdrop-blur-md">
            {t("store.na_biblioteca")}
          </span>
        )}
        {semManifesto && !naBiblioteca && (
          <span className="absolute bottom-2 left-2 rounded bg-black/70 px-1.5 py-0.5 text-[10px] font-semibold text-white/60 backdrop-blur-md">
            {t("store.sem_manifesto")}
          </span>
        )}
      </div>

      <div className="mt-3">
        <h3 className="truncate text-[14px] font-semibold text-white">{jogo.title}</h3>
        {/* Altura reservada: num trilho misto (com e sem preço), sem ela as
            capas ficariam desalinhadas na vertical. */}
        <div className="mt-1 flex h-[20px] items-baseline gap-2">
          {jogo.preco ? (
            <>
              <span className="text-[14px] font-semibold text-white">{jogo.preco}</span>
              {jogo.precoOriginal ? (
                <span className="text-[12px] text-[var(--loja-apagado)] line-through">{jogo.precoOriginal}</span>
              ) : null}
            </>
          ) : null}
        </div>
      </div>
    </button>
  )
}
