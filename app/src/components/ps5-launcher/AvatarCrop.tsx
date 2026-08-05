"use client"

// Recorte interativo de avatar (estilo Steam/PSN): imagem estática grande é
// exibida com um quadrado 1:1 que o usuário arrasta e redimensiona; o preview
// mostra o resultado final em 256×256; "Aplicar" devolve os bytes recortados.
import { useCallback, useEffect, useRef, useState } from "react"

interface AvatarCropProps {
  src: string // file:// URL da imagem escolhida
  onConfirm: (bytes: Uint8Array, mime: string, ext: string) => void
  onCancel: () => void
  t: (k: string) => string
}

const SAIDA = 256 // lado do avatar final
const MIME = "image/png"

export function AvatarCrop({ src, onConfirm, onCancel, t }: AvatarCropProps) {
  const [img, setImg] = useState<HTMLImageElement | null>(null)
  const [crop, setCrop] = useState({ x: 0, y: 0, size: 0 }) // em px NATURAIS (quadrado)
  const [preview, setPreview] = useState("")
  const [processando, setProcessando] = useState(false)
  const ignorar = useRef(false) // se true, o toBlob callback não dispara (modal fechou)
  const [arrastando, setArrastando] = useState<null | { tipo: "mover" | "redimensionar"; inix: number; iniy: number; orig: { x: number; y: number; size: number } }>(null)

  const imgRef = useRef<HTMLImageElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)

  // Ao desmontar (cancel/fechar), descarta callbacks de toBlob pendentes
  useEffect(() => {
    return () => {
      ignorar.current = true
    }
  }, [])

  // Carrega a imagem e inicializa o quadrado (centralizado, cobrindo o menor
  // lado por inteiro — "recorte certinho" desde o início)
  useEffect(() => {
    const el = new Image()
    el.onload = () => {
      const w = el.naturalWidth
      const h = el.naturalHeight
      const size = Math.min(w, h)
      setImg(el)
      setCrop({ x: Math.round((w - size) / 2), y: Math.round((h - size) / 2), size })
    }
    el.src = src
  }, [src])

  // Desenha o recorte no canvas 256×256 (usado pelo preview e pelo Apply)
  const desenha = useCallback(() => {
    if (!img || !crop.size) return false
    const c = canvasRef.current
    if (!c) return false
    c.width = SAIDA
    c.height = SAIDA
    const ctx = c.getContext("2d")
    if (!ctx) return false
    ctx.clearRect(0, 0, SAIDA, SAIDA)
    ctx.drawImage(img, crop.x, crop.y, crop.size, crop.size, 0, 0, SAIDA, SAIDA)
    return true
  }, [img, crop])

  // Preview ao vivo (256×256) a cada mudança do crop
  useEffect(() => {
    if (!desenha()) return
    const c = canvasRef.current
    if (c) setPreview(c.toDataURL(MIME))
  }, [desenha])

  // Escala: px exibidos → px naturais (a imagem é exibida com aspect preservado)
  const escala = useCallback(() => {
    const el = imgRef.current
    return img && el && el.clientWidth > 0 ? img.naturalWidth / el.clientWidth : 1
  }, [img])

  const clamp = (v: number, min: number, max: number) => Math.min(Math.max(v, min), max)

  const onPointerDown = (e: React.PointerEvent, tipo: "mover" | "redimensionar") => {
    if (!img) return
    e.preventDefault()
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
    setArrastando({ tipo, inix: e.clientX, iniy: e.clientY, orig: { ...crop } })
  }

  const onPointerMove = (e: React.PointerEvent) => {
    if (!arrastando || !img) return
    const s = escala()
    const dx = Math.round((e.clientX - arrastando.inix) * s)
    const dy = Math.round((e.clientY - arrastando.iniy) * s)
    if (arrastando.tipo === "redimensionar") {
      const d = Math.max(dx, dy)
      const size = clamp(arrastando.orig.size + d, 64, Math.min(img.naturalWidth, img.naturalHeight))
      setCrop({
        size,
        x: clamp(arrastando.orig.x, 0, img.naturalWidth - size),
        y: clamp(arrastando.orig.y, 0, img.naturalHeight - size),
      })
    } else {
      setCrop({
        ...arrastando.orig,
        x: clamp(arrastando.orig.x + dx, 0, img.naturalWidth - arrastando.orig.size),
        y: clamp(arrastando.orig.y + dy, 0, img.naturalHeight - arrastando.orig.size),
      })
    }
  }

  const onPointerUp = () => setArrastando(null)

  const aplicar = () => {
    if (processando || !desenha()) return
    const c = canvasRef.current
    if (!c) return
    setProcessando(true)
    c.toBlob((blob) => {
      if (!blob || ignorar.current) return
      blob.arrayBuffer().then((ab) => {
        if (ignorar.current) return
        onConfirm(new Uint8Array(ab), MIME, ".png")
      })
    }, MIME)
  }

  const sx = crop.size ? (crop.x / img?.naturalWidth) * 100 : 0
  const sy = crop.size ? (crop.y / img?.naturalHeight) * 100 : 0
  const ss = crop.size && img ? (crop.size / img.naturalWidth) * 100 : 0

  return (
    <div className="fixed inset-0 z-[120] flex items-center justify-center bg-black/70 p-6 backdrop-blur-sm" onPointerUp={onPointerUp}>
      <div className="w-full max-w-2xl rounded-2xl border border-white/10 bg-[#121218] p-6 shadow-[0_0_60px_rgba(0,168,255,0.15)]">
        <h3 className="mb-1 text-lg font-bold text-white">{t("avatar.crop_titulo")}</h3>
        <p className="mb-4 text-xs text-white/40">{t("avatar.crop_dica")}</p>

        <div className="flex items-center justify-center gap-6">
          {/* Área da imagem com o quadrado de recorte */}
          <div ref={boxRef} className="relative max-h-[420px] max-w-[60%] overflow-hidden rounded-xl bg-black/40">
            <img ref={imgRef} src={src} alt="" draggable={false} className="max-h-[420px] w-auto max-w-full select-none" />
            {crop.size > 0 && (
              <>
                {/* quadrado + borda + alça (o boxShadow gigante escurece o resto) */}
                <div
                  className="absolute cursor-move touch-none"
                  style={{
                    left: `${sx}%`,
                    top: `${sy}%`,
                    width: `${ss}%`,
                    aspectRatio: "1",
                    border: "2px solid #00a8ff",
                    boxShadow: "0 0 0 9999px rgba(0,0,0,0.55), inset 0 0 0 1px rgba(255,255,255,0.15)",
                  }}
                  onPointerDown={(e) => onPointerDown(e, "mover")}
                  onPointerMove={onPointerMove}
                >
                  {/* linhas de terço (regra dos terços — fica "certo") */}
                  <div className="pointer-events-none absolute inset-0 opacity-40">
                    <div className="absolute left-1/3 top-0 h-full w-px bg-white/70" />
                    <div className="absolute left-2/3 top-0 h-full w-px bg-white/70" />
                    <div className="absolute top-1/3 left-0 w-full h-px bg-white/70" />
                    <div className="absolute top-2/3 left-0 w-full h-px bg-white/70" />
                  </div>
                  <div
                    className="absolute -bottom-1.5 -right-1.5 h-5 w-5 cursor-nwse-resize rounded-md border-2 border-white bg-[#00a8ff]"
                    onPointerDown={(e) => {
                      e.stopPropagation()
                      onPointerDown(e, "redimensionar")
                    }}
                    onPointerMove={onPointerMove}
                  />
                </div>
              </>
            )}
            {/* canvas invisível que materializa o recorte (preview + apply) */}
            <canvas ref={canvasRef} className="hidden" width={SAIDA} height={SAIDA} />
          </div>

          {/* Preview 256×256 (círculo — como fica no perfil) */}
          <div className="flex flex-col items-center gap-2">
            <div className="flex h-32 w-32 items-center justify-center overflow-hidden rounded-full border border-white/15 bg-white/[0.04]">
              {preview ? <img src={preview} alt="" className="h-full w-full object-cover" /> : <div className="text-[10px] text-white/30">…</div>}
            </div>
            <span className="text-[10px] uppercase tracking-widest text-white/30">{SAIDA}×{SAIDA}</span>
          </div>
        </div>

        <div className="mt-5 flex justify-end gap-3">
          <button onClick={onCancel} disabled={processando} className="rounded-lg border border-white/10 px-4 py-2 text-sm text-white/60 transition-colors hover:bg-white/5 hover:text-white disabled:opacity-30">
            {t("avatar.crop_cancelar")}
          </button>
          <button
            onClick={aplicar}
            disabled={processando}
            className="rounded-lg bg-gradient-to-r from-[#0072ce] to-[#00a8ff] px-5 py-2 text-sm font-semibold text-white transition-all hover:brightness-110 disabled:opacity-50"
          >
            {processando ? t("avatar.crop_processando") : t("avatar.crop_aplicar")}
          </button>
        </div>
      </div>
    </div>
  )
}
