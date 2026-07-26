export function fmtNum(n: number | null | undefined) {
  if (typeof n !== "number" || !Number.isFinite(n)) return "—"
  return Math.round(n).toLocaleString("pt-BR")
}

export function fmtPct(n: number | null | undefined, casas = 0) {
  if (typeof n !== "number" || !Number.isFinite(n)) return "—"
  return `${n.toFixed(casas).replace(".", ",")}%`
}

export function fmtNotaSteam(positivePct: number | null | undefined) {
  if (typeof positivePct !== "number" || !Number.isFinite(positivePct)) return "—"
  return (positivePct / 20).toFixed(1).replace(".", ",")
}

export function fmtFaixaMenor(s: string | null | undefined) {
  const nums = String(s || "")
    .match(/[\d,.]+/g)
    ?.map((x) => Number(x.replace(/[,.]/g, "")))
    .filter(Number.isFinite) || []
  return nums.length ? nums[0] : null
}

export function fmtDataCurta(v: string | number | Date | null | undefined) {
  if (!v) return "—"
  const d = v instanceof Date ? v : new Date(v)
  if (Number.isNaN(d.getTime())) return String(v)
  return d.toLocaleDateString("pt-BR", { day: "2-digit", month: "short", year: "numeric" }).replace(" de ", " ").replace(" de ", " ")
}
