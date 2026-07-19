export function FooterBar() {
  return (
    <footer
      className="anim-nav flex items-center justify-between px-10 py-3"
      style={{ borderTop: "1px solid rgba(255,255,255,0.05)" }}
    >
      {/* Left hints */}
      <div className="flex items-center gap-5 text-xs text-[#7a8aaa]">
        <GamepadHint button="←→" label="Navegar" />
        <GamepadHint button="✕" label="Iniciar" color="#7ba4d9" />
        <GamepadHint button="○" label="Voltar" color="#d97b7b" />
        <GamepadHint button="START" label="Atualizar" small />
      </div>

      {/* Right — status */}
      <div className="flex items-center gap-4 text-xs text-[#7a8aaa]">
        <div className="flex items-center gap-1.5">
          <div className="w-1.5 h-1.5 rounded-full bg-emerald-400" />
          <span>Online</span>
        </div>
      </div>
    </footer>
  )
}

function GamepadHint({
  button,
  label,
  color = "#7a8aaa",
  small = false,
}: {
  button: string
  label: string
  color?: string
  small?: boolean
}) {
  return (
    <div className="flex items-center gap-1.5">
      <span
        className="inline-flex items-center justify-center rounded font-bold text-center"
        style={{
          color,
          border: `1px solid ${color}44`,
          background: `${color}15`,
          padding: small ? "1px 5px" : "2px 6px",
          fontSize: small ? 9 : 10,
          minWidth: 20,
          height: 18,
        }}
      >
        {button}
      </span>
      <span>{label}</span>
    </div>
  )
}
