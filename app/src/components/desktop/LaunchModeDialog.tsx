"use client"

import { useI18n } from "../../i18n/I18nContext"
import { Modal } from "../../ui/Modal"
import type { Game } from "../ps5-launcher/types"

// Menu de escolha de como iniciar um jogo que tem as duas formas possíveis
// (modo padrão do Arcadia + executável configurado na aba Localizações).
//
// O overlay (portal, backdrop, Esc, foco preso, scroll travado, camada z) vem
// do Modal; aqui só mora a escolha em si. O texto e as classes são os mesmos de
// antes — a migração não muda o visual.
export function LaunchModeDialog({
  game,
  onEscolher,
  onClose,
  active = true,
}: {
  game: Game
  onEscolher: (mode: "steam" | "exe") => void
  onClose: () => void
  /** Host focus gate; desktop callers keep the normal default. */
  active?: boolean
}) {
  const { t } = useI18n()

  const Opcao = ({
    mode,
    titulo,
    desc,
  }: {
    mode: "steam" | "exe"
    titulo: string
    desc: string
  }) => (
    <button
      onClick={() => onEscolher(mode)}
      className="w-full rounded-xl border border-white/10 bg-white/[0.03] p-4 text-left transition-colors hover:border-[color:var(--accent)]/50 hover:bg-white/[0.06]"
    >
      <span className="block text-[14px] font-semibold text-white">{titulo}</span>
      <span className="mt-1 block text-[12px] leading-snug text-white/50">{desc}</span>
    </button>
  )

  return (
    <Modal onClose={onClose} title={t("launchmode.titulo")} size="sm" gamepad={active}>
      <p className="mb-5 text-[13px] text-white/70">
        {t("launchmode.subtitulo", { name: game.title })}
      </p>

      <div className="flex flex-col gap-3">
        <Opcao
          mode="steam"
          titulo={t("launchmode.steam_titulo")}
          desc={t("launchmode.steam_desc")}
        />
        <Opcao mode="exe" titulo={t("launchmode.exe_titulo")} desc={t("launchmode.exe_desc")} />
      </div>
    </Modal>
  )
}
