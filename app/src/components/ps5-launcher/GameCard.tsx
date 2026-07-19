"use client"

import { useRef } from "react"
import type { Game } from "./types"
import { LauncherIcon } from "./HeroSection"

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

  const hasCover = Boolean(game.cover)
  const fallbackGradient =
    FALLBACK_GRADIENTS[game.launcher] ??
    "linear-gradient(160deg, #0d0d0f 0%, #000000 100%)"

  return (
    <button
      ref={cardRef}
      onClick={() => { onFocus(); onLaunch() }}
      className="relative flex-shrink-0 rounded-xl overflow-hidden cursor-pointer outline-none group"
      style={{
        width,
        aspectRatio: "2 / 3",
        transformOrigin: "center bottom",
        transition: "transform 0.32s cubic-bezier(0.22, 1, 0.36, 1)",
        transform: focused ? "scale(1.06)" : "scale(0.94)",
        opacity: focused ? 1 : 0.72,
        zIndex: focused ? 10 : 1,
      }}
      aria-label={`${game.title} — iniciar`}
    >
      {/* Card surface */}
      <div
        className="absolute inset-0"
        style={{
          background: hasCover ? undefined : fallbackGradient,
        }}
      >
        {hasCover ? (
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={game.cover}
            alt={`Capa de ${game.title}`}
            className="w-full h-full object-cover"
            loading="lazy"
          />
        ) : (
          /* Fallback art */
          <div className="w-full h-full flex flex-col items-center justify-center gap-3 p-4">
            <div
              className="w-12 h-12 rounded-xl flex items-center justify-center"
              style={{ background: "rgba(255,255,255,0.08)" }}
            >
              <LauncherIcon launcher={game.launcher} size={24} />
            </div>
            <p className="text-white text-xs font-semibold text-center text-balance leading-snug">
              {game.title}
            </p>
            <LauncherPill launcher={game.launcher} />
          </div>
        )}
      </div>

      {/* Selo "Instalar" para jogos possuídos mas não baixados */}
      {game.installed === false && (
        <div
          className="absolute top-2 right-2 z-10 flex items-center gap-1 px-2 py-1 rounded-md"
          style={{
            background: "rgba(0,0,0,0.65)",
            backdropFilter: "blur(4px)",
            border: "1px solid rgba(0,168,255,0.35)",
          }}
        >
          <svg width="10" height="10" viewBox="0 0 24 24" fill="#00a8ff">
            <path d="M5 20h14v-2H5v2zM19 9h-4V3H9v6H5l7 7 7-7z" />
          </svg>
          <span className="text-[9px] font-semibold text-white">Instalar</span>
        </div>
      )}

      {/* Focused glow border */}
      <div
        className="absolute inset-0 rounded-xl pointer-events-none"
        style={{
          border: focused
            ? "2px solid var(--accent)"
            : "2px solid rgba(255,255,255,0.06)",
          boxShadow: focused
            ? "0 0 0 1px var(--accent), 0 8px 40px rgba(0,0,0,0.6), 0 0 55px color-mix(in srgb, var(--accent) 45%, transparent)"
            : "none",
          transition: "border-color 0.25s, box-shadow 0.25s",
        }}
      />

      {/* Bottom gradient with info (show on focus) */}
      <div
        className="absolute inset-x-0 bottom-0 flex flex-col justify-end px-3 pb-3 pt-10"
        style={{
          background:
            "linear-gradient(to top, rgba(0,0,0,0.9) 0%, rgba(0,0,0,0.5) 50%, transparent 100%)",
          opacity: focused ? 1 : hasCover ? 0 : 1,
          transition: "opacity 0.25s",
        }}
      >
        {hasCover && (
          <p className="text-white text-xs font-semibold leading-tight text-balance mb-1">
            {game.title}
          </p>
        )}
        <LauncherPill launcher={game.launcher} />
      </div>

      {/* Focus indicator top bar */}
      <div
        className="absolute top-0 inset-x-0 h-0.5 rounded-t-xl"
        style={{
          background: "linear-gradient(90deg, transparent, var(--accent), transparent)",
          opacity: focused ? 1 : 0,
          transition: "opacity 0.25s",
        }}
      />
    </button>
  )
}

function LauncherPill({ launcher }: { launcher: string }) {
  const config: Record<string, { label: string; color: string }> = {
    steam: { label: "Steam", color: "#7ba4c7" },
    heroic: { label: "Heroic", color: "#f9a020" },
    lutris: { label: "Lutris", color: "#ff7300" },
    psn: { label: "PSN", color: "#4a9eff" },
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
