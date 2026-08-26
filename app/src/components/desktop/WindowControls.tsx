"use client"

// Controles de janela no estilo macOS. A faixa continua arrastável e os
// símbolos aparecem apenas ao passar o mouse sobre o conjunto.
export function WindowControls() {
  const api = typeof window !== "undefined" ? window.launcherAPI : undefined
  if (!api?.winClose) return null

  return (
    <div
      className="fixed left-0 top-0 z-[100] flex h-8 items-center px-3"
      style={{ WebkitAppRegion: "drag" } as React.CSSProperties}
    >
      <div className="group flex items-center gap-2" style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}>
        <Dot color="#ff5f57" ring="#e0443e" label="Fechar" onClick={() => api.winClose?.()}>
          <path d="M4.2 4.2l3.6 3.6M7.8 4.2l-3.6 3.6" />
        </Dot>
        <Dot color="#febc2e" ring="#dea123" label="Minimizar" onClick={() => api.winMinimize?.()}>
          <path d="M3.8 6h4.4" />
        </Dot>
        <Dot color="#28c840" ring="#1aab29" label="Maximizar" onClick={() => api.winMaximize?.()}>
          <path d="M4.3 4.3h3.4v3.4z" fill="currentColor" stroke="none" />
        </Dot>
      </div>
    </div>
  )
}

function Dot({ color, ring, children, onClick, label }: {
  color: string
  ring: string
  children: React.ReactNode
  onClick: () => void
  label: string
}) {
  return (
    <button
      onClick={onClick}
      aria-label={label}
      title={label}
      className="flex h-3 w-3 items-center justify-center rounded-full transition-transform active:scale-90"
      style={{ background: color, boxShadow: `inset 0 0 0 0.5px ${ring}` }}
    >
      <svg
        width="12" height="12" viewBox="0 0 12 12" fill="none"
        stroke="rgba(0,0,0,0.55)" strokeWidth="1.1" strokeLinecap="round" strokeLinejoin="round"
        className="opacity-0 transition-opacity group-hover:opacity-100"
      >
        {children}
      </svg>
    </button>
  )
}
