"use client"

import type { Game } from "./types"
import { useI18n } from "../../i18n/I18nContext"

interface HeroSectionProps {
  game: Game | null
  /** Este jogo é o que está rodando agora — o botão vira "Parar". */
  rodando?: boolean
  /** Lançado, esperando o processo subir. */
  abrindo?: boolean
  onLaunch: () => void
  onMore: () => void
}

export function HeroSection({ game, rodando, abrindo, onLaunch, onMore }: HeroSectionProps) {
  const { t } = useI18n()
  return (
    <div className="anim-rise relative w-full flex-1 flex items-end pb-16">
      {/* Conteúdo do hero (o fundo é a imagem de tela cheia do app) */}
      {game && (
        <div key={game.id} className="w-full px-10 animate-fade-slide-up">
          <div className="anim-fade max-w-xl">
            {/* Logo do jogo, ou o título grande */}
            {game.logo ? (
              <img
                src={game.logo}
                alt={game.title}
                className="max-h-32 max-w-md object-contain mb-5"
                style={{ filter: "drop-shadow(0 2px 12px rgba(0,0,0,0.7))" }}
              />
            ) : (
              <h1
                className="text-6xl font-bold text-white leading-[1.05] text-balance mb-5"
                style={{ textShadow: "0 2px 16px rgba(0,0,0,0.6)" }}
              >
                {game.title}
              </h1>
            )}

            {/* Sinopse curta: duas linhas, como na referência */}
            {game.description && (
              <p
                className="text-[19px] text-white/90 leading-snug line-clamp-2 mb-8"
                style={{ textShadow: "0 2px 12px rgba(0,0,0,0.7)" }}
              >
                {game.description}
              </p>
            )}

            {/* Ações: pílula translúcida larga + "..." redondo */}
            <div className="flex items-center gap-4">
              {/* Rodando: o mesmo laranja do card "jogando" do desktop, para a
                  troca ser percebida sem precisar ler o rótulo. */}
              <button
                onClick={onLaunch}
                className="px-16 py-4 rounded-full text-[19px] font-semibold text-white transition-colors hover:bg-white/25"
                style={{
                  background: rodando ? "#e8703a" : "rgba(255,255,255,0.16)",
                  backdropFilter: "blur(10px)",
                  boxShadow: "0 6px 28px rgba(0,0,0,0.35)",
                }}
              >
                {rodando ? t("hero.parar") : abrindo ? t("hero.abrindo") : game.installed === false ? t("hero.instalar") : t("hero.jogar")}
              </button>
              <button
                onClick={onMore}
                aria-label={t("hero.mais_opcoes")}
                className="w-16 h-16 rounded-full flex items-center justify-center text-white transition-colors hover:bg-white/25"
                style={{
                  background: "rgba(255,255,255,0.16)",
                  backdropFilter: "blur(10px)",
                  boxShadow: "0 6px 28px rgba(0,0,0,0.35)",
                }}
              >
                <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
                  <circle cx="5" cy="12" r="2" />
                  <circle cx="12" cy="12" r="2" />
                  <circle cx="19" cy="12" r="2" />
                </svg>
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

export function LauncherIcon({ launcher, size = 14 }: { launcher: string; size?: number }) {
  if (launcher === "steam") {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor">
        <path d="M11.979 0C5.678 0 .511 4.86.022 11.037l6.432 2.658c.545-.371 1.203-.59 1.912-.59.063 0 .125.004.188.006l2.861-4.142V8.91c0-2.495 2.028-4.524 4.524-4.524 2.494 0 4.524 2.031 4.524 4.527s-2.03 4.525-4.524 4.525h-.105l-4.076 2.911c0 .052.004.105.004.159 0 1.875-1.515 3.396-3.39 3.396-1.635 0-3.016-1.173-3.331-2.727L.436 15.27C1.862 20.307 6.486 24 11.979 24c6.627 0 11.999-5.373 11.999-12S18.606 0 11.979 0zM7.54 18.21l-1.473-.61c.262.543.714.999 1.314 1.25 1.297.539 2.793-.076 3.332-1.375.263-.63.264-1.319.005-1.949s-.75-1.121-1.377-1.383c-.624-.26-1.29-.249-1.878-.03l1.523.63c.956.4 1.409 1.497 1.009 2.452-.397.957-1.497 1.41-2.455 1.015zm11.415-9.303c0-1.662-1.353-3.015-3.015-3.015-1.665 0-3.015 1.353-3.015 3.015 0 1.665 1.35 3.015 3.015 3.015 1.663 0 3.015-1.35 3.015-3.015zm-5.273-.005c0-1.252 1.013-2.266 2.265-2.266 1.249 0 2.266 1.014 2.266 2.266 0 1.251-1.017 2.265-2.266 2.265-1.253 0-2.265-1.014-2.265-2.265z" />
      </svg>
    )
  }
  if (launcher === "heroic") {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor">
        <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" stroke="currentColor" strokeWidth="2" fill="none" strokeLinecap="round" strokeLinejoin="round"/>
      </svg>
    )
  }
  // lutris
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor">
      <path d="M20 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm-8 3c1.93 0 3.5 1.57 3.5 3.5S13.93 14 12 14s-3.5-1.57-3.5-3.5S10.07 7 12 7zm7 13H5v-.23c0-.62.28-1.2.76-1.58C7.47 17.18 9.64 17 12 17s4.53.18 6.24 1.19c.48.38.76.97.76 1.58V20z"/>
    </svg>
  )
}
