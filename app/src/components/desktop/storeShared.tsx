"use client"

// Componentes compartilhados entre Loja e Início (catálogo estilo Hydra).

import { useEffect, useMemo, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"
import { useStoreActions } from "../useStoreActions"
import { StoreGamePage } from "./StoreGamePage"

export type ItemLoja = {
  appid: string
  title: string
  cover?: string
  capa?: string
  heroi?: string
  manifest?: boolean
}

export function StoreImg({
  appid,
  cover,
  capa,
  heroi,
  title,
}: {
  appid: string
  cover?: string
  capa?: string
  heroi?: string
  title: string
}) {
  const [fase, setFase] = useState(0)
  useEffect(() => setFase(0), [appid, cover, capa, heroi])
  const fontes = useMemo(
    () =>
      [
        cover || `https://cdn.akamai.steamstatic.com/steam/apps/${appid}/header.jpg`,
        heroi || "",
        `https://cdn.akamai.steamstatic.com/steam/apps/${appid}/capsule_231x87.jpg`,
        capa || "",
      ].filter(Boolean),
    [appid, cover, capa, heroi],
  )
  if (fase >= fontes.length) {
    return (
      <div className="flex h-full w-full flex-col items-center justify-center gap-1.5 bg-[#121216] px-3 text-center">
        <svg
          width="22"
          height="22"
          viewBox="0 0 24 24"
          fill="none"
          stroke="rgba(255,255,255,0.25)"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <rect x="3" y="3" width="18" height="18" rx="2" />
          <circle cx="8.5" cy="8.5" r="1.5" />
          <path d="m21 15-5-5L5 21" />
        </svg>
        <span className="text-[11px] leading-tight text-white/30">{title}</span>
      </div>
    )
  }
  const src = fontes[fase]
  const retrato = Boolean(capa && src === capa)
  return (
    <img
      src={src}
      alt=""
      loading="lazy"
      decoding="async"
      className={`h-full w-full ${retrato ? "object-contain" : "object-cover"}`}
      draggable={false}
      onError={() => setFase((f) => f + 1)}
    />
  )
}

export function CartaoLoja({
  jogo,
  naBiblioteca,
  adicionado,
  onOpen,
  t,
}: {
  jogo: ItemLoja
  naBiblioteca: boolean
  adicionado: boolean
  onOpen: () => void
  t: (k: string, v?: Record<string, string | number>) => string
}) {
  return (
    <div className="overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02]">
      <button
        onClick={onOpen}
        className="block aspect-[460/215] w-full cursor-pointer bg-black"
        title={jogo.title}
      >
        <StoreImg
          appid={jogo.appid}
          cover={jogo.cover}
          capa={jogo.capa}
          heroi={jogo.heroi}
          title={jogo.title}
        />
      </button>
      <div className="p-3">
        <div className="mb-2 truncate text-[13px] font-medium text-white" title={jogo.title}>
          {jogo.title}
        </div>
        {naBiblioteca || adicionado ? (
          <div
            className="flex items-center justify-center gap-1.5 rounded-lg border border-[color:var(--accent)]/35 py-2 text-[12px] font-semibold"
            style={{ color: "var(--accent)" }}
          >
            <svg
              width="13"
              height="13"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <polyline points="20 6 9 17 4 12" />
            </svg>
            {t("store.na_biblioteca")}
          </div>
        ) : (
          <p className="rounded-lg border border-white/10 py-2 text-center text-[12px] text-white/45">
            {t("store.ver_detalhes")}
          </p>
        )}
      </div>
    </div>
  )
}

// Hook leve para puxar gameSysinfo de um jogo (usado no hero).
export function useGameSysinfo(appid: string) {
  const [info, setInfo] = useState<
    | {
        short_description?: string
        about?: string
        release_date?: string
        publishers?: string[]
        developers?: string[]
        header?: string
        background?: string
      }
    | null
    | undefined
  >(undefined)
  useEffect(() => {
    let vivo = true
    setInfo(undefined)
    const g = { id: `steam:${appid}`, title: "", launcher: "steam", launch_cmd: [] as string[] }
    window.launcherAPI?.gameSysinfo(g as never).then((r) => {
      if (vivo) setInfo(r?.info || null)
    })
    return () => {
      vivo = false
    }
  }, [appid])
  return info
}

export type { Game }
export { useStoreActions, StoreGamePage, useI18n }
