import { useCallback, useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { useSteamHoras, recarregarSteamHoras } from "../steamHoras"

type Status = Awaited<ReturnType<NonNullable<Window["launcherAPI"]>["steamContaStatus"]>>

/**
 * Conta da Steam x conta do Arcadia (B1) e controle da captura (B2).
 *
 * Por que existe: o vigia de conquistas lê ARQUIVOS da máquina (saves de emulador,
 * `.bin` da Steam) e creditava tudo na conta do Arcadia ativa. Trocar de conta na
 * Steam sujava a outra. Aqui o dono vê em qual conta a Steam está, qual está
 * vinculada a esta conta do Arcadia, e decide: automático ligado/desligado, e
 * "Capturar agora" quando quiser forçar.
 */
export function SteamCaptureSection({ onSaved }: { onSaved?: () => void }) {
  const { t } = useI18n()
  const [status, setStatus] = useState<Status | null>(null)
  const [msg, setMsg] = useState("")
  const [ocupado, setOcupado] = useState(false)
  // Quantos jogos a leitura trouxe — diagnóstico na tela (se for 0 com a conta
  // certa, a leitura não chegou ao renderer).
  const jogosComHoras = Object.keys(useSteamHoras()).length

  const recarregar = useCallback(async () => {
    try {
      const s = await window.launcherAPI?.steamContaStatus()
      if (s) setStatus(s)
    } catch {}
  }, [])

  useEffect(() => {
    void recarregar()
  }, [recarregar])

  const aviso = useCallback(
    (texto: string) => {
      setMsg(texto)
      setTimeout(() => setMsg(""), 3000)
    },
    [],
  )

  const alternarAuto = async () => {
    if (!status) return
    setOcupado(true)
    try {
      await window.launcherAPI?.setConfig({
        achievements_auto_capture: !status.auto,
      } as Record<string, unknown>)
      await recarregar()
      onSaved?.()
    } finally {
      setOcupado(false)
    }
  }

  const vincular = async () => {
    setOcupado(true)
    try {
      const r = await window.launcherAPI?.steamVincularConta()
      await recarregar()
      onSaved?.()
      aviso(r?.ok ? t("steam_captura.vinculado") : t("steam_captura.falha_vinculo"))
    } finally {
      setOcupado(false)
    }
  }

  const capturarAgora = async () => {
    setOcupado(true)
    try {
      await window.launcherAPI?.steamCapturarAgora()
      // Releitura imediata: as horas da Steam também mudam depois de jogar, e o
      // botão é o caminho manual para atualizar tudo sem reiniciar.
      await recarregarSteamHoras()
      await recarregar()
      aviso(t("steam_captura.capturando"))
    } finally {
      setOcupado(false)
    }
  }

  if (!status || status.erro) return null

  const pausado = !status.permitido
  const nomeAtual = status.contaAtual?.persona || ""
  const nomeVinculo = status.vinculo?.persona || ""

  return (
    <section className="mb-8">
      <div className="flex flex-col gap-4 rounded-2xl border border-white/[0.06] bg-white/[0.02] p-4">
        <div>
          <h3 className="text-sm font-medium text-white">{t("steam_captura.titulo")}</h3>
          <p className="mt-1 text-[12px] text-white/50">{t("steam_captura.descricao")}</p>
        </div>

        {status.semSteam ? (
          <p className="text-[12px] text-amber-200/80">{t("steam_captura.sem_steam")}</p>
        ) : (
          <>
            <div className="grid grid-cols-1 gap-2 text-[12px] sm:grid-cols-2">
              <p className="text-white/70">
                {t("steam_captura.conta_steam")}: <strong className="text-white">{nomeAtual}</strong>
              </p>
              <p className="text-white/70">
                {t("steam_captura.conta_vinculada")}:{" "}
                <strong className="text-white">{nomeVinculo || "—"}</strong>
              </p>
            </div>

            {pausado && (
              <p
                role="status"
                className="rounded-lg border border-amber-300/40 bg-amber-100/[0.06] px-3 py-2 text-[12px] text-amber-50"
              >
                {status.vinculoOk
                  ? t("steam_captura.pausada_manual")
                  : t("steam_captura.pausada_trocada", {
                      atual: nomeAtual,
                      vinculada: nomeVinculo,
                    })}
              </p>
            )}

            <div className="flex flex-wrap items-center gap-2">
              <button
                onClick={alternarAuto}
                disabled={ocupado}
                className="rounded-lg border border-white/15 px-3.5 py-2 text-[12px] text-white/80 transition-colors hover:border-white/30 disabled:opacity-50"
              >
                {status.auto
                  ? t("steam_captura.desligar_auto")
                  : t("steam_captura.ligar_auto")}
              </button>
              <button
                onClick={vincular}
                disabled={ocupado || status.vinculoOk}
                className="rounded-lg border border-white/15 px-3.5 py-2 text-[12px] text-white/80 transition-colors hover:border-white/30 disabled:opacity-40"
              >
                {t("steam_captura.vincular")}
              </button>
              <button
                onClick={capturarAgora}
                disabled={ocupado}
                className="rounded-lg px-4 py-2 text-[12px] font-bold text-black transition-transform hover:scale-[1.03] disabled:opacity-50"
                style={{ background: "var(--accent)" }}
              >
                {t("steam_captura.capturar_agora")}
              </button>
              {msg && <span className="text-[11px] text-white/50">{msg}</span>}
            </div>

            <p className="text-[11px] text-white/35">{t("steam_captura.explicacao")}</p>
            {/* Diagnóstico: se este número for 0 e a conta estiver certa, a leitura
                não chegou na tela — é o primeiro lugar a olhar. */}
            <p className="text-[11px] text-white/35" data-horas-lidas>
              {t("steam_captura.horas_lidas", { n: String(jogosComHoras) })}
            </p>
          </>
        )}
      </div>
    </section>
  )
}
