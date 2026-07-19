import { useEffect, useRef, useState } from "react"

interface BootSplashProps {
  /** Chamado ao terminar (~3,5 s) ou ao pular por input. */
  onDone?: () => void
}

// Sequência (ms desde o mount) — bate com as transitions do CSS abaixo.
// Se mudar aqui, mudar lá também: os `transition-duration` estão alinhados
// para o efeito não parecer "cortado".
const T_LINHA_APARECE = 400   // linha branca fininha nasce no centro
const T_LINHA_EXPANDE = 900   // vira uma barra horizontal larga
const T_LOGO_ENTRA = 1500     // "PS5" aparece por dentro da barra
const T_SUB_ENTRA = 2400      // "PLAYSTATION" abaixo
const T_FADE_OUT = 3100
const T_FIM = 3600

export function BootSplash({ onDone }: BootSplashProps) {
  const [fase, setFase] = useState(0) // 0 preto · 1 linha · 2 barra · 3 logo · 4 sub · 5 fade
  const doneRef = useRef(false)

  useEffect(() => {
    const finish = () => {
      if (doneRef.current) return
      doneRef.current = true
      onDone?.()
    }

    // Cronograma. Cada timer só sobe a "fase"; o CSS resolve a transição.
    const timers = [
      setTimeout(() => setFase(1), T_LINHA_APARECE),
      setTimeout(() => setFase(2), T_LINHA_EXPANDE),
      setTimeout(() => setFase(3), T_LOGO_ENTRA),
      setTimeout(() => setFase(4), T_SUB_ENTRA),
      setTimeout(() => setFase(5), T_FADE_OUT),
      setTimeout(finish, T_FIM),
    ]

    // Skip por qualquer input.
    const skip = () => finish()
    window.addEventListener("keydown", skip)
    window.addEventListener("mousedown", skip)
    let gpRaf = 0
    let prev: boolean[] = []
    const gpLoop = () => {
      const pads = navigator.getGamepads ? navigator.getGamepads() : []
      const gp = Array.from(pads).find((x) => x)
      if (gp) {
        if (gp.buttons.some((b, i) => b.pressed && !prev[i])) finish()
        prev = gp.buttons.map((b) => b.pressed)
      }
      if (!doneRef.current) gpRaf = requestAnimationFrame(gpLoop)
    }
    gpRaf = requestAnimationFrame(gpLoop)

    return () => {
      timers.forEach(clearTimeout)
      cancelAnimationFrame(gpRaf)
      window.removeEventListener("keydown", skip)
      window.removeEventListener("mousedown", skip)
    }
  }, [onDone])

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 100,
        background: "#000",
        overflow: "hidden",
        color: "#fff",
      }}
    >
      <style>{css}</style>

      {/* Linha/barra horizontal: fininha, engrossa, some ao aparecer o logo. */}
      <div className="bs-bar" data-fase={fase} />

      {/* Logo "PS5" — aparece por dentro da barra em fase 3. */}
      <div className="bs-logo" data-fase={fase}>
        <svg viewBox="0 0 400 150" xmlns="http://www.w3.org/2000/svg">
          <text
            x="200"
            y="115"
            textAnchor="middle"
            fontFamily='"Helvetica Neue", "Impact", "Haettenschweiler", "Arial Black", sans-serif'
            fontWeight={900}
            fontSize={140}
            letterSpacing={-4}
            fill="#fff"
          >
            PS5
          </text>
        </svg>
      </div>

      {/* Sub-linha espaçada como no boot real. */}
      <div className="bs-sub" data-fase={fase}>PLAYSTATION</div>

      {/* Véu de saída — sobe opacidade nos últimos 500 ms. */}
      <div className="bs-fade" data-fase={fase} />
    </div>
  )
}

// CSS inline: sem depender do host. `data-fase` faz cascata: cada valor
// ativa o próximo estado, e a transition cuida da suavização.
const css = `
.bs-bar {
  position: absolute; left: 50%; top: 50%;
  width: 460px; height: 6px;
  background: #ffffff;
  border-radius: 999px;
  transform: translate(-50%, -50%) scaleX(0) scaleY(0.35);
  opacity: 0;
  box-shadow:
    0 0 24px rgba(180, 220, 255, 0.85),
    0 0 60px rgba(80, 160, 255, 0.55);
  transition:
    transform 700ms cubic-bezier(0.22, 1, 0.36, 1),
    opacity 300ms ease-out;
}
/* fase 1: linha finíssima aparece */
.bs-bar[data-fase="1"] {
  transform: translate(-50%, -50%) scaleX(0.35) scaleY(0.35);
  opacity: 1;
}
/* fase 2: expande horizontal (largura do logo) */
.bs-bar[data-fase="2"] {
  transform: translate(-50%, -50%) scaleX(1) scaleY(1);
  opacity: 1;
}
/* fase 3+: o logo toma o lugar; a barra some com fade curto */
.bs-bar[data-fase="3"],
.bs-bar[data-fase="4"],
.bs-bar[data-fase="5"] {
  transform: translate(-50%, -50%) scaleX(1) scaleY(0.15);
  opacity: 0;
  transition: transform 500ms ease-out, opacity 500ms ease-out;
}

.bs-logo {
  position: absolute; left: 50%; top: 50%;
  transform: translate(-50%, -50%) scale(0.94);
  width: clamp(280px, 34vw, 640px);
  opacity: 0;
  filter: drop-shadow(0 0 18px rgba(180, 220, 255, 0.55));
  transition: opacity 700ms ease-out, transform 900ms cubic-bezier(0.22, 1, 0.36, 1);
}
/* fase 3+: logo entra */
.bs-logo[data-fase="3"],
.bs-logo[data-fase="4"] {
  opacity: 1;
  transform: translate(-50%, -50%) scale(1);
}
/* fase 5: fade final acompanha o véu */
.bs-logo[data-fase="5"] {
  opacity: 0;
  transform: translate(-50%, -50%) scale(1.02);
  transition: opacity 500ms ease-in, transform 700ms ease-out;
}
.bs-logo svg { display: block; width: 100%; height: auto; }

.bs-sub {
  position: absolute; left: 50%; top: 50%;
  transform: translate(-50%, calc(-50% + clamp(110px, 12vw, 220px)));
  font: 400 clamp(11px, 1.05vw, 16px)/1 -apple-system, "Segoe UI", Arial, sans-serif;
  letter-spacing: 0.5em;
  color: rgba(255, 255, 255, 0.85);
  opacity: 0;
  transition: opacity 800ms ease-out;
  /* letter-spacing joga o texto pra esquerda; compensa com padding. */
  padding-left: 0.5em;
}
.bs-sub[data-fase="4"] { opacity: 0.9; }
.bs-sub[data-fase="5"] { opacity: 0; transition-duration: 500ms; }

.bs-fade {
  position: absolute; inset: 0;
  background: #000;
  opacity: 0;
  pointer-events: none;
  transition: opacity 500ms ease-in;
}
.bs-fade[data-fase="5"] { opacity: 1; }
`
