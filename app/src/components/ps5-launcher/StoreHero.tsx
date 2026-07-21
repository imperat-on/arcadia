"use client"

import { useEffect, useRef, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { ImgCascata } from "./ImgCascata"
import { urlsHeroi, urlLogo } from "./capaJogo"
import { semHtml } from "./texto"
import type { FichaJogo, JogoLinha } from "./types"

interface StoreHeroProps {
  /** Os destaques que o herói percorre. */
  jogos: JogoLinha[]
  /** Ficha do destaque atual — o pai busca conforme o índice muda. */
  ficha: FichaJogo | null
  trailer: { url: string } | null
  ativo: boolean
  naBiblioteca: boolean
  ocupado: boolean
  indice: number
  onIndice: (i: number) => void
  onAbrir: (j: JogoLinha) => void
  onBaixar: (j: JogoLinha) => void
  onAdicionar: (j: JogoLinha) => void
  /** Foco entrou no herói — a cor ambiente passa a seguir o destaque. */
  onFocar?: (j: JogoLinha) => void
}

// Troca sozinho a cada 9s. Menos que isso vira slideshow ansioso; mais, e a
// rotação não é percebida antes de a pessoa rolar a página.
const INTERVALO = 9000

// O herói da vitrine: arte widescreen, chamada por cima e os botões de ação.
// Percorre alguns destaques em rodízio, com os pontos indicando a posição.
//
// A rotação PARA quando algo aqui dentro tem o foco: trocar o jogo debaixo de
// um botão que a pessoa está prestes a apertar faria a ação cair no jogo
// errado. É o mesmo cuidado que impede o layout de se mexer sob a navegação.
export function StoreHero({
  jogos,
  ficha,
  trailer,
  ativo,
  naBiblioteca,
  ocupado,
  indice,
  onIndice,
  onAbrir,
  onBaixar,
  onAdicionar,
  onFocar,
}: StoreHeroProps) {
  const { t } = useI18n()
  const [pausado, setPausado] = useState(false)
  // Nem todo appid tem logo.png publicado — lançamento recente costuma não ter.
  // Quando falha, o título em texto assume. Guardamos POR APPID: com um único
  // booleano, o primeiro destaque sem logo esconderia o logo dos quatro
  // seguintes, já que o rodízio reusa o mesmo componente.
  const [semLogo, setSemLogo] = useState<Set<string>>(new Set())
  const raiz = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (pausado || jogos.length < 2) return
    const t = setInterval(() => onIndice((indice + 1) % jogos.length), INTERVALO)
    return () => clearInterval(t)
  }, [pausado, indice, jogos.length, onIndice])

  const jogo = jogos[indice]
  if (!jogo) return null

  const semManifesto = jogo.manifest === false
  const mostrarLogo = !semLogo.has(jogo.appid)

  return (
    <div
      ref={raiz}
      className="loja-heroi"
      onFocusCapture={() => {
        setPausado(true)
        // Sem isto, subir do primeiro trilho até o herói deixava a tela com a
        // cor do card de onde se veio.
        onFocar?.(jogo)
      }}
      onBlurCapture={(e) => {
        if (!raiz.current?.contains(e.relatedTarget as Node)) setPausado(false)
      }}
      onMouseEnter={() => setPausado(true)}
      onMouseLeave={() => setPausado(false)}
    >
      <div key={jogo.appid} className="loja-heroi__arte">
        <ImgCascata fontes={urlsHeroi(jogo, ficha?.fundo)} loading="eager" />
        {/* Sem `poster`: a miniatura do trailer tem 600px e, esticada sobre a
            faixa, cobria a arte de 3840px com um borrão. O vídeo só aparece
            quando tem quadro para mostrar — até lá, a arte fica visível. Isso
            também protege o caso de um MP4 quebrado escapar da verificação do
            backend. */}
        {trailer && ativo && (
          <video
            key={trailer.url}
            src={trailer.url}
            autoPlay
            loop
            muted
            playsInline
            preload="none"
            onCanPlay={(e) => e.currentTarget.classList.add("-tocando")}
            className="loja-trailer absolute inset-0 h-full w-full object-cover"
          />
        )}
      </div>

      {/* Degradês: um lateral para o texto ter contraste sobre qualquer arte,
          outro no rodapé para o herói se dissolver no primeiro trilho em vez
          de terminar num corte reto. */}
      <div className="absolute inset-0 bg-gradient-to-r from-[var(--loja-fundo)] via-[var(--loja-fundo)]/55 to-transparent" />
      <div className="absolute inset-x-0 bottom-0 h-2/3 bg-gradient-to-t from-[var(--loja-fundo)] via-[var(--loja-fundo)]/55 to-transparent" />

      <div className="relative flex h-full flex-col justify-end px-12 pb-10">
        <p className="text-[11px] font-semibold uppercase tracking-[0.2em] text-[var(--loja-apagado)]">
          {t("store.em_destaque")}
        </p>
        {/* Logo oficial quando existe; o h1 continua no DOM para leitores de
            tela, apenas visualmente oculto. */}
        {mostrarLogo && (
          <img
            key={`logo-${jogo.appid}`}
            src={urlLogo(jogo)}
            alt=""
            draggable={false}
            onError={() => setSemLogo((s) => new Set(s).add(jogo.appid))}
            className="mt-4 max-h-[132px] max-w-[46%] object-contain object-left drop-shadow-[0_4px_24px_rgba(0,0,0,0.85)]"
          />
        )}
        <h1
          className={
            mostrarLogo ? "sr-only" : "mt-3 max-w-2xl text-5xl font-bold leading-[1.05] tracking-tight"
          }
        >
          {jogo.title}
        </h1>

        {ficha?.descricao && (
          <p className="mt-4 max-w-xl text-[14px] leading-relaxed text-[var(--loja-apagado)] line-clamp-2">
            {semHtml(ficha.descricao)}
          </p>
        )}

        <div className="mt-6 flex flex-wrap items-center gap-3">
          <button
            data-appid={jogo.appid}
            onClick={() => (semManifesto ? onAbrir(jogo) : onBaixar(jogo))}
            disabled={ocupado}
            className="loja-botao -primario"
          >
            {semManifesto ? t("store.ver_detalhes") : t("store.baixar")}
          </button>
          <button
            onClick={() => onAdicionar(jogo)}
            disabled={ocupado || naBiblioteca || semManifesto}
            className="loja-botao"
          >
            {naBiblioteca ? t("store.na_biblioteca") : t("store.adicionar")}
          </button>
          {ficha?.preco && (
            <span className="text-[15px] font-semibold" style={{ color: "var(--loja-cor)" }}>
              {ficha.preco}
            </span>
          )}
          {jogo.desconto ? (
            <span
              className="rounded px-2 py-1 text-[11px] font-bold text-black"
              style={{ background: "var(--loja-cor)" }}
            >
              -{jogo.desconto}%
            </span>
          ) : null}
        </div>

        {jogos.length > 1 && (
          <div className="mt-8 flex items-center gap-2">
            {jogos.map((j, i) => (
              <button
                key={j.appid}
                tabIndex={-1}
                aria-label={t("store.destaque_numero", { n: i + 1 })}
                onClick={() => onIndice(i)}
                className="h-1 rounded-full transition-all"
                style={{
                  width: i === indice ? 32 : 16,
                  background: i === indice ? "#fff" : "rgba(255,255,255,0.3)",
                }}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
