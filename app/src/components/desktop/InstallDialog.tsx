"use client"

import { useEffect, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import { fmtGiB } from "../tamanho"

// Diálogo de instalação (estilo Heroic): mostra tamanho do download/instalado,
// deixa escolher a pasta, exibe espaço livre e confirma/cancela a instalação.

// Recebe GiB e desce para MiB quando o jogo é pequeno — "0.78 GiB" é tão
// ruim quanto "61440 MiB" no card de download.
function GiB({ v }: { v?: number }) {
  return <>{fmtGiB(v)}</>
}

export function InstallDialog({
  game,
  onClose,
}: {
  game: Game
  onClose: (instalou: boolean) => void
}) {
  const [installPath, setInstallPath] = useState("")
  const [disk, setDisk] = useState<{ free?: number; total?: number }>({})
  const [busy, setBusy] = useState(false)

  // Pasta padrão: config.default_install_path ou ~/Games/Arcadia.
  useEffect(() => {
    window.launcherAPI?.getConfig().then((c) => {
      const padrao = c?.default_install_path || `${window.launcherPaths?.home || "~"}/Games/Arcadia`
      setInstallPath(padrao)
    })
  }, [])

  // Espaço livre sempre que a pasta muda.
  useEffect(() => {
    if (!installPath) return
    window.launcherAPI?.diskSpace(installPath).then((r) => {
      if (r?.ok) setDisk({ free: r.free, total: r.total })
    })
  }, [installPath])

  const escolher = async () => {
    const r = await window.launcherAPI?.pickFolder()
    if (r?.ok && r.path) setInstallPath(r.path)
  }

  const instalar = async () => {
    setBusy(true)
    await window.launcherAPI?.dmInstall({
      appid: game.id,
      title: game.title,
      cover: game.cover,
      installPath,
    })
    setBusy(false)
    onClose(true)
  }

  const tamanhoGb = game.size ? game.size / 1024 : undefined // size vem em MiB

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onClick={() => !busy && onClose(false)}
    >
      <div
        className="w-[520px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-6 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Título */}
        <div className="mb-5 flex items-start justify-between">
          <h2 className="text-xl font-light tracking-wide text-white">{game.title}</h2>
          <button
            onClick={() => !busy && onClose(false)}
            className="rounded-md p-1 text-white/40 transition-colors hover:bg-white/[0.06] hover:text-white"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        {/* Tamanhos */}
        <div className="mb-5 flex gap-8">
          <div className="flex items-center gap-2.5">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="7 10 12 15 17 10" /><line x1="12" x2="12" y1="15" y2="3" />
            </svg>
            <div>
              <div className="text-[11px] text-white/45">Tamanho do download</div>
              <div className="text-sm font-medium text-white"><GiB v={tamanhoGb} /></div>
            </div>
          </div>
          <div className="flex items-center gap-2.5">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <rect x="2" y="3" width="20" height="7" rx="2" /><rect x="2" y="14" width="20" height="7" rx="2" />
            </svg>
            <div>
              <div className="text-[11px] text-white/45">Tamanho instalado</div>
              <div className="text-sm font-medium text-white"><GiB v={tamanhoGb} /></div>
            </div>
          </div>
        </div>

        {/* Local */}
        <label className="mb-1.5 block text-[13px] text-white/70">Selecione o local de instalação</label>
        <div className="mb-2 flex gap-2">
          <input
            value={installPath}
            onChange={(e) => setInstallPath(e.target.value)}
            spellCheck={false}
            className="flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors focus:border-[color:var(--accent)]"
          />
          <button
            onClick={escolher}
            title="Escolher pasta"
            className="rounded-lg border border-white/10 bg-white/[0.05] px-3 text-white/70 transition-colors hover:bg-white/10 hover:text-white"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
            </svg>
          </button>
        </div>

        {/* Espaço em disco */}
        <p className="mb-6 text-[12px] text-white/45">
          Espaço restante no dispositivo:{" "}
          <span className="font-semibold text-white/80">
            <GiB v={disk.free} /> / <GiB v={disk.total} />
          </span>
          {tamanhoGb != null && disk.free != null && (
            <>
              {" "}— Pós-instalação:{" "}
              <span className="font-semibold text-white/80">
                <GiB v={Math.max(0, disk.free - tamanhoGb)} />
              </span>
            </>
          )}
        </p>

        {/* Ações */}
        <div className="flex justify-end gap-2.5">
          <button
            onClick={() => !busy && onClose(false)}
            disabled={busy}
            className="rounded-lg border border-white/15 px-5 py-2 text-[13px] font-medium text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white disabled:opacity-50"
          >
            Cancelar
          </button>
          <button
            onClick={instalar}
            disabled={busy || !installPath}
            className="rounded-lg px-5 py-2 text-[13px] font-semibold text-black transition-transform hover:scale-[1.03] disabled:opacity-60"
            style={{ background: "var(--accent)" }}
          >
            {busy ? "Iniciando…" : "INSTALAR"}
          </button>
        </div>
      </div>
    </div>
  )
}
