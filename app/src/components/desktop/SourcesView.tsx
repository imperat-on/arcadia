"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import type { SourceGame, SourceInfo } from "../../global"
import { useI18n } from "../../i18n/I18nContext"

// Aba Fontes: catálogos JSON estilo Hydra, 100% locais (sem servidor).
// Fluxo: colar URL → fonte validada/cacheada → busca por título → baixar o
// magnet pelo subsistema torrent. O jogo aparece na seção Torrent da aba
// Downloads e, ao concluir, o usuário instala/registra (fluxo manual por
// enquanto — repacks têm setup.exe).
export function SourcesView() {
  const { t } = useI18n()
  const [fontes, setFontes] = useState<SourceInfo[]>([])
  const [url, setUrl] = useState("")
  const [adicionando, setAdicionando] = useState(false)
  const [sincronizando, setSincronizando] = useState(false)
  const [erro, setErro] = useState("")
  const [query, setQuery] = useState("")
  const [resultados, setResultados] = useState<SourceGame[]>([])
  const [buscando, setBuscando] = useState(false)
  const [baixando, setBaixando] = useState<string>("") // ref em download
  const debounce = useRef<ReturnType<typeof setTimeout> | null>(null)

  const carregar = useCallback(() => {
    window.launcherAPI?.sourcesList().then((r) => {
      if (r?.sources) setFontes(r.sources)
    })
  }, [])

  useEffect(carregar, [carregar])

  const adicionar = async () => {
    const u = url.trim()
    if (!u || adicionando) return
    setAdicionando(true)
    setErro("")
    const r = await window.launcherAPI?.sourcesAdd(u)
    setAdicionando(false)
    if (r?.ok) {
      setUrl("")
      carregar()
    } else {
      setErro(r?.error || t("fontes.erro_add"))
    }
  }

  const sincronizar = async () => {
    if (sincronizando) return
    setSincronizando(true)
    await window.launcherAPI?.sourcesSync()
    setSincronizando(false)
    carregar()
  }

  const remover = async (id: string) => {
    await window.launcherAPI?.sourcesRemove(id)
    carregar()
  }

  // Busca com debounce: o índice fica em RAM no main, resposta é instantânea,
  // mas não vale spammar IPC a cada tecla.
  useEffect(() => {
    if (debounce.current) clearTimeout(debounce.current)
    const q = query.trim()
    if (!q) {
      setResultados([])
      setBuscando(false)
      return
    }
    setBuscando(true)
    debounce.current = setTimeout(async () => {
      const r = await window.launcherAPI?.sourcesSearch(q, 40)
      setResultados(r?.results || [])
      setBuscando(false)
    }, 250)
    return () => {
      if (debounce.current) clearTimeout(debounce.current)
    }
  }, [query])

  const baixar = async (g: SourceGame) => {
    if (baixando) return
    setBaixando(g.ref)
    setErro("")
    const full = await window.launcherAPI?.sourcesGame(g.ref)
    const uris = full?.game?.uris || (full?.game?.uri ? [full.game.uri] : [])
    const uri =
      uris.find((u) => String(u).startsWith("magnet:")) ||
      uris.find((u) => /^https?:\/\//.test(String(u)))
    if (!uri) {
      setErro(t("fontes.sem_magnet"))
      setBaixando("")
      return
    }
    const r = await window.launcherAPI?.torrentStart({
      gameId: g.ref,
      url: String(uri),
      title: g.title,
    })
    if (!r?.ok) setErro(r?.error || t("fontes.erro_download"))
    setBaixando("")
  }

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <div className="mb-6 flex items-baseline justify-between">
        <h1 className="ui-title">{t("fontes.titulo")}</h1>
        {fontes.length > 0 && (
          <button
            onClick={sincronizar}
            disabled={sincronizando}
            className="rounded-lg border border-white/15 px-4 py-2 text-[12px] font-semibold text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white disabled:opacity-40"
          >
            {sincronizando ? t("fontes.sincronizando") : t("fontes.sincronizar")}
          </button>
        )}
      </div>

      <div className="max-w-[900px]">
        {/* Adicionar fonte */}
        <div className="flex gap-2">
          <input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && adicionar()}
            placeholder={t("fontes.placeholder_url")}
            className="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-4 py-2.5 text-[13px] text-white outline-none placeholder:text-white/30 focus:border-[color:var(--accent)]"
          />
          <button
            onClick={adicionar}
            disabled={adicionando || !url.trim()}
            className="rounded-lg px-5 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03] disabled:opacity-40"
            style={{ background: "var(--accent)" }}
          >
            {adicionando ? t("fontes.adicionando") : t("fontes.adicionar")}
          </button>
        </div>
        {erro && <p className="mt-2 text-[12px] text-[#ff6b81]">{erro}</p>}

        {/* Lista de fontes */}
        {fontes.length > 0 && (
          <div className="mt-5 flex flex-col gap-2">
            {fontes.map((f) => (
              <div
                key={f.id}
                className="flex items-center justify-between gap-4 rounded-xl border border-white/10 bg-white/[0.03] px-4 py-3"
              >
                <div className="min-w-0">
                  <div className="truncate text-[13px] font-medium text-white/90">
                    {f.name || f.url}
                    {typeof f.count === "number" && (
                      <span className="ml-2 text-white/40">
                        {t("fontes.jogos", { count: String(f.count) })}
                      </span>
                    )}
                  </div>
                  <div className="truncate text-[11px] text-white/35">{f.url}</div>
                </div>
                <button
                  onClick={() => remover(f.id)}
                  className="shrink-0 rounded-lg border border-white/15 px-3 py-1.5 text-[11px] font-semibold text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white"
                >
                  {t("common.remover")}
                </button>
              </div>
            ))}
          </div>
        )}

        {/* Busca no catálogo */}
        {fontes.length > 0 && (
          <>
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("fontes.placeholder_busca")}
              className="mt-8 w-full rounded-lg border border-white/10 bg-white/[0.04] px-4 py-2.5 text-[13px] text-white outline-none placeholder:text-white/30 focus:border-[color:var(--accent)]"
            />
            {buscando && <p className="mt-3 text-[12px] text-white/40">{t("fontes.buscando")}</p>}
            {!buscando && query.trim() && resultados.length === 0 && (
              <p className="mt-3 text-[12px] text-white/40">{t("fontes.sem_resultados")}</p>
            )}
            <div className="mt-4 flex flex-col gap-2 pb-8">
              {resultados.map((g) => (
                <div
                  key={g.ref}
                  className="flex items-center justify-between gap-4 rounded-xl border border-white/10 bg-white/[0.03] px-4 py-3"
                >
                  <div className="min-w-0">
                    <div className="truncate text-[13px] font-medium text-white/90">{g.title}</div>
                    <div className="text-[11px] text-white/35">
                      {g.fileSize}
                      {g.fileSize && g.src ? " · " : ""}
                      {g.src}
                    </div>
                  </div>
                  <button
                    onClick={() => baixar(g)}
                    disabled={Boolean(baixando)}
                    className="shrink-0 rounded-lg px-4 py-2 text-[11px] font-bold text-black transition-transform hover:scale-[1.03] disabled:opacity-40"
                    style={{ background: "var(--accent)" }}
                  >
                    {baixando === g.ref ? t("fontes.iniciando") : t("fontes.baixar")}
                  </button>
                </div>
              ))}
            </div>
          </>
        )}

        {fontes.length === 0 && <div className="ui-empty mt-8">{t("fontes.vazio")}</div>}
      </div>
    </div>
  )
}
