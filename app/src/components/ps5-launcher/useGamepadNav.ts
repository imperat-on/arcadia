"use client"

import { useEffect, type RefObject } from "react"

// Navegação por controle em QUALQUER overlay: move o foco entre os elementos
// (navegação espacial), A ativa (click), B volta/fecha.

/** Um focável e o retângulo dele, medidos uma vez só por movimento. */
type Alvo = { el: HTMLElement; cx: number; cy: number }

/**
 * Até onde procurar um vizinho, em telas. A grade de uma categoria chega a
 * centenas de capas: sem limite, o "melhor" alvo podia estar a milhares de
 * pixels e o foco sumia da tela em vez de parar na borda. Uma tela e meia
 * cobre a linha seguinte com folga.
 */
const ALCANCE = 1.5

function focaveis(root: HTMLElement, cx: number, cy: number, alcance = ALCANCE): Alvo[] {
  const sel =
    'button, a[href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
  const limiteX = window.innerWidth * alcance
  const limiteY = window.innerHeight * alcance
  const out: Alvo[] = []
  for (const el of root.querySelectorAll<HTMLElement>(sel)) {
    if (el.hasAttribute("disabled")) continue
    // Um getBoundingClientRect por elemento por movimento. Antes eram dois:
    // um para saber se estava visível e outro para pontuar a direção.
    const r = el.getBoundingClientRect()
    if (r.width <= 0 || r.height <= 0) continue
    const s = getComputedStyle(el)
    if (s.visibility === "hidden" || s.display === "none") continue
    const ex = r.left + r.width / 2
    const ey = r.top + r.height / 2
    if (Math.abs(ex - cx) > limiteX || Math.abs(ey - cy) > limiteY) continue
    out.push({ el, cx: ex, cy: ey })
  }
  return out
}

function bestInDirection(
  cx: number,
  cy: number,
  list: Alvo[],
  atual: HTMLElement | null,
  dx: number,
  dy: number,
): HTMLElement | null {
  let best: HTMLElement | null = null
  let bestScore = Infinity
  for (const a of list) {
    if (a.el === atual) continue
    const ddx = a.cx - cx
    const ddy = a.cy - cy
    const along = ddx * dx + ddy * dy // distância "na frente"
    if (along <= 4) continue
    const perp = Math.abs(ddx * dy - ddy * dx) // desalinhamento
    const score = along + perp * 2.2
    if (score < bestScore) {
      bestScore = score
      best = a.el
    }
  }
  return best
}

/** O elemento está, ao menos em parte, dentro da viewport? */
function naTela(el: HTMLElement): boolean {
  const r = el.getBoundingClientRect()
  return (
    r.bottom > 0 && r.top < window.innerHeight && r.right > 0 && r.left < window.innerWidth
  )
}

/**
 * Põe o foco no focável mais próximo do centro da tela.
 *
 * Serve às duas pontas: ao direcional, quando o foco ficou fora da viewport, e
 * ao analógico, quando a rolagem para — é o que faz A, X e Y agirem no jogo que
 * está sendo olhado, e não num que saiu da tela.
 *
 * Só age se o foco atual saiu da tela. Sem essa condição, rolar a régua de
 * abas de lado (com o foco numa aba, que continua visível) jogaria o foco para
 * uma capa no meio da tela — brigando com quem está navegando pelas abas.
 */
function focarNoCentro(root: HTMLElement): void {
  const ativo = document.activeElement as HTMLElement | null
  if (ativo && root.contains(ativo) && naTela(ativo)) return
  const ax = window.innerWidth / 2
  const ay = window.innerHeight / 2
  const perto = focaveis(root, ax, ay)
  if (!perto.length) return
  let melhor = perto[0]
  let dist = Infinity
  for (const a of perto) {
    const d = Math.abs(a.cx - ax) + Math.abs(a.cy - ay)
    if (d < dist) {
      dist = d
      melhor = a
    }
  }
  if (melhor.el !== document.activeElement) melhor.el.focus()
}

/**
 * Quais eixos são o analógico DIREITO.
 *
 * Antes, o laço varria todos os eixos a partir do 2 e tratava o de maior
 * deflexão como vertical: empurrar o analógico para o LADO rolava a página
 * para baixo, e um gatilho analógico fazia o mesmo.
 *
 * Com `mapping === "standard"` a especificação garante 2 = X e 3 = Y, e é
 * assim que o Chromium — logo, o Electron — mapeia Xbox, DualShock e
 * DualSense. Fora do padrão, o repouso calibrado separa analógico de gatilho:
 * analógico descansa em 0, gatilho descansa em −1 nos mapeamentos SDL/Linux.
 */
function eixosDireito(gp: Gamepad, rest: number[]): [number, number] {
  if (gp.mapping === "standard") return [2, 3]
  for (let i = 2; i + 1 < gp.axes.length; i++) {
    if (Math.abs(rest[i] ?? 0) < 0.5 && Math.abs(rest[i + 1] ?? 0) < 0.5) return [i, i + 1]
  }
  return [2, 3]
}

/** Rolável na horizontal? Os trilhos da vitrine e a régua de abas são. */
function rolaNaHorizontal(el: HTMLElement): boolean {
  const ox = getComputedStyle(el).overflowX
  return (ox === "auto" || ox === "scroll") && el.scrollWidth > el.clientWidth + 8
}

/**
 * O trilho que o analógico horizontal deve mover: o do elemento focado; sem
 * foco, o que estiver mais perto do meio da tela — que é o que a pessoa está
 * olhando.
 */
function acharTrilho(root: HTMLElement, focado: HTMLElement | null): HTMLElement | null {
  for (let el = focado; el && el !== root.parentElement; el = el.parentElement) {
    if (rolaNaHorizontal(el)) return el
  }
  const meio = window.innerHeight / 2
  let melhor: HTMLElement | null = null
  let dist = Infinity
  for (const el of root.querySelectorAll<HTMLElement>("*")) {
    if (!rolaNaHorizontal(el)) continue
    const r = el.getBoundingClientRect()
    if (r.height <= 0) continue
    const d = Math.abs(r.top + r.height / 2 - meio)
    if (d < dist) {
      dist = d
      melhor = el
    }
  }
  return melhor
}

/**
 * O elemento que de fato rola. A raiz vem primeiro (é o caso de todos os
 * overlays), mas a loja tem a raiz `overflow-hidden` e rola numa div interna —
 * ali o analógico direito não fazia nada.
 */
function acharScroller(root: HTMLElement): HTMLElement {
  const sobra = (el: HTMLElement) => {
    const oy = getComputedStyle(el).overflowY
    if (oy !== "auto" && oy !== "scroll") return 0
    return el.scrollHeight - el.clientHeight
  }
  if (sobra(root) > 8) return root

  // O MAIOR sobrando, não o primeiro que aparece. Detalhe do CSS: quando um
  // eixo deixa de ser `visible`, o outro vira `auto` sozinho — então os
  // trilhos horizontais da vitrine e a régua de abas contam como roláveis na
  // vertical. Pegar o primeiro da lista entregaria a régua de abas, e o
  // analógico direito mexeria nela em vez da página.
  let melhor = root
  let maior = 8
  for (const el of root.querySelectorAll<HTMLElement>("*")) {
    const s = sobra(el)
    if (s > maior) {
      maior = s
      melhor = el
    }
  }
  return melhor
}

export function useGamepadNav(
  rootRef: RefObject<HTMLElement | null>,
  active: boolean,
  onBack?: () => void,
  scrollOnly = false, // true: só o scroll do analógico direito (sem foco/botões)
  // Atalhos por tela. A loja usa para baixar (X) e adicionar (Y) sem precisar
  // abrir a página do jogo. Recebem o elemento focado, que é como quem chama
  // sabe de qual item se trata.
  extras?: { onX?: (alvo: HTMLElement | null) => void; onY?: (alvo: HTMLElement | null) => void },
) {
  useEffect(() => {
    if (!active) return
    const root = rootRef.current
    if (!root) return

    // Foca o 1º elemento ao abrir.
    if (!scrollOnly) {
      // Sem poda aqui: na abertura o alvo pode estar em qualquer lugar da tela.
      const first = focaveis(root, 0, 0, Infinity)[0]
      if (first && !root.contains(document.activeElement)) {
        requestAnimationFrame(() => first.el.focus())
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
    let scrollVelX = 0 // idem, na horizontal (trilhos)
    let rolou = false // houve rolagem desde a última parada
    let scroller: HTMLElement | null = null
    let trilho: HTMLElement | null = null // rolável horizontal do gesto atual
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
      const ativo = document.activeElement as HTMLElement | null
      const dentro = ativo && root2.contains(ativo) ? ativo : null

      // Ponto de partida. Quando o foco saiu da tela — típico depois de rolar
      // com o analógico direito —, partir dele faria a tela saltar de volta
      // para onde não se está mais olhando. Nesse caso, reancora no que está
      // mais perto do centro da viewport.
      let ax: number // âncora do movimento (cx/cy do escopo de fora são a direção)
      let ay: number
      const r = dentro?.getBoundingClientRect()
      const naTela =
        r && r.bottom > 0 && r.top < window.innerHeight && r.right > 0 && r.left < window.innerWidth
      if (r && naTela) {
        ax = r.left + r.width / 2
        ay = r.top + r.height / 2
      } else {
        // Reancorar já É o movimento: trazer o foco para a tela e ainda pular
        // um card faria a rolagem parecer que passou do ponto.
        focarNoCentro(root2)
        return
      }

      const list = focaveis(root2, ax, ay)
      if (!list.length) return
      const next = bestInDirection(ax, ay, list, dentro, dx, dy)
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
        // Analógico DIREITO: para baixo rola a página, para os lados percorre
        // o trilho horizontal em foco. Rolagem suave, estilo navegador — sem
        // pular de card em card —, com a velocidade interpolada (inércia leve).
        //
        // O scroller vertical é resolvido sob demanda e memorizado: a raiz não
        // é sempre quem rola
        // (na loja é uma div interna), e varrer o DOM a cada quadro para
        // descobrir isso seria desperdício.
        if (scroller && !document.contains(scroller)) scroller = null
        if (!scroller && rootRef.current) scroller = acharScroller(rootRef.current)
        if (rest && rootRef.current) {
          const [ix, iy] = eixosDireito(gp, rest)
          const ex = (gp.axes[ix] ?? 0) - (rest[ix] ?? 0)
          const ey = (gp.axes[iy] ?? 0) - (rest[iy] ?? 0)
          // Curva quadrática: o começo do curso é fino para posicionar, o fim
          // é rápido para atravessar a página. Zona morta contra analógico
          // gasto, que rolaria sozinho.
          const vel = (v: number) => (Math.abs(v) > 0.15 ? Math.sign(v) * v * v * 46 : 0)

          scrollVel += (vel(ey) - scrollVel) * 0.25
          if (scroller && Math.abs(scrollVel) > 0.05) scroller.scrollTop += scrollVel

          // O trilho é escolhido UMA VEZ por gesto, quando o analógico sai do
          // repouso, e vale até ele voltar. Procurar a cada quadro varreria o
          // DOM da loja inteira 60 vezes por segundo; e reescolher no meio do
          // movimento faria o gesto pular de um trilho para outro sozinho.
          const alvoX = vel(ex)
          if (alvoX || Math.abs(scrollVelX) > 0.05) {
            if (!trilho || !document.contains(trilho)) {
              const focado = document.activeElement as HTMLElement | null
              trilho = acharTrilho(
                rootRef.current,
                focado && rootRef.current.contains(focado) ? focado : null,
              )
            }
            scrollVelX += (alvoX - scrollVelX) * 0.25
            if (trilho && Math.abs(scrollVelX) > 0.05) trilho.scrollLeft += scrollVelX
          } else {
            scrollVelX = 0
            trilho = null
          }

          // O foco só acompanha com o analógico PARADO. Focar durante a
          // rolagem dispara o scrollIntoView do ladrilho, que brigaria com a
          // inércia daqui e faria a tela tremer.
          const rolando = Math.abs(scrollVel) > 0.05 || Math.abs(scrollVelX) > 0.05
          if (!scrollOnly) {
            if (rolando) rolou = true
            else if (rolou) {
              rolou = false
              focarNoCentro(rootRef.current)
            }
          }
        }

        // A (0) = ativar; B (1) = voltar
        if (!scrollOnly && primed && gp.buttons[0]?.pressed && !prev[0]) {
          const el = document.activeElement as HTMLElement | null
          if (el && rootRef.current?.contains(el)) el.click()
        }
        if (!scrollOnly && primed && gp.buttons[1]?.pressed && !prev[1]) onBack?.()
        if (!scrollOnly && primed && gp.buttons[2]?.pressed && !prev[2]) {
          const el = document.activeElement as HTMLElement | null
          extras?.onX?.(rootRef.current?.contains(el) ? el : null)
        }
        if (!scrollOnly && primed && gp.buttons[3]?.pressed && !prev[3]) {
          const el = document.activeElement as HTMLElement | null
          extras?.onY?.(rootRef.current?.contains(el) ? el : null)
        }
        prev = gp.buttons.map((b) => b.pressed)
      }
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [active, rootRef, onBack, scrollOnly, extras])
}
