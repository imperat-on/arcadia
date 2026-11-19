"use client"

import { useEffect, type RefObject } from "react"

/**
 * Janela em que o gesto de scroll é considerado ativo.
 *
 * A roda emite eventos em rajadas curtas; rearmar o timer a cada um mantém a
 * marca ligada durante todo o gesto real sem piscar.
 */
export const SCROLL_IDLE_MS = 140

/**
 * Liga uma classe no elemento enquanto houver scroll, sem estado React.
 *
 * O motivo de não ser `useState`: numa grade de 24+ cards, um setState no
 * componente-pai re-renderiza a grade inteira duas vezes por gesto (entrada e
 * saída da classe) — e esse é justamente o custo que a classe existe para
 * evitar. Mexer no `classList` não toca na árvore React.
 *
 * O listener é registrado no `window` em fase de captura porque o container
 * rolável real é um ancestral (o palco da loja), não o elemento apontado pela
 * ref. `passive` evita que o navegador espere o handler para rolar.
 */
export function useScrollClassFlag(
  ref: RefObject<HTMLElement | null>,
  classe: string,
) {
  useEffect(() => {
    const elemento = ref.current
    if (!elemento) return

    let timer: ReturnType<typeof setTimeout> | null = null

    const ligar = () => {
      if (!elemento.classList.contains(classe)) elemento.classList.add(classe)
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        elemento.classList.remove(classe)
        timer = null
      }, SCROLL_IDLE_MS)
    }

    window.addEventListener("scroll", ligar, { capture: true, passive: true })
    return () => {
      window.removeEventListener("scroll", ligar, { capture: true })
      if (timer) clearTimeout(timer)
      elemento.classList.remove(classe)
    }
  }, [ref, classe])
}
