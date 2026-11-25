"use client"

import { useEffect, useRef, useState } from "react"
import { useI18n } from "../i18n/I18nContext"

// Diagnóstico ao vivo: o ponto do analógico esquerdo, o anel da zona morta
// configurada e os botões acendendo ao pressionar. Serve para achar drift (o
// ponto que não volta ao centro sozinho) e conferir o mapeamento.
//
// O laço só re-renderiza quando algo muda de verdade — um setState por frame a
// 60fps deixaria a aba inteira trabalhando à toa enquanto ninguém mexe no
// controle.

// Índices do mapeamento padrão (W3C): 0-3 face, 4/5 ombros, 12-15 D-pad.
const BOTOES: { i: number; rotulo: string }[] = [
  { i: 0, rotulo: "A" },
  { i: 1, rotulo: "B" },
  { i: 2, rotulo: "X" },
  { i: 3, rotulo: "Y" },
  { i: 4, rotulo: "L1" },
  { i: 5, rotulo: "R1" },
  { i: 12, rotulo: "↑" },
  { i: 13, rotulo: "↓" },
  { i: 14, rotulo: "←" },
  { i: 15, rotulo: "→" },
]

const LADO = 132 // área do analógico, em px
const MEIO = LADO / 2
const RAIO = MEIO - 12 // raio útil: o ponto nunca encosta na borda

export function GamepadDiagnostics({ deadzone }: { deadzone: number }) {
  const { t } = useI18n()
  const [conectado, setConectado] = useState(false)
  const [eixo, setEixo] = useState({ x: 0, y: 0 })
  const [apertados, setApertados] = useState<number[]>([])
  const deadzoneRef = useRef(deadzone)
  deadzoneRef.current = deadzone

  useEffect(() => {
    let raf = 0
    const laco = () => {
      const gp = navigator.getGamepads?.().find((g) => g && g.connected) ?? null
      setConectado((antes) => (antes === !!gp ? antes : !!gp))
      if (gp) {
        const x = gp.axes[0] ?? 0
        const y = gp.axes[1] ?? 0
        setEixo((a) => (Math.abs(a.x - x) > 0.01 || Math.abs(a.y - y) > 0.01 ? { x, y } : a))
        const agora: number[] = []
        gp.buttons.forEach((b, i) => {
          if (b.pressed || b.value > 0.5) agora.push(i)
        })
        setApertados((antes) =>
          antes.length === agora.length && agora.every((i) => antes.includes(i)) ? antes : agora,
        )
      }
      raf = requestAnimationFrame(laco)
    }
    raf = requestAnimationFrame(laco)
    return () => cancelAnimationFrame(raf)
  }, [])

  if (!conectado) {
    return (
      <p className="text-xs leading-relaxed text-[color:var(--text-3)]">
        {t("controller.nenhum_conectado_desc")}
      </p>
    )
  }

  const [eixoX, eixoY] = [eixo.x, eixo.y]

  return (
    <div className="flex flex-wrap items-center gap-5">
      <div
        className="relative shrink-0 rounded-2xl border border-white/10 bg-[color:var(--surface-1)]"
        style={{ width: LADO, height: LADO }}
        aria-hidden="true"
      >
        {/* Cruz do centro, para conferir o repouso. */}
        <span className="absolute top-1/2 left-3 right-3 h-px bg-white/10" />
        <span className="absolute left-1/2 top-3 bottom-3 w-px bg-white/10" />
        {/* Anel da zona morta: abaixo deste raio o analógico é considerado parado. */}
        <span
          className="absolute rounded-full border border-dashed border-[color:var(--accent)] opacity-50"
          style={{
            left: MEIO - RAIO * deadzoneRef.current,
            top: MEIO - RAIO * deadzoneRef.current,
            width: RAIO * deadzoneRef.current * 2,
            height: RAIO * deadzoneRef.current * 2,
          }}
        />
        <span
          className="absolute rounded-full bg-[color:var(--accent)] transition-transform duration-75"
          style={{
            width: 12,
            height: 12,
            left: MEIO - 6 + eixoX * RAIO,
            top: MEIO - 6 + eixoY * RAIO,
          }}
        />
      </div>

      <div className="flex flex-wrap gap-1.5">
        {BOTOES.map(({ i, rotulo }) => {
          const ativo = apertados.includes(i)
          return (
            <span
              key={i}
              className={`flex h-8 min-w-8 items-center justify-center rounded-lg border px-2 text-xs font-medium tabular-nums ${
                ativo
                  ? "border-[color:var(--accent)] bg-[color:var(--accent)] text-[color:var(--surface-0)]"
                  : "border-white/10 text-[color:var(--text-3)]"
              }`}
            >
              {rotulo}
            </span>
          )
        })}
      </div>
    </div>
  )
}
