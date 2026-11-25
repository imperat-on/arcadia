// TypeScript · React renderer · Arcadia Desktop and Big Picture
import { useI18n } from "../i18n/I18nContext"
import { Modal } from "../ui/Modal"
import type { GameSessionSummary } from "./useGameReturnMotion"

export function GameReturnSummary({ summary, reviewing, onReview, onClose, gamepad, shortcut = false }: {
  summary: GameSessionSummary | null
  reviewing: boolean
  onReview: () => void
  onClose: () => void
  gamepad: boolean
  shortcut?: boolean
}) {
  const { t } = useI18n()
  if (!summary) return null
  return (
    <>
      <aside className="pointer-events-none fixed bottom-6 left-6 z-[72] max-w-[min(26rem,85vw)]">
        <div className="arcadia-session-summary rounded-xl border border-white/15 bg-[color:var(--surface-3)] px-4 py-3 text-white shadow-xl">
          <p className="truncate text-sm font-semibold">{summary.title}</p>
          <p role="status" className="text-xs text-white/75">
            {t("session.summary", {
              time: `${Math.floor(summary.seconds / 60)}:${String(summary.seconds % 60).padStart(2, "0")}`,
              count: summary.trophies.length,
            })}
          </p>
          {!!summary.trophies.length && <button type="button" className="detail-achievements-all pointer-events-auto mt-2" onClick={onReview}>
            {t(shortcut ? "session.trophies_gamepad" : "session.trophies")}
          </button>}
        </div>
      </aside>
      {reviewing && <Modal title={t("session.trophies")} description={summary.title} onClose={onClose} gamepad={gamepad} showClose size="lg">
        <ul data-gamepad-scroll className="max-h-[55vh] space-y-2 overflow-y-auto">
          {summary.trophies.map(trophy => <li key={trophy.apiname || trophy.key} tabIndex={0} className="flex items-center gap-3 rounded-md border border-white/10 px-3 py-2">
            {trophy.icon && <img src={trophy.icon} alt="" className="h-9 w-9 shrink-0 rounded object-cover" />}
            <span className="min-w-0 break-words text-sm font-semibold">{trophy.title}</span>
            {(trophy.platinum || trophy.rare) && <span className="ml-auto text-right text-xs text-[var(--state-info-soft)]">{t(trophy.platinum ? "trofeus.platina" : "trofeus.raro")}</span>}
          </li>)}
        </ul>
      </Modal>}
    </>
  )
}
