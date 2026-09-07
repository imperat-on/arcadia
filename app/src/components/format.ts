import { userLocale } from "../i18n/locale"

export function fmtNum(n: number | null | undefined) {
  if (typeof n !== "number" || !Number.isFinite(n)) return "—"
  return Math.round(n).toLocaleString(userLocale())
}

export function fmtNotaSteam(positivePct: number | null | undefined) {
  if (typeof positivePct !== "number" || !Number.isFinite(positivePct)) return "—"
  return (positivePct / 20).toFixed(1).replace(".", ",")
}
