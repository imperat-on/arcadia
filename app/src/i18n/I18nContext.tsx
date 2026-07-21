import { createContext, useContext, useEffect, useState, type ReactNode } from "react"
import { ALL_MSGS, toLocale, type Vars } from "."
import { userLocale } from "./locale"

type Messages = Record<string, string>

interface I18nCtx {
  lang: string
  t: (key: string, vars?: Vars) => string
  locale: string
  setLang: (l: string) => void
}

const I18nContext = createContext<I18nCtx>({
  lang: "en-US",
  t: (k: string) => k,
  locale: "en-US",
  setLang: () => {},
})

// A detecção do idioma inicial mora no locale.ts, que os formatadores de data
// também usam — uma regra só para a interface e para as datas.
export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState(userLocale)
  // Já começa com as mensagens certas: com `{}` o primeiro quadro mostrava as
  // chaves cruas ("store.buscar") antes do efeito rodar.
  const [msgs, setMsgs] = useState<Messages>(() => ALL_MSGS[userLocale()] || ALL_MSGS["en-US"])

  const setLang = (l: string) => {
    setLangState(l)
    setMsgs(ALL_MSGS[l] || ALL_MSGS["en-US"])
    try { localStorage.setItem("arcadia_lang", l) } catch {}
    // O processo principal também precisa saber: é o idioma que ele manda para
    // a Steam e para a loja da Microsoft ao buscar descrições e requisitos.
    window.launcherAPI?.setConfig({ language: l })
  }

  // Primeira execução sem nada gravado: registra o idioma detectado, para o
  // main não continuar assumindo inglês.
  useEffect(() => {
    let salvo: string | null = null
    try { salvo = localStorage.getItem("arcadia_lang") } catch {}
    if (!salvo) setLang(lang)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const t = (key: string, vars?: Vars): string => {
    let s = msgs[key] || key
    if (vars) {
      for (const [k, v] of Object.entries(vars)) {
        s = s.split(`{${k}}`).join(String(v))
      }
    }
    return s
  }

  const locale = toLocale(lang)

  return (
    <I18nContext.Provider value={{ lang, t, locale, setLang }}>
      {children}
    </I18nContext.Provider>
  )
}

export function useI18n() {
  return useContext(I18nContext)
}
