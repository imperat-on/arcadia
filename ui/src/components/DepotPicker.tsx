"use client"

import { useMemo, useState } from "react"
import { useI18n } from "../i18n/I18nContext"
import type { DepotInfo, EscolhaDisco } from "./useStoreActions"
import { fmtGiB } from "./tamanho"

// Seleciona quais depots baixar: Base + DLCs + Idiomas + OS.
// Sem isso, downloadmanager enfileira TODOS os depots do app
// (Cyberpunk = 47 depots = ~296 GiB em vez de ~66 GiB).
//
// Na tela, o que o usuário decide (idioma, DLC, arquivos base) aparece com nome
// legível; o jargão (depotId, OS, código cru do idioma) desce para o detalhe.

type TipoGrupo = "base" | "idioma" | "dlc" | "sem_meta"
type Grupo = { id: string; tipo: TipoGrupo; titulo: string; os?: string; depots: DepotInfo[] }
type Tradutor = (key: string, vars?: Record<string, string | number>) => string

// Steam usa códigos próprios de idioma no depot ("polish", "brazilian"). O nome
// vem do i18n; código sem tradução cai no fallback capitalizado.
function nomeIdioma(code: string, t: Tradutor): string {
  const chave = `depot.lang.${code}`
  const nome = t(chave)
  return nome === chave ? code.charAt(0).toUpperCase() + code.slice(1) : nome
}

function tituloDepot(d: DepotInfo, t: Tradutor): string {
  if (d.language) return nomeIdioma(d.language, t)
  return d.name || t("depot.depot", { id: d.depotId })
}

function agrupar(depots: DepotInfo[], t: Tradutor): Grupo[] {
  const semMeta = depots.every((d) => !d.os && !d.language && !d.dlcAppid)
  if (semMeta) return [{ id: "sem_meta", tipo: "sem_meta", titulo: t("depot.sem_metadata"), depots }]

  // Só o que pertence ao jogo: base + DLC + idioma. Ignora shared/runtimes
  // (sharedinstall=1 tipo Steamworks Redistributables), depots sem classificação
  // e macOS (Arcadia roda no Linux/Proton — depots Mac são peso morto).
  const relevantes = depots.filter(
    (d) => !d.shared && d.os !== "macos" && (d.dlcAppid || d.language || d.os),
  )

  const buckets = new Map<string, Grupo>()
  const push = (tipo: TipoGrupo, titulo: string, d: DepotInfo) => {
    const id = `${tipo}|${titulo}|${d.os || ""}`
    const atual = buckets.get(id)
    if (atual) atual.depots.push(d)
    else buckets.set(id, { id, tipo, titulo, os: d.os, depots: [d] })
  }

  for (const d of relevantes) {
    if (d.dlcAppid) push("dlc", d.name || t("depot.dlc_sem_nome", { id: d.dlcAppid }), d)
    else if (d.language) push("idioma", nomeIdioma(d.language, t), d)
    else push("base", t("depot.grupo_base"), d)
  }

  // Hierarquia fixa na tela: base primeiro, idiomas depois, DLCs por último.
  const ordem: TipoGrupo[] = ["base", "idioma", "dlc"]
  return [...buckets.values()].sort((a, b) => ordem.indexOf(a.tipo) - ordem.indexOf(b.tipo))
}

function sizeGiB(depots: DepotInfo[]): number {
  return depots.reduce((a, d) => a + (Number(d.size) || 0), 0) / 1024 ** 3
}

// Idioma que o usuário fala, na nomenclatura da Steam. pt-BR tenta o depot
// brasileiro antes do português de Portugal; sem correspondência, inglês.
const IDIOMAS_APP: Record<string, string[]> = {
  "pt-BR": ["brazilian", "portuguese"],
  "es-ES": ["spanish"],
  "en-US": ["english"],
}

function idiomaPreferido(depots: DepotInfo[], lang: string): string {
  for (const c of IDIOMAS_APP[lang] || []) {
    if (depots.some((d) => d.language === c)) return c
  }
  return "english"
}

// Essencial: base do OS atual (Windows, via Proton). Sem DLC e sem idiomas.
function essencial(depots: DepotInfo[]): Set<string> {
  const os = "windows"
  const sel = new Set<string>()
  for (const d of depots) {
    if (d.dlcAppid || d.language) continue
    if (!d.os || d.os === os) sel.add(d.depotId)
  }
  return sel
}

// Seleção padrão: essencial + idioma do usuário (inglês se o jogo não tiver).
function padrao(depots: DepotInfo[], lang: string): Set<string> {
  const os = "windows" // arcadia usa Proton — Windows depots
  const idiomaPref = idiomaPreferido(depots, lang)
  const sel = essencial(depots)
  for (const d of depots) {
    if (d.language === idiomaPref && (!d.os || d.os === os)) sel.add(d.depotId)
  }
  return sel
}

// Ação rápida do topo da lista.
function Acao({ onClick, children }: { onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      className="rounded-full border border-white/10 px-2.5 py-1 text-[11px] font-medium text-white/60 transition-colors hover:border-[color:var(--accent)]/50 hover:text-white"
    >
      {children}
    </button>
  )
}

// Checkbox próprio: o ☑/☐ de texto não combinava com o resto do app.
function Checkbox({ marcado, parcial }: { marcado: boolean; parcial?: boolean }) {
  return (
    <span
      className={`grid h-[18px] w-[18px] shrink-0 place-items-center rounded-[5px] border transition-colors ${
        marcado
          ? "border-[color:var(--accent)] bg-[color:var(--accent)] text-black"
          : parcial
            ? "border-[color:var(--accent)]/60 bg-[color:var(--accent)]/15 text-[color:var(--accent)]"
            : "border-white/20 bg-white/[0.03] text-transparent"
      }`}
    >
      <svg
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {parcial && !marcado ? <path d="M6 12h12" /> : <path d="m5 12.5 4.5 4.5L19 7.5" />}
      </svg>
    </span>
  )
}

// Linha de um depot. `solto` é quando ela não está dentro de um grupo (idioma
// ou DLC costumam ter um depot só, aí a própria linha é o grupo).
function LinhaDepot({
  d,
  marcado,
  onToggle,
  solto,
}: {
  d: DepotInfo
  marcado: boolean
  onToggle: () => void
  solto?: boolean
}) {
  const { t } = useI18n()
  return (
    <button
      onClick={onToggle}
      title={`${d.depotId}${d.language ? ` · ${d.language}` : ""}`}
      className={`group flex w-full items-center gap-3 rounded-xl border px-3 py-2.5 text-left transition-colors ${
        solto
          ? "border-white/10 bg-[color:var(--surface-2)] hover:border-[color:var(--accent)]/40 hover:bg-[color:var(--surface-3)]"
          : "border-transparent hover:bg-white/[0.04]"
      }`}
    >
      <Checkbox marcado={marcado} />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate text-[12.5px] font-medium text-white/90">
            {tituloDepot(d, t)}
          </span>
          {d.os && (
            <span className="shrink-0 rounded bg-white/[0.06] px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wide text-white/45">
              {d.os.toUpperCase()}
            </span>
          )}
        </span>
        <span className="mt-0.5 block truncate text-[10.5px] text-white/40">{d.depotId}</span>
      </span>
      <span className="shrink-0 text-[11px] font-semibold text-white/55">
        {fmtGiB((Number(d.size) || 0) / 1024 ** 3)}
      </span>
    </button>
  )
}

// Grupo com mais de um depot: o cabeçalho marca/desmarca o grupo inteiro.
function CartaoGrupo({
  g,
  sel,
  onToggleGrupo,
  onToggleDepot,
}: {
  g: Grupo
  sel: Set<string>
  onToggleGrupo: (g: Grupo) => void
  onToggleDepot: (id: string) => void
}) {
  const marcados = g.depots.filter((d) => sel.has(d.depotId)).length
  const todos = marcados === g.depots.length
  return (
    <div className="overflow-hidden rounded-xl border border-white/10 bg-[color:var(--surface-2)]">
      <button
        onClick={() => onToggleGrupo(g)}
        className="flex w-full items-center gap-3 border-b border-white/[0.06] px-3 py-2.5 text-left transition-colors hover:bg-white/[0.04]"
      >
        <Checkbox marcado={todos} parcial={marcados > 0 && !todos} />
        <span className="min-w-0 flex-1 truncate text-[12.5px] font-semibold text-white/90">
          {g.titulo}
        </span>
        {g.os && (
          <span className="shrink-0 rounded bg-white/[0.06] px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wide text-white/45">
            {g.os.toUpperCase()}
          </span>
        )}
        <span className="shrink-0 text-[11px] font-semibold text-white/55">
          {fmtGiB(sizeGiB(g.depots))}
        </span>
      </button>
      <div className="flex flex-col gap-0.5 p-1.5">
        {g.depots.map((d) => (
          <LinhaDepot
            key={d.depotId}
            d={d}
            marcado={sel.has(d.depotId)}
            onToggle={() => onToggleDepot(d.depotId)}
          />
        ))}
      </div>
    </div>
  )
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
  const { t, lang } = useI18n()
  const [sel, setSel] = useState<Set<string>>(() => padrao(depots, lang))
  const grupos = useMemo(() => agrupar(depots, t), [depots, t])
  const total = useMemo(() => sizeGiB(depots.filter((d) => sel.has(d.depotId))), [depots, sel])

  const toggleDepot = (id: string) => {
    setSel((prev) => {
      const n = new Set(prev)
      if (n.has(id)) n.delete(id)
      else n.add(id)
      return n
    })
  }
  const toggleGrupo = (g: Grupo) => {
    setSel((prev) => {
      const n = new Set(prev)
      const todosMarcados = g.depots.every((d) => n.has(d.depotId))
      for (const d of g.depots) {
        if (todosMarcados) n.delete(d.depotId)
        else n.add(d.depotId)
      }
      return n
    })
  }

  const escolhidos = depots.filter((d) => sel.has(d.depotId))

  // Seções na ordem da hierarquia: base -> idiomas -> DLCs.
  const secoes = useMemo(() => {
    const ordem: TipoGrupo[] = ["base", "idioma", "dlc", "sem_meta"]
    const mapa = new Map<TipoGrupo, Grupo[]>()
    for (const g of grupos) {
      if (!mapa.has(g.tipo)) mapa.set(g.tipo, [])
      mapa.get(g.tipo)!.push(g)
    }
    return ordem.filter((tp) => mapa.has(tp)).map((tp) => ({ tipo: tp, grupos: mapa.get(tp)! }))
  }, [grupos])

  const tituloSecao = (tp: TipoGrupo) =>
    tp === "base"
      ? t("depot.grupo_base")
      : tp === "idioma"
        ? t("depot.grupo_idiomas")
        : t("depot.grupo_dlcs")

  return (
    <div className="max-h-[60vh] overflow-y-auto pr-1">
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <span className="mr-auto text-[12px] text-white/60">
          {t("depot.resumo", {
            size: fmtGiB(total),
            sel: String(escolhidos.length),
            total: String(depots.length),
          })}
        </span>
        <Acao onClick={() => setSel(padrao(depots, lang))}>{t("depot.acao_recomendado")}</Acao>
        <Acao onClick={() => setSel(essencial(depots))}>{t("depot.acao_essencial")}</Acao>
        <Acao onClick={() => setSel(new Set<string>())}>{t("depot.acao_limpar")}</Acao>
      </div>
      <div className="mb-4 flex flex-col gap-3">
        {secoes.map((s) => (
          <div key={s.tipo}>
            {s.tipo !== "sem_meta" && (
              <div className="mb-1.5 flex items-center gap-2">
                <span className="text-[10px] font-bold uppercase tracking-[0.14em] text-white/35">
                  {tituloSecao(s.tipo)}
                </span>
                {s.tipo === "base" && (
                  <span className="rounded-full border border-[color:var(--accent)]/40 bg-[color:var(--accent)]/10 px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wide text-[color:var(--accent)]">
                    {t("depot.essencial")}
                  </span>
                )}
              </div>
            )}
            <div className="flex flex-col gap-1.5">
              {s.grupos.map((g) =>
                g.depots.length === 1 ? (
                  <LinhaDepot
                    key={g.id}
                    d={g.depots[0]}
                    marcado={sel.has(g.depots[0].depotId)}
                    onToggle={() => toggleDepot(g.depots[0].depotId)}
                    solto
                  />
                ) : (
                  <CartaoGrupo
                    key={g.id}
                    g={g}
                    sel={sel}
                    onToggleGrupo={toggleGrupo}
                    onToggleDepot={toggleDepot}
                  />
                ),
              )}
            </div>
          </div>
        ))}
      </div>
      {extras}
      <div className="mt-3 flex justify-end gap-2">
        <button
          onClick={onCancel}
          className="rounded-lg border border-white/15 px-4 py-1.5 text-[12px] text-white/70 hover:bg-white/[0.06] hover:text-white"
        >
          {t("common.cancelar")}
        </button>
        <button
          disabled={!escolhidos.length}
          onClick={() => onConfirm(escolhidos)}
          className="rounded-lg px-4 py-1.5 text-[12px] font-semibold text-black disabled:opacity-50"
          style={{ background: "var(--accent)" }}
        >
          {t("depot.baixar", { total: fmtGiB(total) })}
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
  const { t } = useI18n()
  const [lib, setLib] = useState<string>(escolhendo.libs[0]?.steamDir || "")
  return (
    <div data-no-drag
      className="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onClick={onCancel}
    >
      <div data-no-drag
        className="w-[560px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[color:var(--surface-1)] p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="mb-3 text-base font-semibold text-white">{titulo}</h3>
        <div className="mb-3 flex flex-col gap-1.5">
          {escolhendo.libs.map((l) => (
            <button
              key={l.steamDir}
              onClick={() => setLib(l.steamDir)}
              className={`flex items-center justify-between rounded-lg border px-3 py-2 text-left text-[12px] ${lib === l.steamDir ? "border-[color:var(--accent)] bg-[color-mix(in_srgb,var(--accent)_12%,transparent)]" : "border-white/10 hover:border-white/25"}`}
            >
              <span className="text-white/90">{l.steamDir.replace(/^\/home\/[^/]+/, "~")}</span>
              <span className="text-[11px] text-white/50">{t("store.gib_livres", { free: l.free.toFixed(2) })}</span>
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
