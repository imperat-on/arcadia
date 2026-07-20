/**
 * "smooth" ou "auto", conforme a preferência de movimento do sistema.
 *
 * O `prefers-reduced-motion` do CSS neutraliza animações e `scroll-behavior`,
 * mas não alcança o `behavior` passado por JavaScript em `scrollIntoView` e
 * `scrollBy` — que é justamente como a loja rola. Daí esta consulta.
 */
export function rolagemSuave(): ScrollBehavior {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth"
}
