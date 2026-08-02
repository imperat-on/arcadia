"use client"

import { useMemo, useState } from "react"
import type { DepotInfo, EscolhaDisco } from "./useStoreActions"
import { fmtGiB } from "./tamanho"

// Seleciona quais depots baixar: Base + DLCs + Idiomas + OS.
// Sem isso, downloadmanager enfileira TODOS os depots do app
// (Cyberpunk = 47 depots = ~296 GiB em vez de ~66 GiB).

type Grupo = { titulo: string; depots: DepotInfo[] }

function agrupar(depots: DepotInfo[]): Grupo[] {
  const semMeta = depots.every((d) => !d.os && !d.language && !d.dlcAppid)
  if (semMeta) return [{ titulo: "Depots (sem metadata — restart do app pode ajudar)", depots }]

  // Só o que pertence ao jogo: base + DLC + idioma. Ignora shared/runtimes
  // (sharedinstall=1 tipo Steamworks Redistributables), depots sem classificação
  // e macOS (Arcadia roda no Linux/Proton — depots Mac são peso morto).
  const relevantes = depots.filter((d) => !d.shared && d.os !== "macos" && (d.dlcAppid || d.language || d.os))

  const buckets = new Map<string, DepotInfo[]>()
  const push = (k: string, d: DepotInfo) => {
    if (!buckets.has(k)) buckets.set(k, [])
    buckets.get(k)!.push(d)
  }
  const rotuloOs = (os: string) => (os ? os.toUpperCase() : "SEM OS")

  for (const d of relevantes) {
    const os = rotuloOs(d.os || "")
    if (d.dlcAppid) push(`DLC · ${d.name || d.dlcAppid} · ${os}`, d)
    else if (d.language) push(`Idioma: ${d.language} · ${os}`, d)
    else push(`Base · ${os}`, d)
  }
  return [...buckets.entries()].map(([titulo, ds]) => ({ titulo, depots: ds }))
}

function labelDepot(d: DepotInfo): string {
  const partes: string[] = [`${d.depotId} —`]
  if (d.os) partes.push(`[${d.os.toUpperCase()}]`)
  partes.push(d.name || `depot ${d.depotId}`)
  if (d.language) partes.push(`(${d.language})`)
  return partes.join(" ")
}

function sizeGiB(depots: DepotInfo[]): number {
  return depots.reduce((a, d) => a + (Number(d.size) || 0), 0) / (1024 ** 3)
}

// Seleção padrão: base do OS atual + inglês. Sem DLC.
function padrao(depots: DepotInfo[]): Set<string> {
  const os = "windows" // arcadia usa Proton — Windows depots
  const idiomaPref = "english"
  const sel = new Set<string>()
  for (const d of depots) {
    if (d.dlcAppid) continue
    if (d.language) {
      if (d.language === idiomaPref && (!d.os || d.os === os)) sel.add(d.depotId)
      continue
    }
    if (!d.os || d.os === os) sel.add(d.depotId)
  }
  return sel
}

export function DepotPicker({
  depots,
  onConfirm,
  onCancel,
  extras,
}: {
  depots: DepotInfo[]
  onConfirm: (sel: DepotInfo[]) => void
  onCancel: () => void
  extras?: React.ReactNode
}) {
  const [sel, setSel] = useState<Set<string>>(() => padrao(depots))
  const grupos = useMemo(() => agrupar(depots), [depots])
  const total = useMemo(() => sizeGiB(depots.filter((d) => sel.has(d.depotId))), [depots, sel])

  const toggleDepot = (id: string) => {
    setSel((prev) => {
      const n = new Set(prev)
      if (n.has(id)) n.delete(id); else n.add(id)
      return n
    })
  }
  const toggleGrupo = (g: Grupo) => {
    setSel((prev) => {
      const n = new Set(prev)
      const todosMarcados = g.depots.every((d) => n.has(d.depotId))
      for (const d of g.depots) {
        if (todosMarcados) n.delete(d.depotId); else n.add(d.depotId)
      }
      return n
    })
  }

  const escolhidos = depots.filter((d) => sel.has(d.depotId))

  return (
    <div className="max-h-[60vh] overflow-y-auto pr-1">
      <div className="mb-3 flex items-center justify-between text-[12px] text-white/60">
        <span>Total selecionado</span>
        <span className="font-semibold text-white">{fmtGiB(total)}</span>
      </div>
      <div className="mb-4 flex flex-col gap-2">
        {grupos.map((g) => {
          const gsize = sizeGiB(g.depots)
          const marcadosG = g.depots.filter((d) => sel.has(d.depotId)).length
          return (
            <div key={g.titulo} className="rounded-xl border border-white/10 bg-white/[0.03] p-2.5">
              <button
                onClick={() => toggleGrupo(g)}
                className="mb-1 flex w-full items-center justify-between text-left text-[13px] text-white/90"
              >
                <span className="font-medium">
                  <span className="mr-2">{marcadosG === g.depots.length ? "☑" : marcadosG > 0 ? "◪" : "☐"}</span>
                  {g.titulo}
                </span>
                <span className="text-[11px] text-white/45">{fmtGiB(gsize)}</span>
              </button>
              {(
                <div className="ml-6 flex flex-col gap-0.5">
                  {g.depots.map((d) => (
                    <button
                      key={d.depotId}
                      onClick={() => toggleDepot(d.depotId)}
                      className="flex items-center justify-between text-left text-[11px] text-white/60 hover:text-white/90"
                    >
                      <span>
                        <span className="mr-2">{sel.has(d.depotId) ? "☑" : "☐"}</span>
                        {labelDepot(d)}
                      </span>
                      <span className="text-white/35">{fmtGiB((Number(d.size) || 0) / (1024 ** 3))}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          )
        })}
      </div>
      {extras}
      <div className="mt-3 flex justify-end gap-2">
        <button
          onClick={onCancel}
          className="rounded-lg border border-white/15 px-4 py-1.5 text-[12px] text-white/70 hover:bg-white/[0.06] hover:text-white"
        >
          Cancelar
        </button>
        <button
          disabled={!escolhidos.length}
          onClick={() => onConfirm(escolhidos)}
          className="rounded-lg px-4 py-1.5 text-[12px] font-semibold text-black disabled:opacity-50"
          style={{ background: "var(--accent)" }}
        >
          Baixar {fmtGiB(total)}
        </button>
      </div>
    </div>
  )
}

// Dialog completo: escolhe biblioteca Steam + depots num só passo.
// Substitui o antigo popup de "escolher biblioteca" que baixava tudo.
export function EscolhaDownloadDialog({
  escolhendo,
  onCancel,
  onConfirm,
  titulo,
}: {
  escolhendo: EscolhaDisco
  onCancel: () => void
  onConfirm: (steamDir: string, sel: DepotInfo[]) => void
  titulo: string
}) {
  const [lib, setLib] = useState<string>(escolhendo.libs[0]?.steamDir || "")
  return (
    <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm" onClick={onCancel}>
      <div className="w-[560px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-5 shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <h3 className="mb-3 text-base font-semibold text-white">{titulo}</h3>
        <div className="mb-3 flex flex-col gap-1.5">
          {escolhendo.libs.map((l) => (
            <button
              key={l.steamDir}
              onClick={() => setLib(l.steamDir)}
              className={`flex items-center justify-between rounded-lg border px-3 py-2 text-left text-[12px] ${lib === l.steamDir ? "border-[color:var(--accent)] bg-[color-mix(in_srgb,var(--accent)_12%,transparent)]" : "border-white/10 hover:border-white/25"}`}
            >
              <span className="text-white/90">{l.steamDir.replace(/^\/home\/[^/]+/, "~")}</span>
              <span className="text-[11px] text-white/50">{l.free.toFixed(2)} GiB livres</span>
            </button>
          ))}
        </div>
        <DepotPicker
          depots={escolhendo.info.depots}
          onCancel={onCancel}
          onConfirm={(sel) => onConfirm(lib, sel)}
        />
      </div>
    </div>
  )
}
