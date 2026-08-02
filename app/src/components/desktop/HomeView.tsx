"use client"

import { useEffect, useMemo, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import { CartaoLoja, StoreGamePage, useGameSysinfo, useI18n, useStoreActions, type ItemLoja } from "./storeShared"
import { MetodoDownloadDialog } from "./MetodoDownloadDialog"
import { EscolhaDownloadDialog } from "../DepotPicker"

const CATEGORIAS = [
  { id: "top100in2weeks", labelKey: "store.mais_baixados_semana" },
] as const

export function HomeView({ games = [] }: { games?: Game[] }) {
  const { t } = useI18n()
  const {
    bloqueados,
    jaAdicionados,
    escolhendo,
    setEscolhendo,
    metodo,
    setMetodo,
    busy: acaoBusy,
    toast,
    setToast,
    baixar,
    baixarDepot,
    confirmarTorrent,
    confirmarBaixar,
    adicionar,
    slsAtivo,
  } = useStoreActions(games)
  const [categoria, setCategoria] = useState<(typeof CATEGORIAS)[number]["id"]>("top100in2weeks")
  const [recentes, setRecentes] = useState<ItemLoja[]>([])
  const [carregando, setCarregando] = useState(true)
  const [pagina, setPagina] = useState<ItemLoja | null>(null)
  const [heroIdx, setHeroIdx] = useState(0)
  const esqueletos = useMemo(() => Array.from({ length: 8 }, (_, i) => i), [])

  useEffect(() => {
    let vivo = true
    setCarregando(true)
    window.launcherAPI?.storeRecent(categoria).then((r) => {
      if (vivo && r?.ok) setRecentes(r.jogos || [])
    }).finally(() => {
      if (vivo) setCarregando(false)
    })
    return () => { vivo = false }
  }, [categoria])

  useEffect(() => {
    if (recentes.length < 2) return
    const timer = setInterval(() => setHeroIdx((i) => (i + 1) % Math.min(5, recentes.length)), 6000)
    return () => clearInterval(timer)
  }, [recentes.length])

  const hero = recentes[heroIdx % Math.max(1, recentes.length)]

  const abrirAleatorio = () => {
    if (!recentes.length) return
    setPagina(recentes[Math.floor(Math.random() * recentes.length)])
  }

  return (
    <div className="h-full overflow-y-auto bg-[#08080a]">
      {hero ? <HomeHero jogo={hero} onOpen={() => setPagina(hero)} /> : <div className="h-[42vh] min-h-[300px] animate-pulse bg-white/[0.04]" />}

      <div className="px-8 py-6">
        <div className="mb-6 flex items-center justify-between">
          <div className="flex gap-2">
            {CATEGORIAS.map((c) => (
              <button
                key={c.id}
                onClick={() => setCategoria(c.id)}
                className={`rounded-lg border px-4 py-2 text-[13px] font-medium transition-colors ${
                  c.id === categoria ? "border-white/80 bg-white text-black" : "border-white/10 bg-white/[0.02] text-white/70 hover:text-white"
                }`}
              >
                {t(c.labelKey)}
              </button>
            ))}
          </div>
          <button
            onClick={abrirAleatorio}
            disabled={!recentes.length}
            className="ui-btn-secondary flex items-center gap-1.5 rounded-lg px-4 py-2 text-[13px] disabled:opacity-40"
          >
            {t("store.surpreenda_me")}
          </button>
        </div>

        <h2 className="mb-4 text-xl font-semibold text-white">
          {t(CATEGORIAS.find((c) => c.id === categoria)?.labelKey || "store.mais_baixados_semana")}
        </h2>

        <div className="grid-stagger grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-4 pb-10">
          {carregando && esqueletos.map((i) => (
            <div key={`sk${i}`} className="overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02]">
              <div className="aspect-[460/215] w-full animate-pulse bg-white/[0.05]" />
              <div className="p-3"><div className="mb-2 h-3.5 w-3/4 animate-pulse rounded bg-white/[0.07]" /><div className="h-8 animate-pulse rounded-lg bg-white/[0.04]" /></div>
            </div>
          ))}
          {!carregando && recentes.map((j) => (
            <CartaoLoja
              key={j.appid}
              jogo={j}
              naBiblioteca={bloqueados.has(j.appid)}
              adicionado={jaAdicionados.has(j.appid)}
              onOpen={() => setPagina(j)}
              t={t}
            />
          ))}
        </div>
      </div>

      {pagina && (
        <StoreGamePage
          jogo={pagina}
          onClose={() => setPagina(null)}
          onBaixar={() => baixar(pagina)}
          onAdicionar={() => { adicionar(pagina); setPagina(null) }}
          naBiblioteca={bloqueados.has(pagina.appid)}
          ocupado={acaoBusy !== ""}
        />
      )}

      {metodo && (
        <MetodoDownloadDialog
          jogo={metodo.jogo}
          opcoes={metodo.opcoes}
          onDepot={() => { const j = metodo.jogo; setMetodo(null); baixarDepot(j) }}
          onTorrent={(magnet, pasta) => confirmarTorrent(metodo.jogo, magnet, pasta)}
          onClose={() => setMetodo(null)}
          depotDisponivel={slsAtivo}
        />
      )}

      {escolhendo && (
        <EscolhaDownloadDialog
          escolhendo={escolhendo}
          onCancel={() => setEscolhendo(null)}
          onConfirm={(steamDir, sel) => confirmarBaixar(escolhendo.jogo, escolhendo.info, steamDir, sel)}
          titulo={t("store.instalar_em", { title: escolhendo.jogo.title })}
        />
      )}

      {toast && (
        <div className="fixed bottom-5 right-5 z-[80] max-w-[360px] rounded-xl border border-white/15 bg-[#0d1017]/95 px-4 py-3 text-[13px] text-white/90 shadow-2xl shadow-black/60 backdrop-blur-md" onClick={() => setToast("")}>{toast}</div>
      )}
    </div>
  )
}

function HomeHero({ jogo, onOpen }: { jogo: ItemLoja; onOpen: () => void }) {
  const info = useGameSysinfo(jogo.appid)
  const desc = info?.short_description || ""
  const hero = `https://cdn.cloudflare.steamstatic.com/steam/apps/${jogo.appid}/library_hero.jpg`
  const header = jogo.cover || `https://cdn.cloudflare.steamstatic.com/steam/apps/${jogo.appid}/header.jpg`
  return (
    <button onClick={onOpen} className="relative block h-[44vh] min-h-[330px] w-full overflow-hidden bg-black text-left">
      <img src={hero} alt="" className="h-full w-full object-cover opacity-90" draggable={false} onError={(e) => { (e.currentTarget as HTMLImageElement).src = header }} />
      <div className="absolute inset-0 bg-gradient-to-r from-black/85 via-black/25 to-black/35" />
      <div className="absolute inset-0 bg-gradient-to-t from-[#08080a] via-transparent to-transparent" />
      <div className="absolute bottom-10 left-8 max-w-[720px]">
        <h1 className="mb-5 text-4xl font-semibold tracking-wide text-white drop-shadow-lg">{jogo.title}</h1>
        {desc && <p className="max-w-[680px] text-[14px] leading-relaxed text-white/80 drop-shadow-lg">{desc}</p>}
      </div>
    </button>
  )
}

