"use client"

import { forwardRef, useEffect, useState } from "react"
import type { DmItem } from "../../global"

interface DownloadManagerProps {
  onClose: () => void
}

const STATUS_TXT: Record<DmItem["status"], string> = {
  queued: "Na fila",
  downloading: "Baixando",
  paused: "Pausado",
  done: "Concluído",
  error: "Erro",
  canceled: "Cancelado",
}

// Tela de downloads estilo PS5: um card por jogo, barra azul-glow, MB/s e ETA.
export const DownloadManager = forwardRef<HTMLDivElement, DownloadManagerProps>(function DownloadManager(
  { onClose },
  ref,
) {
  const [items, setItems] = useState<DmItem[]>([])

  useEffect(() => {
    window.launcherAPI?.dmQueue().then((q) => {
      if (Array.isArray(q)) setItems(q)
    })
    return window.launcherAPI?.onDmProgress((q) => {
      if (Array.isArray(q)) setItems(q)
    })
  }, [])

  // Ativos e não concluídos em seções separadas. O desktop recebeu isso no
  // commit 717a793 e esta tela ficou para trás: os cards vinham misturados sob
  // "Baixando agora", com o contador dizendo "N ativo(s)" ao lado de itens em
  // erro — a tela se contradizia.
  const ativos = items.filter((i) => ["downloading", "queued", "paused"].includes(i.status))
  const parados = items.filter((i) => !["downloading", "queued", "paused"].includes(i.status))
  const baixando = ativos.some((i) => i.status === "downloading")

  return (
    <div ref={ref} className="gp-scope fixed inset-0 z-50 overflow-y-auto bg-black/95 text-white antialiased backdrop-blur-xl">
      <div className="mx-auto max-w-[1100px] px-10 py-8">
        {/* Cabeçalho */}
        <div className="mb-2 flex items-center gap-2 text-[11px] font-medium uppercase tracking-[0.24em] text-white/50">
          <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--accent)" }} />
          Downloads
        </div>
        <div className="mb-8 flex items-baseline justify-between">
          <h1 className="text-3xl font-light tracking-wide">
            {baixando ? "Baixando agora" : ativos.length ? "Na fila" : "Fila de downloads"}
          </h1>
          <span className="text-sm text-white/40">
            {ativos.length} ativo(s)
            {parados.length > 0 && ` · ${parados.length} com falha`}
          </span>
        </div>

        {items.length === 0 ? (
          <div className="flex min-h-[300px] items-center justify-center text-white/35">
            Nenhum download.
          </div>
        ) : (
          <div className="flex flex-col gap-4 pb-10">
            {ativos.map((it) => (
              <DmCard key={it.appid} item={it} />
            ))}
            {parados.length > 0 && (
              <>
                <h2 className="mt-4 text-sm font-medium text-white/45">Não concluídos</h2>
                {parados.map((it) => (
                  <DmCard key={it.appid} item={it} />
                ))}
              </>
            )}
          </div>
        )}

        <div className="flex items-center justify-end gap-6 pb-2 text-xs text-white/40">
          <button onClick={onClose} className="outline-none transition-colors hover:text-white/70 focus-visible:text-[color:var(--accent)]">
            B — voltar
          </button>
        </div>
      </div>
    </div>
  )
})

export function DmCard({ item: it }: { item: DmItem }) {
  // Marca o card assim que o botão é apertado. O back-end remove o item em
  // ~20ms, mas se a rede ou o disco atrasarem a resposta, sem isto o botão
  // parece morto — foi o que motivou esta correção.
  const [cancelando, setCancelando] = useState(false)
  const baixando = it.status === "downloading"
  const pausado = it.status === "paused"
  const ativo = baixando || pausado || it.status === "queued"
  const pct = Math.round(it.percent)

  return (
    <div
      className={`flex items-center gap-5 rounded-2xl border p-4 transition-colors ${
        baixando ? "border-[color:var(--accent)]" : "border-white/10"
      } bg-white/[0.03]`}
      style={baixando ? { boxShadow: "0 0 30px -8px var(--accent)" } : undefined}
    >
      {it.cover ? (
        <img src={it.cover} alt="" className="h-20 w-14 shrink-0 rounded-lg object-cover ring-1 ring-white/10" draggable={false} />
      ) : (
        <div className="h-20 w-14 shrink-0 rounded-lg bg-white/5 ring-1 ring-white/10" />
      )}

      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-4">
          <h3 className="truncate text-base font-medium">{it.title}</h3>
          <span className="shrink-0 text-xs text-white/50">{STATUS_TXT[it.status]}</span>
        </div>

        {/* Barra de progresso azul-glow */}
        <div className="mt-2.5 h-1.5 w-full overflow-hidden rounded-full bg-white/10">
          <div
            className="h-full rounded-full transition-all duration-500"
            style={{
              width: `${pct}%`,
              background: "var(--accent)",
              boxShadow: baixando ? "0 0 12px var(--accent)" : "none",
            }}
          />
        </div>

        <div className="mt-2 flex items-baseline justify-between text-xs text-white/50">
          <span className="tabular-nums">
            {it.total > 0
              ? `${it.done.toFixed(0)} / ${it.total.toFixed(0)} MiB · ${pct}%`
              : it.done > 0
                ? `${it.done.toFixed(0)} MiB baixados · ${pct}%`
                : it.status === "queued" ? "aguardando…" : `${pct}%`}
          </span>
          {baixando && (
            <span className="tabular-nums text-white/70">
              {it.speed > 0 ? `${it.speed.toFixed(1)} MB/s` : ""} {it.eta ? `· ETA ${it.eta}` : ""}
            </span>
          )}
          {it.status === "error" && <span className="text-[#ff6b81]">{it.error || "falhou"}</span>}
        </div>
      </div>

      {/* Ações */}
      <div className="flex shrink-0 flex-col gap-2">
        {baixando && (
          <Acao label="Pausar" onClick={() => window.launcherAPI?.dmPause(it.appid)} />
        )}
        {pausado && (
          <Acao label="Retomar" primaria onClick={() => window.launcherAPI?.dmResume(it.appid)} />
        )}
        {ativo && (
          <Acao
            label={cancelando ? "Cancelando…" : "Cancelar"}
            perigo
            onClick={() => {
              setCancelando(true)
              window.launcherAPI?.dmCancel(it.appid)
            }}
          />
        )}
        {/* Item que falhou não tinha ação nenhuma: ficava preso na tela para
            sempre, e mandar baixar de novo criava um card duplicado. */}
        {it.status === "error" && (
          <>
            <Acao label="Tentar de novo" primaria onClick={() => window.launcherAPI?.dmRetry(it.appid)} />
            <Acao label="Remover" onClick={() => window.launcherAPI?.dmDismiss(it.appid)} />
          </>
        )}
      </div>
    </div>
  )
}

function Acao({ label, onClick, primaria, perigo }: { label: string; onClick: () => void; primaria?: boolean; perigo?: boolean }) {
  return (
    <button
      onClick={onClick}
      className={`rounded-full px-4 py-1.5 text-xs font-semibold outline-none transition-all focus-visible:shadow-[0_0_0_2px_var(--accent)] ${
        primaria
          ? "bg-white text-black hover:scale-105"
          : perigo
            ? "border border-[#ff6b81]/40 text-[#ff6b81] hover:bg-[#ff6b81]/10"
            : "border border-white/15 text-white/80 hover:bg-white/10"
      }`}
    >
      {label}
    </button>
  )
}
