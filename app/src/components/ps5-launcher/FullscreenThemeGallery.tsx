"use client"

import { useEffect, useState, useCallback, useRef } from "react"
import { useFullscreenTheme } from "../themes/FullscreenThemeContext"

type ThemeInfo = {
  id: string
  manifest: {
    name: string
    author: string
    version: string
    description: string
    previews: string[]
    license: string
    compat: string
  } | null
  source: "builtin" | "local"
  installed: boolean
  valid: boolean
  error: string
  state: string
  options: Record<string, boolean | number | string>
  active: boolean
}

interface FullscreenThemeGalleryProps {
  onClose: () => void
}

export function FullscreenThemeGallery({ onClose }: FullscreenThemeGalleryProps) {
  const { state: themeState, activate } = useFullscreenTheme()
  const [themes, setThemes] = useState<ThemeInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [importing, setImporting] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)

  const loadThemes = useCallback(async () => {
    try {
      setLoading(true)
      const list = await window.launcherAPI?.fullscreenThemesList?.()
      if (list) setThemes(list as ThemeInfo[])
      setError(null)
    } catch {
      setError("Falha ao carregar temas")
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    loadThemes()
  }, [loadThemes])

  // Escuta mudanças
  useEffect(() => {
    const off = window.launcherAPI?.onFullscreenThemesChanged?.(() => {
      loadThemes()
    })
    return () => { off?.() }
  }, [loadThemes])

  const handleActivate = useCallback(async (id: string) => {
    try {
      const ok = await activate(id)
      if (!ok) {
        setError(themeState.error || "Falha ao ativar tema")
      }
    } catch {
      setError("Falha ao ativar tema")
    }
  }, [activate, themeState.error])

  const handleImport = useCallback(async () => {
    try {
      setImporting(true)
      const result = await window.launcherAPI?.fullscreenThemesImport?.()
      if (result?.ok) {
        await loadThemes()
      } else if (result?.error && result.error !== "cancelado") {
        setError(result.error)
      }
    } catch {
      setError("Falha ao importar tema")
    } finally {
      setImporting(false)
    }
  }, [loadThemes])

  const handleRemove = useCallback(async (id: string) => {
    try {
      const result = await window.launcherAPI?.fullscreenThemesRemove?.(id)
      if (result?.ok) {
        await loadThemes()
      } else {
        setError(result?.error || "Falha ao remover tema")
      }
    } catch {
      setError("Falha ao remover tema")
    }
  }, [loadThemes])

  const handleRecover = useCallback(async () => {
    try {
      await window.launcherAPI?.fullscreenThemesRecover?.()
      await loadThemes()
    } catch {
      setError("Falha ao recuperar tema")
    }
  }, [loadThemes])

  return (
    <div
      ref={rootRef}
      data-theme-slot="settings.theme-gallery"
      className="retro-theme-gallery gp-scope fixed inset-0 z-50 flex flex-col bg-black/95 text-white backdrop-blur-xl"
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-white/10 px-8 py-5">
        <div>
          <h2 className="font-display text-lg font-bold uppercase tracking-wider">Temas</h2>
          <p className="mt-1 text-xs text-white/40">Personalize a aparência do Big Picture</p>
        </div>
        <button
          data-theme-action="back"
          onClick={onClose}
          className="rounded-lg border border-white/10 px-4 py-2 text-xs font-bold uppercase tracking-wider hover:bg-white/10"
        >
          Voltar
        </button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto px-8 py-6">
        {error && (
          <div className="mb-4 rounded-lg bg-red-900/50 border border-red-500/30 px-4 py-3 text-sm">
            {error}
            <button onClick={() => setError(null)} className="ml-3 text-xs underline">Fechar</button>
          </div>
        )}

        {loading ? (
          <div className="flex items-center justify-center py-20 text-white/40">
            <span className="text-sm">Carregando temas...</span>
          </div>
        ) : (
          <div className="grid gap-4" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))" }}>
            {themes.map((theme) => (
              <ThemeCard
                key={theme.id}
                theme={theme}
                isActive={themeState.activeId === theme.id}
                onActivate={() => handleActivate(theme.id)}
                onRemove={() => handleRemove(theme.id)}
              />
            ))}
          </div>
        )}

        {/* Actions */}
        <div className="mt-8 flex gap-3">
          <button
            onClick={handleImport}
            disabled={importing}
            className="rounded-lg border border-white/20 px-5 py-3 text-xs font-bold uppercase tracking-wider hover:bg-white/10 disabled:opacity-50"
          >
            {importing ? "Importando..." : "Importar tema"}
          </button>
          <button
            onClick={handleRecover}
            className="rounded-lg border border-white/10 px-5 py-3 text-xs font-bold uppercase tracking-wider text-white/50 hover:bg-white/5"
          >
            Restaurar padrão
          </button>
        </div>
      </div>
    </div>
  )
}

function ThemeCard({
  theme,
  isActive,
  onActivate,
  onRemove,
}: {
  theme: ThemeInfo
  isActive: boolean
  onActivate: () => void
  onRemove: () => void
}) {
  const isBuiltin = theme.source === "builtin"
  const hasError = theme.error && theme.error !== ""

  return (
    <div
      className={`relative rounded-xl border p-5 transition-all ${
        isActive
          ? "border-[var(--fs-color-accent,#72ddff)] bg-[var(--fs-color-accent,#72ddff)]/10"
          : "border-white/10 bg-white/5 hover:bg-white/8"
      }`}
    >
      {/* Preview placeholder */}
      {theme.manifest?.previews?.[0] ? (
        <div className="mb-4 aspect-video overflow-hidden rounded-lg bg-white/5">
          <img
            src={theme.manifest.previews[0]}
            alt={theme.manifest.name}
            className="h-full w-full object-cover"
          />
        </div>
      ) : (
        <div className="mb-4 aspect-video rounded-lg bg-gradient-to-br from-white/5 to-white/10 flex items-center justify-center">
          <span className="text-white/20 text-xs">Sem preview</span>
        </div>
      )}

      {/* Info */}
      <div className="mb-3">
        <div className="flex items-center gap-2">
          <h3 className="font-bold text-sm">{theme.manifest?.name || theme.id}</h3>
          {isActive && (
            <span className="rounded bg-[var(--fs-color-accent,#72ddff)] px-2 py-0.5 text-[10px] font-bold text-black uppercase">
              Ativo
            </span>
          )}
          {isBuiltin && (
            <span className="rounded bg-white/10 px-2 py-0.5 text-[10px] font-bold text-white/50 uppercase">
              Embutido
            </span>
          )}
        </div>
        {theme.manifest?.author && (
          <p className="mt-1 text-xs text-white/40">por {theme.manifest.author}</p>
        )}
        {theme.manifest?.description && (
          <p className="mt-2 text-xs text-white/50 line-clamp-2">{theme.manifest.description}</p>
        )}
      </div>

      {/* Error */}
      {hasError && (
        <div className="mb-3 rounded bg-red-900/30 px-3 py-2 text-xs text-red-300">
          {theme.error}
        </div>
      )}

      {/* Compatibilidade */}
      {theme.manifest?.compat && theme.manifest.compat !== "ok" && (
        <div className="mb-3 rounded bg-yellow-900/30 px-3 py-2 text-xs text-yellow-300">
          Incompatível com a versão atual
        </div>
      )}

      {/* Actions */}
      <div className="flex gap-2">
        {!isActive && theme.valid && (
          <button
            onClick={onActivate}
            className="rounded-lg bg-[var(--fs-color-accent,#72ddff)] px-4 py-2 text-xs font-bold text-black uppercase tracking-wider hover:opacity-90"
          >
            Aplicar
          </button>
        )}
        {!isBuiltin && !isActive && (
          <button
            onClick={onRemove}
            className="rounded-lg border border-white/10 px-4 py-2 text-xs font-bold text-white/50 uppercase tracking-wider hover:bg-white/10 hover:text-white"
          >
            Remover
          </button>
        )}
      </div>
    </div>
  )
}
