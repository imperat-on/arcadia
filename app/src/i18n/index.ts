import ptBR from "./pt-BR.json"
import enUS from "./en-US.json"
import esES from "./es-ES.json"

export type Messages = Record<string, string>

export const ALL_MSGS: Record<string, Messages> = {
  "pt-BR": ptBR,
  "en-US": enUS,
  "es-ES": esES,
}

/** Valores interpolados: número é comum (contagens, índices). */
export type Vars = Record<string, string | number>

export function t(lang: string, key: string, vars?: Vars): string {
  const msgs = ALL_MSGS[lang] || ALL_MSGS["en-US"]
  let s = msgs[key] || key
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      s = s.split(`{${k}}`).join(String(v))
    }
  }
  return s
}

export function toLocale(lang: string): string {
  return lang === "pt-BR" ? "pt-BR" : lang === "es-ES" ? "es-ES" : "en-US"
}
