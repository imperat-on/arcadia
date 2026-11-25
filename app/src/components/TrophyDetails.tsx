// TypeScript · TrophyDetails.tsx · React · Desktop and Big Picture
import { useEffect, useRef, useState } from "react"
import type { TrophyItem, TrophySummary } from "../global"
import { useI18n } from "../i18n/I18nContext"
import { userLocale } from "../i18n/locale"
import { Modal } from "../ui/Modal"

const filters = ["todos", "desbloqueados", "pendentes"] as const
const control = "trophy-control"
const unknown: TrophySummary = { items: [], catalog: "unknown", progress: "unknown", done: 0, total: 0, percent: null, stale: false }
export const unavailableTrophies = { ...unknown, stale: true }

function TrophyArt({ icon, hidden }: { icon?: string; hidden?: boolean }) {
  return <span className="trophy-art" aria-hidden="true">
    {!hidden && icon ? <img src={icon} alt="" loading="lazy" /> :
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
        {hidden ? <path d="M8 11V7a4 4 0 0 1 8 0v4M7 11h10a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2v-6a2 2 0 0 1 2-2Zm5 4v2" /> :
          <path d="M8 3h8v6a4 4 0 0 1-8 0V3Zm0 2H4v2a4 4 0 0 0 4 4m8-6h4v2a4 4 0 0 1-4 4m-4 2v7m-4 0h8" />}
      </svg>}
  </span>
}

export function TrophyStatus({ summary }: { summary: TrophySummary | null }) {
  const { t } = useI18n()
  if (!summary) return <>{t("common.carregando")}</>
  const { catalog, percent, done, total } = summary
  if (summary.platinum?.awarded) return <>{t("trofeus.platina")}</>
  if (catalog === "empty") return <>{t("conquistas.vazio")}</>
  if (catalog === "unknown") return <>{t("trofeus.sem_catalogo")}</>
  if (catalog === "partial") return <>{summary.progress === "detected"
    ? t("trofeus.parcial", { done, total })
    : `${t("trofeus.itens_conhecidos", { total })} · ${t("trofeus.sem_progresso")}`}</>
  if (percent == null) return <>{catalog === "known"
    ? `${t("trofeus.catalogo_disponivel")} · ${t("trofeus.sem_progresso")}`
    : t("trofeus.sem_progresso")}</>
  return <>{t("conquistas.contador", { done, total })} · {percent}%</>
}

// Older Steam caches encode missing rarity as zero. Don't label it the rarest.
function rarity(item: TrophyItem) {
  const value = item.percent
  return typeof value === "number" && value > 0 && value <= 100 ? value : Infinity
}

export function TrophyDetails({ summary, title, source, onClose, onCapture, gamepad }: {
  summary: TrophySummary | null
  title?: string
  source: "steam" | "retro"
  onClose: () => void
  onCapture?: () => Promise<boolean>
  gamepad?: boolean
}) {
  const { t } = useI18n()
  const [filter, setFilter] = useState<typeof filters[number]>("todos")
  const [sort, setSort] = useState<"recentes" | "raridade">("recentes")
  const [group, setGroup] = useState<"all" | "base" | "dlc">("all")
  const [selected, setSelected] = useState<string | null>(null)
  const [showSource, setShowSource] = useState(false)
  const [revealed, setRevealed] = useState<string | null>(null)
  const selectedRow = useRef<HTMLButtonElement>(null)
  const [capture, setCapture] = useState<"busy" | "ok" | "error" | null>(null)
  const tracked = summary?.progress === "detected"
  const hasRarity = source === "steam" && summary?.items.some(item => Number.isFinite(rarity(item)))
  const rows = (summary?.items ?? []).map((item, index) => ({ item, id: `${item.apiname ?? item.id ?? `${item.block}:${item.bit}`}:${index}` }))
    .filter(({ item }) => group === "all" || item.trophyGroup === group)
    .filter(({ item }) => !tracked || filter === "todos" || !!item.achieved === (filter === "desbloqueados"))
    .sort((a, b) => sort === "raridade" && hasRarity
      ? rarity(a.item) - rarity(b.item)
      : (b.item.unlock || 0) - (a.item.unlock || 0))
  const active = rows.find(row => row.id === selected) ?? rows[0]
  const item = active?.item
  const isHidden = !!item?.hidden && !item.achieved && active?.id !== revealed
  const detailIcon = item?.achieved ? item.icon || item.icongray : item?.icongray || item?.icon
  const hasDate = item?.achieved && item.unlock && !item.unlockEstimated
  const hasSteamRarity = source === "steam" && item && Number.isFinite(rarity(item))
  useEffect(() => {
    if (revealed) selectedRow.current?.focus({ preventScroll: true })
  }, [revealed])

  return (
    <Modal onClose={onClose} title={title ? <><span className="trophy-eyebrow">{t("trofeus.titulo")}</span>{title}</> : t("trofeus.titulo")}
      description={<TrophyStatus summary={summary} />} size="full" className="trophy-browser" gamepad={gamepad} showClose initialFocus="[data-trophy-default]">
      <div className="trophy-shell">
        {tracked && summary?.percent != null && <progress className="trophy-meter" max={100} value={summary.percent} aria-label={t("trofeus.progresso_pessoal")} />}
        {summary?.platinum && <div className="trophy-groups">
          <button type="button" className={control} aria-pressed={group === "base"}
            onClick={() => setGroup(group === "base" ? "all" : "base")}>{t("trofeus.grupo_base", summary.platinum.base)}</button>
          {summary.platinum.dlc.total > 0 && <button type="button" className={control} aria-pressed={group === "dlc"}
            onClick={() => setGroup(group === "dlc" ? "all" : "dlc")}>{t("trofeus.grupo_dlc", summary.platinum.dlc)}</button>}
          {summary.platinum.base.known < summary.platinum.base.total && <span>{t("trofeus.platina_dados_incompletos")}</span>}
          {summary.platinum.earnedAt && <span>{new Date(summary.platinum.earnedAt * 1000).toLocaleDateString(userLocale())}</span>}
        </div>}
        {summary?.stale && <p role="status" className="trophy-warning">{t("trofeus.leitura_falhou")}</p>}
        <div className="trophy-workspace">
          <div className="trophy-toolbar">
            <div className="trophy-filters">
              {filters.map(value => <button key={value} type="button" data-trophy-default={value === "todos" || undefined}
                className={control} disabled={!tracked && value !== "todos"}
                aria-pressed={(tracked ? filter : "todos") === value} onClick={() => { setFilter(value); setRevealed(null) }}>
                {t(`trofeus.${value}`)}
              </button>)}
            </div>
            {hasRarity && <button type="button" className={`${control} trophy-sort`}
              aria-label={`${t("trofeus.ordenar")}: ${t(`trofeus.${sort}`)}`}
              onClick={() => setSort(sort === "recentes" ? "raridade" : "recentes")}>
              <svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round"><path d="M2.5 4h11m-11 4h7m-7 4h3" /></svg>
              {t(`trofeus.${sort}`)}
            </button>}
          </div>
          <div data-gamepad-scroll className="trophy-list">
            {rows.map(({ item: row, id }) => {
              const hidden = row.hidden && !row.achieved && revealed !== id
              const icon = row.achieved ? row.icon || row.icongray : row.icongray || row.icon
              return <button key={id} type="button" ref={id === active?.id ? selectedRow : undefined} aria-pressed={id === active?.id}
                className="trophy-row"
                onFocus={() => setSelected(id)} onClick={() => setSelected(id)}>
                <TrophyArt icon={icon} hidden={hidden} />
                <span className="trophy-row-name">{hidden ? t("trofeus.secreto") : row.title}</span>
                {row.achieved && <span className="trophy-row-check" aria-label={t("console.unlocked")}>✓</span>}
              </button>
            })}
            {!rows.length && <p className="trophy-empty"><>{summary?.items.length ? t("trofeus.sem_resultados") : <TrophyStatus summary={summary} />}</></p>}
          </div>
          {item && <section className="trophy-detail">
            <div className="trophy-detail-head">
              <TrophyArt icon={detailIcon} hidden={isHidden} />
              <div>
                <p className="trophy-detail-state" data-earned={item.achieved || undefined}>{item.achieved ? t("console.unlocked") : tracked ? t("console.locked") : t("trofeus.sem_progresso")}</p>
                <h3>{isHidden ? t("trofeus.secreto") : item.title}</h3>
              </div>
            </div>
            {isHidden ? <button type="button" className={control} onClick={() => setRevealed(active.id)}>{t("trofeus.revelar")}</button> : <>
              {item.desc && <p className="trophy-detail-description">{item.desc}</p>}
              {(hasDate || hasSteamRarity) && <div className="trophy-detail-meta">
                {hasSteamRarity && <p>{t("trofeus.raridade_steam", { percent: new Intl.NumberFormat(userLocale(), { style: "percent", maximumFractionDigits: 2 }).format(rarity(item) / 100) })}</p>}
                {hasDate && <p>{new Date(item.unlock! * 1000).toLocaleDateString(userLocale())}</p>}
              </div>}
            </>}
          </section>}
        </div>
        <div className="trophy-source">
          <div className="trophy-source-actions">
          {!!summary?.items.length && <span>{t("trofeus.itens_exibidos", { count: rows.length, total: summary.items.length })}</span>}
          <button type="button" className={control} aria-expanded={showSource} onClick={() => setShowSource(value => !value)}>
            {t("trofeus.fonte")}
          </button>
          </div>
          {showSource && <div className="trophy-source-details">
            <p>{t(source === "retro" ? "trofeus.fonte_retro" : "trofeus.fonte_steam")}</p>
            {onCapture && <p className="mt-1">{t("trofeus.captura_biblioteca")}</p>}
            {onCapture && <button type="button" aria-disabled={capture === "busy"} className={control} onClick={async () => {
              if (capture === "busy") return
              setCapture("busy")
              try { setCapture(await onCapture() ? "ok" : "error") } catch { setCapture("error") }
            }}>{t("steam_captura.capturar_agora")}</button>}
            {capture && <p role="status">{t(capture === "error" ? "trofeus.leitura_falhou" : "steam_captura.capturando")}</p>}
          </div>}
        </div>
      </div>
    </Modal>
  )
}
