"use client"

// Selo da loja no canto do card (estilo o chip "EPIC GAMES" do Heroic).

const CORES: Record<string, string> = {
  steam: "#66c0f4",
  epic: "#9b6bff",
  heroic: "#f9a020",
  lutris: "#ff9f1c",
  psn: "#4aa3ff",
  slssteam: "#9b6bff",
}

export function StoreBadge({ launcher }: { launcher: string }) {
  const nome = launcher === "heroic" ? "HEROIC" : launcher.toUpperCase()
  return (
    <span
      className="absolute right-2 top-2 rounded-md px-1.5 py-0.5 text-[9px] font-black uppercase tracking-wide text-white"
      style={{ background: "rgba(0,0,0,0.75)", border: `1px solid ${CORES[launcher] || "#888"}` }}
    >
      {nome}
    </span>
  )
}
