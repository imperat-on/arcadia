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
  const [arrastando, setArrastando] = useState<null | { tipo: "mover" | "redimensionar"; inix: number; iniy: number; orig: { x: number; y: number; size: number } }>(null)

  const imgRef = useRef<HTMLImageElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)

  // Carrega a imagem e inicializa o quadrado (centralizado, 80% do menor lado)
  useEffect(() => {
    const el = new Image()
    el.onload = () => {
      const w = el.naturalWidth
      const h = el.naturalHeight
      const size = Math.round(Math.min(w, h) * 0.8)
      setImg(el)
      setCrop({ x: Math.round((w - size) / 2), y: Math.round((h - size) / 2), size })
    }
    el.src = src
  }, [src])

  // Preview ao vivo (256×256) a cada mudança do crop
  useEffect(() => {
    if (!img || !crop.size) return
    const c = canvasRef.current
    if (!c) return
    c.width = SAIDA
    c.height = SAIDA
    const ctx = c.getContext("2d")
    if (!ctx) return
    ctx.clearRect(0, 0, SAIDA, SAIDA)
    ctx.drawImage(img, crop.x, crop.y, crop.size, crop.size, 0, 0, SAIDA, SAIDA)
    setPreview(c.toDataURL(MIME))
  }, [img, crop])

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
    const c = canvasRef.current
    if (!c) return
    c.toBlob((blob) => {
      if (!blob) return
      blob.arrayBuffer().then((ab) => onConfirm(new Uint8Array(ab), MIME, ".png"))
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
                  className="absolute cursor-move touch-none border-2 border-[#00a8ff]"
                  style={{ left: `${sx}%`, top: `${sy}%`, width: `${ss}%`, aspectRatio: "1", boxShadow: "0 0 0 9999px rgba(0,0,0,0.55)" }}
                  onPointerDown={(e) => onPointerDown(e, "mover")}
                  onPointerMove={onPointerMove}
                >
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
          </div>

          {/* Preview 256×256 */}
          <div className="flex flex-col items-center gap-2">
            <div className="flex h-32 w-32 items-center justify-center overflow-hidden rounded-full border border-white/15 bg-white/[0.04]">
              {preview ? <img src={preview} alt="" className="h-full w-full object-cover" /> : <div className="text-[10px] text-white/30">…</div>}
            </div>
            <span className="text-[10px] uppercase tracking-widest text-white/30">{SAIDA}×{SAIDA}</span>
          </div>
        </div>

        <div className="mt-5 flex justify-end gap-3">
          <button onClick={onCancel} className="rounded-lg border border-white/10 px-4 py-2 text-sm text-white/60 transition-colors hover:bg-white/5 hover:text-white">
            {t("avatar.crop_cancelar")}
          </button>
          <button
            onClick={aplicar}
            className="rounded-lg bg-gradient-to-r from-[#0072ce] to-[#00a8ff] px-5 py-2 text-sm font-semibold text-white transition-all hover:brightness-110"
          >
            {t("avatar.crop_aplicar")}
          </button>
        </div>
      </div>
    </div>
  )
}
