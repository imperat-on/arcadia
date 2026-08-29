"use client"

import type { Game } from "./types"
import { useI18n } from "../../i18n/I18nContext"

interface HeroSectionProps {
  game: Game | null
  trailerUrl?: string | null
  rodando?: boolean
  abrindo?: boolean
  onLaunch: () => void
  onMore: () => void
  onToggleFavorite?: () => void
}

export function HeroSection({ game, rodando, abrindo, onLaunch, onMore }: HeroSectionProps) {
  const { t } = useI18n()
  if (!game) return <div className="retro-hero-section flex-1" />

  return (
    <section key={game.id} className="retro-hero-section anim-rise relative min-h-0 flex-1 border-t px-6 py-4">
      <div className="retro-game-dashboard grid h-full min-h-0">
        <article className="retro-game-copy flex min-w-0 flex-col justify-center pr-3">
          {game.logo ? (
            <img src={game.logo} alt={game.title} className="ps5-hero-logo" />
          ) : (
            <h1 className="game-name">{game.title}</h1>
          )}
          <p className="ps5-hero-tagline">{game.genre || game.developer || "Pronto para jogar."}</p>
          <div className="ps5-hero-actions">
            <button
              type="button"
              onClick={onLaunch}
              data-game-action={rodando ? "stop" : abrindo ? "cancel" : undefined}
              aria-label={
                rodando
                  ? t("hero.parar")
                  : abrindo
                    ? t("common.cancelar")
                    : game.installed === false
                      ? t("hero.instalar")
                      : t("hero.jogar")
              }
              className={`retro-play-button ${rodando ? "is-running" : ""} ${abrindo ? "is-opening" : ""}`}
            >
              {rodando ? "■" : abrindo ? "×" : "▶"}{" "}
              {rodando
                ? t("hero.parar")
                : abrindo
                  ? t("common.cancelar")
                  : game.installed === false
                    ? t("hero.instalar")
                    : t("hero.jogar")}
            </button>
            <button onClick={onMore} className="ps5-hero-more" aria-label={t("gameoverview.detalhes")}>•••</button>
          </div>
        </article>
      </div>
    </section>
  )
}

export function LauncherIcon({ launcher, size = 14 }: { launcher: string; size?: number }) {
  if (launcher === "steam") return <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor"><path d="M12 0A12 12 0 0 0 .02 11.04l6.43 2.66a3.4 3.4 0 0 1 2.1-.59l2.86-4.14v-.06a4.53 4.53 0 1 1 4.42 4.53l-4.07 2.91v.16a3.39 3.39 0 0 1-6.72.67L.44 15.27A12 12 0 1 0 12 0Zm-4.46 18.21-1.47-.61a2.55 2.55 0 1 0 3.27-3.46 2.53 2.53 0 0 0-1.88-.03l1.53.63a1.88 1.88 0 1 1-1.45 3.47Zm8.4-6.29a3.02 3.02 0 1 1 0-6.03 3.02 3.02 0 0 1 0 6.03Zm0-.75a2.27 2.27 0 1 0 0-4.53 2.27 2.27 0 0 0 0 4.53Z"/></svg>
  if (launcher === "heroic") return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 2 2 7l10 5 10-5-10-5ZM2 17l10 5 10-5M2 12l10 5 10-5"/></svg>
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor"><path d="M20 4H4a2 2 0 0 0-2 2v12c0 1.1.9 2 2 2h16a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2Zm-8 3a3.5 3.5 0 1 1 0 7 3.5 3.5 0 0 1 0-7Zm7 13H5v-.23c0-.62.28-1.2.76-1.58C7.47 17.18 9.64 17 12 17s4.53.18 6.24 1.19c.48.38.76.97.76 1.58V20Z"/></svg>
}
