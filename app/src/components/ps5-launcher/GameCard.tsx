"use client"

import { useEffect, useRef, useState } from "react"
import type { Game } from "./types"
import { LauncherIcon } from "./HeroSection"
import { useI18n } from "../../i18n/I18nContext"

interface GameCardProps {
  game: Game
  focused: boolean
  onFocus: () => void
  onLaunch: () => void
  width: number | string
}

const FALLBACK_GRADIENTS: Record<string, string> = {
  steam: "linear-gradient(160deg, #1b2838 0%, #0d1a26 60%, #1b2838 100%)",
  heroic: "linear-gradient(160deg, #1c1f2e 0%, #0f1119 60%, #1e1028 100%)",
  lutris: "linear-gradient(160deg, #1a1a2e 0%, #16213e 60%, #0f3460 100%)",
  psn: "linear-gradient(160deg, #0a1a3a 0%, #04122b 60%, #002a6b 100%)",
}

export function GameCard({ game, focused, onFocus, onLaunch, width }: GameCardProps) {
  const cardRef = useRef<HTMLButtonElement>(null)
  const { t } = useI18n()
  const appid = game.launcher === "steam" ? String(game.id).replace(/^steam:/, "") : ""
  const portraitUrl = appid
    ? `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/library_600x900.jpg`
    : ""
  const headerUrl = appid
    ? `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/header.jpg`
    : ""
  const [faseCapa, setFaseCapa] = useState<"cover" | "portrait" | "header" | "none">(
    /\/(?:library_)?header\.jpg/i.test(game.cover || "") ? "portrait" : "cover",
  )
  useEffect(
    () => setFaseCapa(/\/(?:library_)?header\.jpg/i.test(game.cover || "") ? "portrait" : "cover"),
    [game.id, game.cover],
  )
  const coverSrc =
    faseCapa === "cover"
      ? game.cover || portraitUrl
      : faseCapa === "portrait"
        ? portraitUrl
        : faseCapa === "header"
          ? headerUrl
          : ""
  const isLandscape = /\/(?:library_)?header\.jpg/i.test(coverSrc)

  const hasCover = Boolean(coverSrc)
  const fallbackGradient =
    FALLBACK_GRADIENTS[game.launcher] ?? "linear-gradient(160deg, #0d0d0f 0%, #000000 100%)"

  return (
    <button
      ref={cardRef}
      onClick={() => {
        onFocus()
        onLaunch()
      }}
      className="relative flex-shrink-0 rounded-xl overflow-hidden cursor-pointer outline-none group"
      style={{
        width,
        aspectRatio: "2 / 3",
        transformOrigin: "center bottom",
        transition: "transform 0.35s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.25s",
        transform: focused ? "scale(1.06) translateY(-4px)" : "scale(0.94)",
        opacity: focused ? 1 : 0.7,
        zIndex: focused ? 10 : 1,
      }}
      aria-label={game.title}
    >
      {/* Card surface */}
      <div
        className="absolute inset-0 transition-all duration-300"
        style={{
          background: hasCover ? undefined : fallbackGradient,
        }}
      >
        {hasCover ? (
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={coverSrc}
            alt={game.title}
            className={`w-full h-full ${isLandscape ? "object-contain" : "object-cover"} transition-transform duration-500`}
            style={{
              transform: focused ? "scale(1.03)" : "scale(1)",
            }}
            loading="lazy"
            onError={() =>
              setFaseCapa((f) =>
                f === "cover" ? "portrait" : f === "portrait" ? "header" : "none",
              )
            }
          />
        ) : (
          /* Fallback art */
          <div className="w-full h-full flex flex-col items-center justify-center gap-3 p-4">
            <div
              className="w-14 h-14 rounded-xl flex items-center justify-center"
              style={{
                background: "rgba(255,255,255,0.08)",
                border: "1px solid rgba(255,255,255,0.1)",
              }}
            >
              <LauncherIcon launcher={game.launcher} size={26} />
            </div>
            <p className="text-white text-sm font-bold text-center text-balance leading-snug">
              {game.title}
            </p>
            <LauncherPill launcher={game.launcher} />
          </div>
        )}
      </div>

      {/* Selo "Instalar" para jogos possuídos mas não baixados */}
      {game.installed === false && (
        <div
          className="absolute top-2.5 right-2.5 z-10 flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg"
          style={{
            background: "rgba(0, 168, 255, 0.15)",
            backdropFilter: "blur(10px)",
            border: "1px solid rgba(0, 168, 255, 0.3)",
            boxShadow: "0 4px 15px rgba(0, 0, 0, 0.5)",
          }}
        >
          <svg width="11" height="11" viewBox="0 0 24 24" fill="var(--accent)">
            <path d="M5 20h14v-2H5v2zM19 9h-4V3H9v6H5l7 7 7-7z" />
          </svg>
          <span className="text-[10px] font-bold text-white drop-shadow">{t("gamecard.instalar")}</span>
        </div>
      )}

      {/* Borda elegante quando focado */}
      <div
        className="absolute inset-0 rounded-xl pointer-events-none transition-all duration-300"
        style={{
          border: focused
            ? "2px solid rgba(0, 168, 255, 0.5)"
            : "2px solid rgba(255,255,255,0.06)",
          boxShadow: focused
            ? "0 0 30px rgba(0, 168, 255, 0.4), 0 8px 40px rgba(0,0,0,0.7), inset 0 0 20px rgba(0, 168, 255, 0.08)"
            : "none",
        }}
      />

      {/* Halo de acento suave atrás do card em foco */}
      <div
        className="absolute -inset-3 rounded-2xl pointer-events-none opacity-50 blur-xl transition-all duration-400"
        style={{
          background: focused
            ? "radial-gradient(ellipse, rgba(0, 168, 255, 0.4) 0%, transparent 70%)"
            : "transparent",
        }}
      />

      {/* Bottom gradient with info (show on focus) */}
      <div
        className="absolute inset-x-0 bottom-0 flex flex-col justify-end px-3.5 pb-3.5 pt-12 transition-opacity duration-300"
        style={{
          background:
            "linear-gradient(to top, rgba(0,0,0,0.92) 0%, rgba(0,0,0,0.6) 40%, transparent 100%)",
          opacity: focused ? 1 : hasCover ? 0 : 1,
        }}
      >
        {hasCover && (
          <p className="text-white text-sm font-semibold leading-tight text-balance mb-1.5 drop-shadow-lg">
            {game.title}
          </p>
        )}
        <LauncherPill launcher={game.launcher} />
      </div>

      {/* Linha superior sutil quando focado */}
      {focused && (
        <div
          className="absolute top-0 inset-x-0 h-[2px] rounded-t-xl"
          style={{
            background: "linear-gradient(90deg, transparent, rgba(0, 168, 255, 0.6), transparent)",
            boxShadow: "0 0 10px rgba(0, 168, 255, 0.5)",
          }}
        />
      )}
    </button>
  )
}

function LauncherPill({ launcher }: { launcher: string }) {
  const { t } = useI18n()
  const config: Record<string, { label: string; color: string }> = {
    steam: { label: t("gamecard.steam"), color: "#7ba4c7" },
    heroic: { label: t("gamecard.heroic"), color: "#f9a020" },
    lutris: { label: t("gamecard.lutris"), color: "#ff7300" },
    psn: { label: t("gamecard.psn"), color: "#4a9eff" },
  }
  const c = config[launcher] ?? { label: launcher, color: "#aaa" }
  return (
    <div className="flex items-center gap-1">
      <LauncherIcon launcher={launcher} size={10} />
      <span className="text-[10px] font-medium" style={{ color: c.color }}>
        {c.label}
      </span>
    </div>
  )
}
