"use client"

import { useEffect, type RefObject } from "react"

// Navegação por controle em QUALQUER overlay: move o foco entre os elementos
// (navegação espacial), A ativa (click), B volta/fecha.

function visible(el: HTMLElement): boolean {
  const r = el.getBoundingClientRect()
  if (r.width <= 0 || r.height <= 0) return false
  const s = getComputedStyle(el)
  return s.visibility !== "hidden" && s.display !== "none"
}

function getFocusables(root: HTMLElement): HTMLElement[] {
  const sel =
    'button, a[href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
  return Array.from(root.querySelectorAll<HTMLElement>(sel)).filter(
    (el) => !el.hasAttribute("disabled") && visible(el),
  )
}

function bestInDirection(
  current: HTMLElement,
  list: HTMLElement[],
  dx: number,
  dy: number,
): HTMLElement | null {
  const cr = current.getBoundingClientRect()
  const cx = cr.left + cr.width / 2
  const cy = cr.top + cr.height / 2
  let best: HTMLElement | null = null
  let bestScore = Infinity
  for (const el of list) {
    if (el === current) continue
    const r = el.getBoundingClientRect()
    const ex = r.left + r.width / 2
    const ey = r.top + r.height / 2
    const ddx = ex - cx
    const ddy = ey - cy
    const along = ddx * dx + ddy * dy // distância "na frente"
    if (along <= 4) continue
    const perp = Math.abs(ddx * dy - ddy * dx) // desalinhamento
    const score = along + perp * 2.2
    if (score < bestScore) {
      bestScore = score
      best = el
    }
  }
  return best
}

export function useGamepadNav(
  rootRef: RefObject<HTMLElement | null>,
  active: boolean,
  onBack?: () => void,
  scrollOnly = false, // true: só o scroll do analógico direito (sem foco/botões)
) {
  useEffect(() => {
    if (!active) return
    const root = rootRef.current
    if (!root) return

    // Foca o 1º elemento ao abrir.
    if (!scrollOnly) {
      const first = getFocusables(root)[0]
      if (first && !root.contains(document.activeElement)) {
        requestAnimationFrame(() => first.focus())
      }
    }

    let raf = 0
    let prev: boolean[] = []
    let rest: number[] | null = null
    let sx = 0, sy = 0 // direção estável
    let cx = 0, cy = 0 // direção candidata
    let candSince = 0
    let holdStart = 0
    let lastRepeat = 0
    let lastStep = 0
    let scrollVel = 0 // velocidade do scroll suave (analógico direito)
    const DEBOUNCE = 90
    const MIN_GAP = 200
    const INITIAL_DELAY = 500
    const REPEAT = 260

    const dirOf = (gp: Gamepad): [number, number] => {
      let x = 0
      let y = 0
      if (gp.buttons[15]?.pressed) x = 1
      else if (gp.buttons[14]?.pressed) x = -1
      if (gp.buttons[13]?.pressed) y = 1
      else if (gp.buttons[12]?.pressed) y = -1
      if (!rest) rest = Array.from(gp.axes)
      const ax = (gp.axes[0] ?? 0) - (rest[0] ?? 0)
      const ay = (gp.axes[1] ?? 0) - (rest[1] ?? 0)
      if (!x) x = ax > 0.6 ? 1 : ax < -0.6 ? -1 : 0
      if (!y) y = ay > 0.6 ? 1 : ay < -0.6 ? -1 : 0
      // D-pad como hat (eixo 9)
      const h = gp.axes[9]
      if ((!x && !y) && typeof h === "number" && h >= -1.05 && h <= 1.05) {
        const near = (t: number) => Math.abs(h - t) < 0.1
        if (near(-1)) y = -1
        else if (near(-0.714)) { x = 1; y = -1 }
        else if (near(-0.428)) x = 1
        else if (near(-0.142)) { x = 1; y = 1 }
        else if (near(0.142)) y = 1
        else if (near(0.428)) { x = -1; y = 1 }
        else if (near(0.714)) x = -1
        else if (near(1)) { x = -1; y = -1 }
      }
      return [x, y]
    }

    const move = (dx: number, dy: number) => {
      const root2 = rootRef.current
      if (!root2) return
      const list = getFocusables(root2)
      if (!list.length) return
      const cur =
        document.activeElement && root2.contains(document.activeElement)
          ? (document.activeElement as HTMLElement)
          : list[0]
      const next = bestInDirection(cur, list, dx, dy)
      if (next) next.focus()
    }

    const loop = () => {
      // Janela sem foco: ignora o controle (Gamepad API entrega input desfocada).
      if (!document.hasFocus()) {
        prev = []
        scrollVel = 0
        raf = requestAnimationFrame(loop)
        return
      }
      const pads = navigator.getGamepads ? navigator.getGamepads() : []
      const gp = Array.from(pads).find((p) => p) || null
      if (gp) {
        const now = Date.now()
        const primed = prev.length > 0
        if (!rest) rest = Array.from(gp.axes) // calibração (tb usada pelo scrollOnly)
        const [rx, ry] = scrollOnly ? [0, 0] : dirOf(gp)
        if (rx !== cx || ry !== cy) {
          cx = rx
          cy = ry
          candSince = now
        }
        if (now - candSince >= DEBOUNCE && (sx !== cx || sy !== cy)) {
          const wasNeutral = sx === 0 && sy === 0
          sx = cx
          sy = cy
          if ((sx || sy) && wasNeutral && now - lastStep >= MIN_GAP) {
            move(sx, sy)
            lastStep = now
            holdStart = now
            lastRepeat = now
          }
        }
        if (
          (sx || sy) &&
          now - holdStart > INITIAL_DELAY &&
          now - lastRepeat > REPEAT
        ) {
          move(sx, sy)
          lastRepeat = now
          lastStep = now
        }
        // Analógico DIREITO rola a página suavemente, estilo navegador /
        // tela de "aceitar termos" — sem pular de card em card. A velocidade
        // é interpolada (inércia leve) e o eixo é detectado entre os comuns
        // (3 = RY padrão W3C, 5 = alguns controles via SDL).
        const scroller = rootRef.current
        if (scroller && rest) {
          let ry = 0
          // Eixo do analógico direito varia por controle/driver — varre todos
          // além do esquerdo (0/1) e usa o de maior deflexão.
          for (let ai = 2; ai < gp.axes.length; ai++) {
            const v = (gp.axes[ai] ?? 0) - (rest[ai] ?? 0)
            if (Math.abs(v) > Math.abs(ry)) ry = v
          }
          const target = Math.abs(ry) > 0.15 ? Math.sign(ry) * ry * ry * 46 : 0
          scrollVel += (target - scrollVel) * 0.25
          if (Math.abs(scrollVel) > 0.05) scroller.scrollTop += scrollVel
        }

        // A (0) = ativar; B (1) = voltar
        if (!scrollOnly && primed && gp.buttons[0]?.pressed && !prev[0]) {
          const el = document.activeElement as HTMLElement | null
          if (el && rootRef.current?.contains(el)) el.click()
        }
        if (!scrollOnly && primed && gp.buttons[1]?.pressed && !prev[1]) onBack?.()
        prev = gp.buttons.map((b) => b.pressed)
      }
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [active, rootRef, onBack, scrollOnly])
}
