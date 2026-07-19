"use client"

import { forwardRef, useMemo, useState } from "react"
import type { NewsItem } from "../../global"

interface NewsViewProps {
  news: NewsItem[]
  rotacao?: number // slot de 5 min do relógio — gira o destaque
  loading: boolean
  onOpen: (url: string) => void
}

// "há 2 h", "há 3 d", etc. a partir da data ISO.
function tempoRelativo(iso: string): string {
  if (!iso) return ""
  const diff = Date.now() - new Date(iso).getTime()
  if (isNaN(diff)) return ""
  const min = Math.floor(diff / 60000)
  if (min < 1) return "agora"
  if (min < 60) return `há ${min} min`
  const h = Math.floor(min / 60)
  if (h < 24) return `há ${h} h`
  const d = Math.floor(h / 24)
  return d === 1 ? "há 1 dia" : `há ${d} dias`
}

// Fundo de fallback quando a notícia não tem imagem.
function Fallback({ source }: { source: string }) {
  return (
    <div className="flex h-full w-full items-center justify-center bg-[#050505]">
      <span className="text-sm font-semibold uppercase tracking-[0.2em] text-white/30">{source}</span>
    </div>
  )
}

function Imagem({ src, source, alt }: { src: string; source: string; alt: string }) {
  const [erro, setErro] = useState(false)
  if (!src || erro) return <Fallback source={source} />
  return (
    <img
      src={src}
      alt={alt}
      onError={() => setErro(true)}
      className="h-full w-full object-cover transition-transform duration-700 group-hover:scale-[1.03]"
      draggable={false}
    />
  )
}

export const NewsView = forwardRef<HTMLDivElement, NewsViewProps>(function NewsView(
  { news, rotacao = 0, loading, onOpen },
  ref,
) {
  // Destaque ROTATIVO a cada marco de 5 min do relógio: gira entre as 5
  // manchetes mais novas; o mosaico e a grade mostram as demais, em ordem.
  const ordenadas = useMemo(() => {
    if (!news.length) return news
    const k = rotacao % Math.min(5, news.length)
    return [...news.slice(k), ...news.slice(0, k)]
  }, [news, rotacao])
  const active = ordenadas[0]
  // Só as principais: destaque + 4 no mosaico + 8 na grade vertical (13 no máx).
  const mosaic = ordenadas.slice(1, 5)
  const vertical = ordenadas.slice(5, 13)

  return (
    <div ref={ref} className="h-full w-full overflow-y-auto overflow-x-hidden bg-black text-white antialiased">
      <div className="mx-auto max-w-[1400px] px-10 py-6">
        {/* Cabeçalho */}
        <div className="mb-6 flex items-center gap-2 text-[11px] font-medium uppercase tracking-[0.24em] text-white/50">
          <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--accent)" }} />
          Notícias
        </div>

        {loading && !active ? (
          <div className="flex flex-col gap-6">
            <div className="aspect-[21/10] w-full animate-pulse rounded-2xl border border-white/[0.06] bg-white/[0.03]" />
            {Array.from({ length: 4 }).map((_, i) => (
              <div key={i} className="h-44 w-full animate-pulse rounded-2xl border border-white/[0.06] bg-white/[0.03]" />
            ))}
          </div>
        ) : !active ? (
          <div className="flex min-h-[400px] items-center justify-center text-white/40">
            Sem notícias no momento.
          </div>
        ) : (
          <div className="flex flex-col gap-6">
            {/* Destaque — imagem grande com texto sobreposto no canto inferior */}
            <button
              onClick={() => onOpen(active.url)}
              className="group relative block w-full overflow-hidden rounded-2xl border border-white/[0.08] text-left outline-none transition-all duration-300 hover:border-white/25 focus-visible:border-[color:var(--accent)] focus-visible:shadow-[0_0_0_2px_var(--accent)]"
            >
              <div className="relative aspect-[21/10] w-full overflow-hidden">
                <Imagem src={active.image} source={active.source} alt={active.title} />
                <div className="absolute inset-0 bg-gradient-to-t from-black via-black/30 to-transparent" />
              </div>
              <div className="absolute inset-x-0 bottom-0 flex flex-col gap-2 p-8 md:p-12">
                <h2 className="max-w-3xl text-3xl font-medium leading-tight tracking-tight md:text-4xl">{active.title}</h2>
                <span className="text-sm text-white/50">{tempoRelativo(active.date)}</span>
                <p className="mt-1 line-clamp-2 max-w-2xl text-base leading-relaxed text-white/75">{active.summary}</p>
              </div>
            </button>

            {/* Mosaico 2 colunas — alterna cards com imagem e cards só de texto */}
            {mosaic.length > 0 && (
              <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
                {mosaic.map((item, i) => {
                  const comImagem = !!item.image && i % 2 === 0
                  return comImagem ? (
                    <button
                      key={item.id}
                      onClick={() => onOpen(item.url)}
                      className="group relative block min-h-[320px] w-full overflow-hidden rounded-2xl border border-white/[0.08] text-left outline-none transition-all duration-300 hover:border-white/25 focus-visible:border-[color:var(--accent)] focus-visible:shadow-[0_0_0_2px_var(--accent)]"
                    >
                      <div className="absolute inset-0">
                        <Imagem src={item.image} source={item.source} alt={item.title} />
                        <div className="absolute inset-0 bg-gradient-to-t from-black via-black/30 to-transparent" />
                      </div>
                      <div className="absolute inset-x-0 bottom-0 flex flex-col gap-1.5 p-7">
                        <h3 className="line-clamp-2 text-xl font-medium leading-snug tracking-tight md:text-2xl">{item.title}</h3>
                        <span className="text-sm text-white/50">{tempoRelativo(item.date)}</span>
                        <p className="mt-1 line-clamp-3 max-w-xl text-sm leading-relaxed text-white/75">{item.summary}</p>
                      </div>
                    </button>
                  ) : (
                    <button
                      key={item.id}
                      onClick={() => onOpen(item.url)}
                      className="group flex min-h-[320px] w-full flex-col gap-1.5 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-7 text-left outline-none transition-all duration-300 hover:border-white/25 focus-visible:border-[color:var(--accent)] focus-visible:shadow-[0_0_0_2px_var(--accent)] md:p-9"
                    >
                      <h3 className="line-clamp-2 text-xl font-medium leading-snug tracking-tight md:text-2xl">{item.title}</h3>
                      <span className="text-sm text-white/50">— {tempoRelativo(item.date)}</span>
                      <p className="mt-4 line-clamp-[6] max-w-xl text-base leading-relaxed text-white/70">{item.summary}</p>
                    </button>
                  )
                })}
              </div>
            )}

            {/* Grade vertical — cards retrato com imagem no topo e texto embaixo */}
            {vertical.length > 0 && (
              <div className="grid grid-cols-1 gap-6 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
                {vertical.map((item) => (
                  <button
                    key={item.id}
                    onClick={() => onOpen(item.url)}
                    className="group flex w-full flex-col overflow-hidden rounded-2xl border border-white/[0.08] text-left outline-none transition-all duration-300 hover:border-white/25 focus-visible:border-[color:var(--accent)] focus-visible:shadow-[0_0_0_2px_var(--accent)]"
                  >
                    <div className="relative aspect-[4/3] w-full shrink-0 overflow-hidden">
                      <Imagem src={item.image} source={item.source} alt={item.title} />
                      <div className="absolute inset-0 bg-gradient-to-t from-black via-black/20 to-transparent" />
                      <div className="absolute inset-x-0 bottom-0 flex flex-col gap-1 p-5">
                        <h3 className="line-clamp-2 text-lg font-medium leading-snug tracking-tight">{item.title}</h3>
                        <span className="text-xs text-white/50">{tempoRelativo(item.date)}</span>
                      </div>
                    </div>
                    <div className="flex flex-1 flex-col p-5 pt-4">
                      <p className="line-clamp-4 text-sm leading-relaxed text-white/70">{item.summary}</p>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
})
