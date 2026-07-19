"use client"

import { useEffect, useMemo, useState } from "react"
import type { WineVer, ArtCandidate } from "../../global"
import type { Game } from "../ps5-launcher/types"

// Diálogo "Adicionar jogo" (estilo Heroic): adiciona um jogo/app manualmente
// à biblioteca — Windows (via Wine) ou Linux nativo. Salvo em custom_games.json.

function slug(t: string) {
  return t.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "")
}

export function AddGameDialog({ onClose, onAdded, editGame }: { onClose: () => void; onAdded: () => void; editGame?: Game | null }) {
  const editando = Boolean(editGame)
  const custom = !editGame || editGame.launcher === "custom" // seções de Wine/exe
  const [titulo, setTitulo] = useState(editGame?.title || "")
  const [descricao, setDescricao] = useState(editGame?.description || "")
  const [platform, setPlatform] = useState<"windows" | "linux">(editGame?.platform === "linux" ? "linux" : "windows")
  const [exe, setExe] = useState(editGame?.exe || "")
  const [prefix, setPrefix] = useState("")
  const [prefixPadrao, setPrefixPadrao] = useState("")
  const [wineVersion, setWineVersion] = useState("")
  const [wines, setWines] = useState<WineVer[]>([])
  const [erro, setErro] = useState("")
  const [busy, setBusy] = useState(false)
  // Busca automática de capa pelo título (SteamGridDB/fontes ligadas).
  const [candidatas, setCandidatas] = useState<ArtCandidate[]>([])
  const [buscandoArte, setBuscandoArte] = useState(false)
  const [capaEscolhida, setCapaEscolhida] = useState("")

  // Editando: o id é o do próprio jogo (preserva configs/arte). Novo: do slug.
  const id = useMemo(
    () => editGame?.id || `custom:${slug(titulo) || "jogo"}`,
    [titulo, editGame],
  )

  // Debounce: 700ms após parar de digitar, busca capas e mostra previews.
  useEffect(() => {
    const q = titulo.trim()
    setCandidatas([])
    setCapaEscolhida("")
    if (q.length < 3) return
    setBuscandoArte(true)
    const t = setTimeout(() => {
      window.launcherAPI
        ?.searchArt(id, q, "cover")
        .then((r) => setCandidatas((r?.candidatos || []).slice(0, 4)))
        .finally(() => setBuscandoArte(false))
    }, 700)
    return () => clearTimeout(t)
  }, [titulo, id])

  useEffect(() => {
    window.launcherAPI?.wineList().then((r) => setWines(r?.installed || []))
  }, [])
  useEffect(() => {
    window.launcherAPI?.gameSettingsGet(id).then((r) => {
      setPrefixPadrao(r?.defaultPrefix || "")
      // Editando: preenche prefixo e versão do Wine já configurados.
      if (editando) {
        setPrefix(r?.settings?.prefixPath || "")
        setWineVersion(r?.settings?.wineVersion || "")
      }
    })
  }, [id])

  const wineEscolhido = wines.find((w) => w.id === wineVersion)?.wine
  const prefixoEfetivo = prefix || prefixPadrao

  const pickExe = async () => {
    const r = await window.launcherAPI?.pickFile()
    if (r?.ok && r.path) setExe(r.path)
  }
  const pickPrefix = async () => {
    const r = await window.launcherAPI?.pickFolder()
    if (r?.ok && r.path) setPrefix(r.path)
  }

  const rodarInstalador = async () => {
    setBusy(true)
    await window.launcherAPI?.customGameRunInstaller({ appid: id, wine: wineEscolhido, prefix: prefixoEfetivo || undefined })
    setBusy(false)
  }

  const terminar = async () => {
    setErro("")
    if (!titulo.trim()) return setErro("Informe o título do jogo/app.")
    if (custom && !exe) return setErro("Selecione o executável.")
    setBusy(true)
    if (editando && !custom) {
      // Jogo de loja (Steam/Epic/etc.): salva título/descrição via overrides.
      await window.launcherAPI?.setOverride(id, {
        title: titulo.trim(),
        description: descricao || null,
      })
      setBusy(false)
      onAdded()
      onClose()
      return
    }
    // Salva prefixo/versão do Wine nas configurações do jogo (usadas no launch).
    await window.launcherAPI?.gameSettingsSet(id, {
      prefixPath: prefix || undefined,
      wineVersion: wineVersion || undefined,
    })
    const r = editando
      ? await window.launcherAPI?.customGameUpdate({ id, title: titulo.trim(), exe })
      : await window.launcherAPI?.customGameAdd({ id, title: titulo.trim(), platform, exe })
    setBusy(false)
    if (!r?.ok) return setErro(r?.error || (editando ? "Falha ao salvar" : "Falha ao adicionar"))
    onAdded()
    onClose()
  }

  const ArteBtn = ({ kind, label }: { kind: "cover" | "hero" | "logo"; label: string }) => (
    <button
      onClick={async () => {
        const r = await window.launcherAPI?.pickArt(id, kind)
        if (r?.ok && r.path) await window.launcherAPI?.setOverride(id, { [kind]: r.path })
      }}
      disabled={!titulo.trim()}
      className="rounded-lg border border-white/15 px-3 py-2 text-[12px] text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white disabled:opacity-40"
    >
      {label}
    </button>
  )

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm" onClick={onClose}>
      <div
        className="flex max-h-[88vh] w-[560px] max-w-[94vw] flex-col rounded-2xl border border-white/[0.08] bg-[#0d0d10] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between px-6 pt-5">
          <h2 className="text-lg font-light tracking-wide text-white">{editando ? `Editar ${editGame?.title}` : "Título do jogo/app"}</h2>
          <button onClick={onClose} className="rounded-md p-1 text-white/40 transition-colors hover:bg-white/[0.06] hover:text-white">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          <input
            value={titulo}
            onChange={(e) => setTitulo(e.target.value)}
            placeholder="Título"
            spellCheck={false}
            className="mb-4 w-full rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/30 focus:border-[color:var(--accent)]"
          />

          {/* Imagens */}
          <label className="mb-1.5 block text-[12px] text-white/60">Imagens (capa, fundo, logo)</label>
          <div className="mb-2 flex gap-2">
            <ArteBtn kind="cover" label="Capa" />
            <ArteBtn kind="hero" label="Fundo" />
            <ArteBtn kind="logo" label="Logo" />
          </div>
          {/* Preview da busca automática de capa */}
          {buscandoArte && <p className="mb-3 text-[12px] text-white/40">Buscando capas…</p>}
          {!buscandoArte && candidatas.length > 0 && (
            <div className="mb-4 flex gap-2.5">
              {candidatas.map((c) => (
                <button
                  key={c.url}
                  onClick={async () => {
                    const r = await window.launcherAPI?.downloadArt(id, "cover", c.url)
                    if (r?.ok && r.path) {
                      await window.launcherAPI?.setOverride(id, { cover: r.path })
                      setCapaEscolhida(c.url)
                    }
                  }}
                  title={`${c.fonte} · ${c.largura}x${c.altura}`}
                  className={`relative aspect-[2/3] w-[72px] overflow-hidden rounded-lg border-2 transition-all hover:scale-[1.04] ${
                    capaEscolhida === c.url ? "border-[color:var(--accent)]" : "border-transparent"
                  }`}
                >
                  <img src={c.thumb || c.url} alt="" className="h-full w-full object-cover" loading="lazy" draggable={false} />
                  {capaEscolhida === c.url && (
                    <span className="absolute inset-0 flex items-center justify-center bg-black/40">
                      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                        <polyline points="20 6 9 17 4 12" />
                      </svg>
                    </span>
                  )}
                </button>
              ))}
            </div>
          )}

          {custom ? (
            <>
          <label className="mb-1.5 block text-[12px] text-white/60">Selecione a versão da plataforma para instalar:</label>
          <select
            value={platform}
            onChange={(e) => setPlatform(e.target.value as "windows" | "linux")}
            className="mb-4 w-full appearance-none rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none focus:border-[color:var(--accent)]"
          >
            <option value="windows" className="bg-[#16161a]">Windows</option>
            <option value="linux" className="bg-[#16161a]">Linux (nativo)</option>
          </select>

          {platform === "windows" && (
            <>
              <label className="mb-1.5 block text-[12px] text-white/60">Prefixo Wine (WinePrefix)</label>
              <div className="mb-4 flex gap-2">
                <input
                  value={prefixoEfetivo}
                  onChange={(e) => setPrefix(e.target.value)}
                  spellCheck={false}
                  className="flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors focus:border-[color:var(--accent)]"
                />
                <button onClick={pickPrefix} title="Escolher pasta" className="rounded-lg border border-white/10 bg-white/[0.05] px-3 text-white/70 transition-colors hover:bg-white/10 hover:text-white">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                  </svg>
                </button>
              </div>

              <label className="mb-1.5 block text-[12px] text-white/60">Versão do Wine:</label>
              <div className="relative mb-4">
                <svg className="absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-white/50" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M8 22h8M12 15v7M7 3h10l-1 7a4 4 0 0 1-8 0L7 3z" />
                </svg>
                <select
                  value={wineVersion}
                  onChange={(e) => setWineVersion(e.target.value)}
                  className="w-full appearance-none rounded-lg border border-white/10 bg-white/[0.04] py-2.5 pl-10 pr-9 text-[13px] text-white outline-none focus:border-[color:var(--accent)]"
                >
                  <option value="" className="bg-[#16161a]">Padrão do sistema</option>
                  {wines.map((w) => (
                    <option key={w.id} value={w.id} className="bg-[#16161a]">{w.name}</option>
                  ))}
                </select>
                <svg className="pointer-events-none absolute right-3 top-1/2 h-4 w-4 -translate-y-1/2 text-white/40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </div>
            </>
          )}

          <label className="mb-1.5 block text-[12px] text-white/60">Selecionar executável</label>
          <div className="mb-2 flex gap-2">
            <button
              onClick={pickExe}
              className={`flex flex-1 items-center justify-between rounded-lg border px-3.5 py-2.5 text-left text-[13px] transition-colors ${
                exe ? "border-white/15 text-white/80" : "border-white/10 text-white/35"
              } bg-white/[0.04] hover:border-white/25`}
            >
              <span className="truncate">{exe || "Selecionar executável"}</span>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className="shrink-0 text-white/50">
                <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
              </svg>
            </button>
          </div>
            </>
          ) : (
            <>
              <label className="mb-1.5 block text-[12px] text-white/60">Descrição</label>
              <textarea
                value={descricao}
                onChange={(e) => setDescricao(e.target.value)}
                rows={6}
                spellCheck={false}
                placeholder="Descrição do jogo"
                className="mb-2 w-full resize-y rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/30 focus:border-[color:var(--accent)]"
              />
            </>
          )}
          {erro && <p className="mb-2 text-[12px] text-[#ff6b81]">{erro}</p>}
        </div>

        <div className="flex justify-end gap-2.5 border-t border-white/[0.06] px-6 py-4">
          {custom && platform === "windows" && (
            <button
              onClick={rodarInstalador}
              disabled={busy}
              className="rounded-lg px-5 py-2.5 text-[12px] font-bold tracking-wide text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
              style={{ background: "var(--accent)" }}
            >
              EXECUTAR INSTALADOR ANTES
            </button>
          )}
          <button
            onClick={terminar}
            disabled={busy}
            className="rounded-lg border border-white/20 px-5 py-2.5 text-[12px] font-semibold tracking-wide text-white/80 transition-colors hover:bg-white/[0.06] hover:text-white disabled:opacity-50"
          >
            TERMINAR
          </button>
        </div>
      </div>
    </div>
  )
}
