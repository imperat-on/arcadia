import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

/**
 * Horas do jogo, combinando as duas fontes:
 *
 *   - a conta Steam (lida do `localconfig.vdf`) — é o TOTAL, e já inclui o tempo das
 *     sessões que o Arcadia lançou, porque o jogo abre pela própria Steam;
 *   - o que o Arcadia mediu por conta própria (`playtime_added_minutes`) — vale para
 *     jogo que a Steam não contabiliza (crackeado/emulador).
 *
 * A escolha é o MAIOR dos dois, nunca a soma: somar contaria duas vezes o mesmo
 * tempo. O maior também impede que um resíduo da Steam (segundos/minutos de um
 * teste) apague horas medidas pelo Arcadia — era assim que o perfil mostrava "3min"
 * num jogo com dezenas de horas.
 */
export function HorasNaSteam({
  appid,
  minutosArcadia = 0,
}: {
  appid: string
  minutosArcadia?: number
}) {
  const { t } = useI18n()
  const [minutosSteam, setMinutosSteam] = useState(0)

  useEffect(() => {
    let vivo = true
    setMinutosSteam(0)
    window.launcherAPI
      ?.steamHorasDoJogo(appid)
      .then((r) => {
        if (vivo && r?.ok) setMinutosSteam(Number(r.minutos) || 0)
      })
      .catch(() => {})
    return () => {
      vivo = false
    }
  }, [appid])

  const total = Math.max(minutosSteam, Number(minutosArcadia) || 0)
  const daSteam = minutosSteam > 0 && minutosSteam >= total
  if (!appid || total <= 0) return null

  const h = String(Math.floor(total / 60))
  const m = String(total % 60)
  return (
    <span className="text-[12px] text-white/45" data-horas-jogo>
      {daSteam
        ? t("steam_captura.horas_na_steam", { h, m })
        : t("steam_captura.tempo_jogo", { h, m })}
    </span>
  )
}
