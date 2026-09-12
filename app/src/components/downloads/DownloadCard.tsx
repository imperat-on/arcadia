"use client"

import { useEffect, useState } from "react"
import { fmtBytes, fmtMiB } from "../tamanho"
import { useI18n } from "../../i18n/I18nContext"
import { actionsFor, kindKey, statusKey } from "./normalize"
import type { DownloadActionType, DownloadKind, DownloadVM } from "./normalize"

// Card único de download — desktop e console renderizam ESTE componente.
//
// Antes existiam dois cards com linguagens diferentes (DmCard para a fila
// Epic/Steam e TorrentCard para o P2P), o que causava a tela se contradizer.
// Aqui a aparência é única e as ações são despachadas por `kind`: itens da
// fila usam dm*, itens torrent usam torrent*. NUNCA cruzam — o dm não sabe
// pausar um torrent e vice-versa.

/** Despacho das ações — mesmos IPC de sempre, roteados pela origem. */
function runAction(kind: DownloadKind, type: DownloadActionType, id: string) {
  const api = window.launcherAPI
  if (!api) return
  const daFila = kind === "legendary" || kind === "steam"
  if (daFila) {
    if (type === "pause") api.dmPause(id)
    else if (type === "resume") api.dmResume(id)
    else if (type === "cancel") api.dmCancel(id)
    else if (type === "retry") api.dmRetry(id)
    else if (type === "dismiss") api.dmDismiss(id)
  } else {
    if (type === "pause") api.torrentPause(id)
    else if (type === "resume" || type === "retry") api.torrentResume(id)
    else if (type === "cancel" || type === "dismiss") api.torrentCancel(id)
  }
}

const ROTULO: Record<DownloadActionType, string> = {
  pause: "downloads.pausar",
  resume: "downloads.retomar",
  cancel: "common.cancelar",
  retry: "downloads.tentar_novamente",
  dismiss: "common.remover",
}

function statusTone(status: DownloadVM["status"]): string {
  if (status === "error") return "#ff6b81"
  if (status === "active") return "var(--accent)"
  if (status === "done") return "rgba(255,255,255,0.75)"
  return "rgba(255,255,255,0.55)"
}

export function DownloadCard({ item: vm }: { item: DownloadVM }) {
  const { t } = useI18n()
  // Marca o card assim que o botão é apertado. O back-end remove o item em
  // ~20ms, mas se a rede ou o disco atrasarem a resposta, sem isto o botão
  // parece morto — foi o que motivou esta correção.
  const [cancelando, setCancelando] = useState(false)
  const daFila = vm.kind === "legendary" || vm.kind === "steam"
  // A URL salva no item pode falhar (404, CDN offline). Sem fallback a tela
  // mostra o ícone de imagem quebrada em vez do placeholder.
  const appidSteam = daFila && /^steam:/i.test(vm.id) ? vm.id.replace(/^steam:/i, "") : ""
  const fallbackSteam = appidSteam
    ? `https://cdn.cloudflare.steamstatic.com/steam/apps/${appidSteam}/library_600x900.jpg`
    : ""
  const [coverSrc, setCoverSrc] = useState(vm.cover)
  const [coverQuebrou, setCoverQuebrou] = useState(false)
  useEffect(() => {
    setCoverSrc(vm.cover)
    setCoverQuebrou(false)
    setCancelando(false)
  }, [vm.cover, vm.id, vm.status])

  const baixando = vm.status === "active"
  const acoes = actionsFor(vm.kind, vm.status)

  // Linha de métrica da esquerda (progresso) — formatação por origem:
  // a fila reporta MiB; torrent/HTTP reportam bytes.
  const metricas =
    daFila
      ? vm.dmTotalMiB > 0
        ? t("downloads.progresso", {
            done: fmtMiB(vm.dmDoneMiB),
            total: fmtMiB(vm.dmTotalMiB),
            pct: String(vm.percent),
          })
        : vm.status === "queued"
          ? t("downloads.aguardando")
          : `${vm.percent}%`
      : vm.status === "caching"
        ? t("torrent.cacheando_desc")
        : `${fmtBytes(vm.bytesDone)} / ${fmtBytes(vm.bytesTotal)} · ${vm.percent}%`

  // Linha da direita (vivo): velocidade, ETA e peers quando existirem.
  const vivos: string[] = []
  if (baixando) {
    if (vm.speedBps > 0) vivos.push(`${fmtBytes(vm.speedBps)}/s`)
    if (vm.eta) vivos.push(t("downloads.eta", { eta: vm.eta }))
    if (vm.kind === "torrent") vivos.push(t("torrent.peers", { count: String(vm.peers) }))
  }

  return (
    <div
      className={`retro-download-card flex items-center gap-5 rounded-2xl border p-4 transition-all ${
        baixando
          ? "border-[color:var(--accent)]/40 bg-[color:var(--accent)]/[0.04]"
          : "border-white/10 bg-white/[0.03] hover:border-white/20"
      }`}
      style={baixando ? { boxShadow: "0 0 30px -8px var(--accent)" } : undefined}
    >
      {/* Capa com halo */}
      <div className="relative shrink-0">
        {coverSrc && !coverQuebrou ? (
          <img
            src={coverSrc}
            alt=""
            className="h-24 w-16 shrink-0 rounded-lg object-cover ring-1 ring-white/10"
            draggable={false}
            onError={() => {
              if (fallbackSteam && coverSrc !== fallbackSteam) setCoverSrc(fallbackSteam)
              else setCoverQuebrou(true)
            }}
          />
        ) : (
          <div className="flex h-24 w-16 shrink-0 items-center justify-center rounded-lg bg-white/5 ring-1 ring-white/10">
            <svg
              width="22"
              height="22"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              strokeLinejoin="round"
              className="text-white/40"
            >
              <path d="M12 3v12" />
              <path d="m7 11 5 5 5-5" />
              <path d="M5 21h14" />
            </svg>
          </div>
        )}
        {baixando && (
          <div
            className="pointer-events-none absolute -inset-1.5 -z-10 rounded-xl opacity-40 blur-lg"
            style={{ background: "color-mix(in oklab, var(--accent) 50%, transparent)" }}
          />
        )}
      </div>

      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-4">
          <h3 className="truncate text-base font-semibold text-white/95">{vm.title}</h3>
          <span
            className="shrink-0 text-xs"
            style={{ color: statusTone(vm.status), fontWeight: baixando ? 600 : 400 }}
          >
            {t(statusKey(vm.status))}
          </span>
        </div>

        {/* Barra de progresso — indeterminada em cacheando, vermelha em erro */}
        <div className="mt-3 h-2 w-full overflow-hidden rounded-full bg-white/[0.07]">
          {vm.status === "caching" ? (
            <div
              className="h-full w-1/4 rounded-full"
              style={{
                background: "var(--accent)",
                boxShadow: "0 0 14px var(--accent)",
                animation: "dm-indeterminate 1.4s ease-in-out infinite",
              }}
            />
          ) : (
            <div
              className="relative h-full rounded-full transition-all duration-500"
              style={{
                width: `${vm.percent}%`,
                background: vm.status === "error" ? "#ff6b81" : "var(--accent)",
                boxShadow: baixando ? "0 0 14px var(--accent)" : "none",
              }}
            >
              <div
                className="absolute inset-0 overflow-hidden rounded-full"
                style={{
                  background:
                    "linear-gradient(90deg, transparent 0%, rgba(255,255,255,0.35) 50%, transparent 100%)",
                  backgroundSize: "200% 100%",
                  animation: baixando ? "dm-shine 1.8s linear infinite" : "none",
                }}
              />
            </div>
          )}
        </div>

        <div className="mt-2 flex items-baseline justify-between gap-3 text-xs text-white/50">
          <span className="flex min-w-0 items-baseline gap-2">
            <span className="shrink-0 rounded bg-white/[0.07] px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wider text-white/55">
              {t(kindKey(vm.kind))}
            </span>
            <span className="tabular-nums truncate">{metricas}</span>
          </span>
          <span className="shrink-0 tabular-nums text-white/70">{vivos.join(" · ")}</span>
        </div>

        {/* Erro é informação de primeira classe, não nota de rodapé */}
        {vm.status === "error" && (
          <div className="mt-1.5 truncate text-[11px] text-[#ff6b81]">
            {vm.error || t("downloads.falhou")}
          </div>
        )}
      </div>

      {/* Ações — mesmos handlers, roteados pela origem do item */}
      <div className="flex shrink-0 flex-col gap-2">
        {acoes.map((tipo) => (
          <button
            key={tipo}
            onClick={() => {
              if (tipo === "cancel" || tipo === "dismiss") setCancelando(true)
              runAction(vm.kind, tipo, vm.id)
            }}
            className={`rounded-full px-4 py-1.5 text-xs font-semibold outline-none transition-all focus-visible:shadow-[0_0_0_2px_var(--accent)] ${
              tipo === "resume" || tipo === "retry"
                ? "text-black hover:scale-105"
                : tipo === "cancel"
                  ? "border border-[#ff6b81]/40 text-[#ff6b81] hover:border-[#ff6b81]/70 hover:bg-[#ff6b81]/10"
                  : "border border-white/15 text-white/80 hover:border-white/30 hover:bg-white/10"
            }`}
            style={
              tipo === "resume" || tipo === "retry"
                ? { background: "var(--accent)" }
                : undefined
            }
          >
            {tipo === "cancel" && cancelando ? t("downloads.cancelando") : t(ROTULO[tipo])}
          </button>
        ))}
      </div>
    </div>
  )
}
