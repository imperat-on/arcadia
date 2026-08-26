"use client"

import { useMemo, type ReactNode } from "react"
import type { FullscreenLayout, FullscreenSlotMap } from "../../themes/fullscreen/types"

interface FullscreenLayoutRendererProps {
  surface: "home" | "overview"
  layout?: FullscreenLayout | null
  slots: FullscreenSlotMap
  className?: string
}

export function FullscreenLayoutRenderer({
  surface,
  layout,
  slots,
  className,
}: FullscreenLayoutRendererProps) {
  const gridStyle = useMemo(() => {
    if (!layout?.grid) return {}

    const { columns, rows, areas } = layout.grid
    return {
      display: "grid" as const,
      gridTemplateColumns: columns.join(" "),
      gridTemplateRows: rows.join(" "),
      gridTemplateAreas: areas.map((row) => `"${row.join(" ")}"`).join(" "),
      width: "100%",
      height: "100%",
    }
  }, [layout])

  // Sem layout customizado: renderiza slots em ordem padrão
  if (!layout?.grid) {
    return (
      <div className={className} data-layout-surface={surface}>
        {Object.entries(slots).map(([key, node]) => (
          <div key={key} data-theme-slot={key} style={{ display: "contents" }}>
            {node}
          </div>
        ))}
      </div>
    )
  }

  // Com layout customizado: posiciona slots pelo grid
  return (
    <div className={className} style={gridStyle} data-layout-surface={surface}>
      {Object.entries(layout.slots).map(([slotName, slotDef]) => {
        const node = slots[slotName]
        if (!node && !slotDef.required) return null
        if (!node) return null

        return (
          <div
            key={slotName}
            data-theme-slot={slotName}
            style={{ gridArea: slotDef.area }}
          >
            {node}
          </div>
        )
      })}
    </div>
  )
}
