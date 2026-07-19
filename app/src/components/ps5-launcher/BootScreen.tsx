"use client"

// Tela de boot: toca um vídeo de abertura (estilo PS5) em tela cheia.
// O arquivo fica em ~/.local/share/arcadia/boot.mp4 (ou .webm).
// `saindo` dispara o fade de saída.
export function BootScreen({
  src,
  saindo,
  onEnded,
  onError,
}: {
  src: string
  saindo: boolean
  onEnded: () => void
  onError: () => void
}) {
  return (
    <div className={`fixed inset-0 z-[80] bg-black ${saindo ? "boot-out" : ""}`}>
      <video
        src={src}
        autoPlay
        playsInline
        onEnded={onEnded}
        onError={onError}
        className="h-full w-full object-cover"
      />
    </div>
  )
}
