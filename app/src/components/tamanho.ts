// Tamanhos de arquivo com a unidade certa para a grandeza.
//
// A escada é de 1024, então a unidade é MiB/GiB — e não MB/GB, que seriam
// 1000. O projeto tinha cinco formatadores espalhados, cada um preso a uma
// unidade só: a aba Downloads mostrava "61440 MiB" para um jogo de 60 GB, e o
// diálogo de instalação mostrava "0.78 GiB" para um jogo pequeno. Mesmo
// defeito em direções opostas.

/**
 * Corta zeros à direita da parte decimal: "1.00" vira "1", "278.50" vira
 * "278.5". A âncora no ponto é o que impede "100" de virar "1".
 */
const enxuto = (n: number, casas: number) =>
  n
    .toFixed(casas)
    .replace(/(\.\d*?)0+$/, "$1")
    .replace(/\.$/, "")

/** Recebe MiB. Sobe para GiB a partir de 1024. */
export function fmtMiB(mib?: number): string {
  if (!mib || Number.isNaN(mib)) return "—"
  if (mib >= 1024) return `${enxuto(mib / 1024, 1)} GiB`
  return `${mib.toFixed(0)} MiB`
}

/** Recebe GiB. Desce para MiB abaixo de 1. */
export function fmtGiB(gib?: number): string {
  if (!gib || Number.isNaN(gib)) return "—"
  if (gib < 1) return `${(gib * 1024).toFixed(0)} MiB`
  return `${enxuto(gib, 2)} GiB`
}

/** Recebe bytes. Atalho para quem tem o valor cru. */
export function fmtBytes(bytes?: number): string {
  if (!bytes || Number.isNaN(bytes)) return "—"
  return fmtMiB(bytes / 1024 ** 2)
}
