import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

/**
 * "Na Steam: 81h47" — as horas que a conta mostra na própria Steam, lidas do
 * localconfig.vdf (`steam:horasDoJogo`). O Arcadia só contava o tempo das sessões
 * que ele mesmo lançou, então jogo aberto direto pela Steam aparecia com 0 e o
 * número nunca batia com a Steam.
 */
export function HorasNaSteam({ appid }: { appid: string }) {
  const { t } = useI18n()
  const [minutos, setMinutos] = useState(0)

  useEffect(() => {
    let vivo = true
    window.launcherAPI
      ?.steamHorasDoJogo(appid)
      .then((r) => {
        if (vivo && r?.ok) setMinutos(Number(r.minutos) || 0)
      })
      .catch(() => {})
    return () => {
      vivo = false
    }
  }, [appid])

  if (!appid || minutos <= 0) return null

  const h = Math.floor(minutos / 60)
  const m = minutos % 60
  return (
    <span className="text-[12px] text-white/45" data-steam-horas>
      {t("steam_captura.horas_na_steam", { h: String(h), m: String(m) })}
    </span>
  )
}
