// TypeScript · railEdgeMotion.ts · Electron renderer · Big Picture
const activeMotion = new WeakMap<HTMLElement, Animation>()

export function signalRailEdge(card: HTMLElement | null, direction: number) {
  const cover = card?.querySelector<HTMLElement>(".retro-library-cover")
  if (!cover) return
  const previous = activeMotion.get(cover)
  if (previous?.playState === "running") return
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches
  const motion = cover.animate(reduced
    ? [{ outline: "2px solid var(--accent)", outlineOffset: "-4px" }, { outline: "2px solid transparent", outlineOffset: "-4px" }]
    : [{ translate: "0" }, { translate: `${Math.sign(direction) * 5}px`, offset: .3 }, { translate: "0" }],
  { duration: reduced ? 160 : 190, easing: "ease-out" })
  activeMotion.set(cover, motion)
}
