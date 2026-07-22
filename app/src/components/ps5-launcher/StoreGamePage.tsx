"use client"

import { useEffect, useRef, useState } from "react"
import { useGamepadNav } from "./useGamepadNav"
import type { JogoLinha } from "./types"
import { useI18n } from "../../i18n/I18nContext"

interface StoreGamePageProps {
  jogo: JogoLinha | null
  bloqueado: boolean
  ocupado: boolean
  onBaixar: () => void
  onAdicionar: () => void
  onRemover: () => void
  onFechar: () => void
}

// i18n do Arcadia → parâmetro `l` da loja Steam. A Steam aceita os nomes em
// inglês; fora do mapa, omite (serve no idioma dos cookies/Accept-Language).
const STEAM_LANG: Record<string, string> = {
  "pt-BR": "brazilian",
  "en-US": "english",
  "es-ES": "spanish",
}

export function StoreGamePage({
  jogo,
  bloqueado,
  ocupado,
  onBaixar,
  onAdicionar,
  onRemover,
  onFechar,
}: StoreGamePageProps) {
  const { t, lang } = useI18n()
  const ref = useRef<HTMLDivElement>(null)
  const webRef = useRef<any>(null)
  const prontoRef = useRef(false)
  const aberto = Boolean(jogo)
  useGamepadNav(ref, aberto, onFechar)

  const [carregando, setCarregando] = useState(true)
  const [falhou, setFalhou] = useState(false)

  const semManifesto = jogo?.manifest === false

  // Monta o contexto que a barra injetada (webview-steam-preload.js) usa.
  function contexto() {
    // A página Steam é isolada e não enxerga o --accent do Arcadia; leio aqui
    // no host e mando junto para o preload aplicar na barra e no botão.
    const accent = getComputedStyle(document.documentElement)
      .getPropertyValue("--accent").trim() || "#57b6ff"
    return {
      appid: jogo?.appid || "",
      fontes: jogo?.fontes || [],
      bloqueado,
      semManifesto,
      ocupado,
      accent,
      labels: {
        baixar: t("store.baixar"),
        adicionar: t("store.adicionar_steam"),
        remover: t("common.remover"),
        voltar: t("common.voltar"),
        naBiblioteca: t("store.na_biblioteca"),
        semManifesto: t("store.sem_manifesto"),
        disponivelEm: t("store.manifesto_disponivel_em"),
      },
    }
  }

  // Liga os eventos da webview (uma vez por jogo aberto).
  useEffect(() => {
    if (!jogo) return
    const el = webRef.current
    if (!el) return
    prontoRef.current = false
    setCarregando(true)
    setFalhou(false)

    const onStart = () => setCarregando(true)
    const onStop = () => setCarregando(false)
    const onReady = () => {
      prontoRef.current = true
      try { el.send("arcadia:contexto", contexto()) } catch {}
    }
    const onFail = (e: any) => {
      // errorCode -3 = navegação abortada (normal em redirects); ignora.
      if (e?.errorCode && e.errorCode !== -3) setFalhou(true)
    }
    const onMsg = (e: any) => {
      if (e?.channel !== "arcadia:acao") return
      const tipo = e.args?.[0]?.tipo
      if (tipo === "baixar") onBaixar()
      else if (tipo === "adicionar") onAdicionar()
      else if (tipo === "remover") onRemover()
      else if (tipo === "voltar") onFechar()
    }

    el.addEventListener("did-start-loading", onStart)
    el.addEventListener("did-stop-loading", onStop)
    el.addEventListener("dom-ready", onReady)
    el.addEventListener("did-fail-load", onFail)
    el.addEventListener("ipc-message", onMsg)
    return () => {
      el.removeEventListener("did-start-loading", onStart)
      el.removeEventListener("did-stop-loading", onStop)
      el.removeEventListener("dom-ready", onReady)
      el.removeEventListener("did-fail-load", onFail)
      el.removeEventListener("ipc-message", onMsg)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jogo?.appid])

  // Reenvia o contexto quando o estado muda (ex.: baixou → vira bloqueado).
  useEffect(() => {
    if (!prontoRef.current) return
    try { webRef.current?.send("arcadia:contexto", contexto()) } catch {}
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bloqueado, ocupado, semManifesto, jogo?.fontes])

  if (!jogo) return null

  const l = STEAM_LANG[lang]
  const url =
    `https://store.steampowered.com/app/${encodeURIComponent(jogo.appid)}/` +
    `?cc=br${l ? `&l=${l}` : ""}`

  return (
    <div ref={ref} className="fixed inset-0 z-[70] flex flex-col bg-[#08090b] text-white">
      {/* Barra mínima do host: garante saída sempre (o gamepad não navega dentro
          da página; a barra rica fica injetada na própria página Steam). */}
      <div className="flex items-center gap-3 border-b border-white/10 bg-black/60 px-4 py-2">
        <span className="text-sm font-semibold text-white/70">{jogo.title}</span>
        {carregando && <span className="text-xs text-white/40">{t("store.carregando")}</span>}
        <button
          onClick={onFechar}
          className="ml-auto rounded-lg border border-white/15 px-4 py-1.5 text-sm font-semibold text-white/80 outline-none hover:bg-white/[0.08]"
        >
          {t("common.voltar")}
        </button>
      </div>

      <div className="relative flex-1">
        {falhou ? (
          <div className="flex h-full flex-col items-center justify-center gap-4 text-white/50">
            <p>{t("store.ficha_indisponivel", { erro: "" })}</p>
            <button
              onClick={() => window.launcherAPI?.openExternal(url)}
              className="rounded-lg border border-white/15 px-4 py-2 text-sm text-white/80 hover:bg-white/[0.08]"
            >
              {t("store.abrir_navegador")}
            </button>
          </div>
        ) : (
          <webview
            ref={webRef}
            src={url}
            partition="persist:steamstore"
            allowpopups={true}
            useragent="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
            style={{ width: "100%", height: "100%", background: "#000" }}
          />
        )}
      </div>
    </div>
  )
}
