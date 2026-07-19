import { useState } from "react"
import { BootSplash } from "./BootSplash"

export function App() {
  const [booting, setBooting] = useState(true)

  return (
    <div style={{ position: "fixed", inset: 0, background: "#000", color: "#fff" }}>
      {booting ? (
        <BootSplash onDone={() => setBooting(false)} />
      ) : (
        <div
          style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 12,
            textAlign: "center",
            fontWeight: 500,
            letterSpacing: "0.02em",
          }}
        >
          <div style={{ fontSize: 28 }}>Boot terminado.</div>
          <div style={{ opacity: 0.6, fontSize: 15 }}>
            Recarregue (F5) para ver de novo.
          </div>
          <button
            onClick={() => setBooting(true)}
            style={{
              marginTop: 16,
              padding: "10px 24px",
              borderRadius: 999,
              border: "1px solid rgba(255,255,255,0.2)",
              background: "rgba(255,255,255,0.06)",
              color: "#fff",
              fontSize: 14,
              cursor: "pointer",
            }}
          >
            Rebobinar
          </button>
        </div>
      )}
    </div>
  )
}
