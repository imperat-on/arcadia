// Idioma escolhido, sem depender do React.
//
// Existe separado do I18nContext porque quem chama é `toLocaleDateString` e
// `toLocaleTimeString` dentro de funções puras (formatadores de data e hora),
// onde não dá para usar um hook. As duas pontas têm de responder a mesma
// coisa: com regras diferentes, o relógio ficaria em formato americano numa
// interface em português.

const IDIOMAS = ["pt-BR", "en-US", "es-ES"]

/**
 * Idioma da primeira execução: o do sistema, quando é um dos três que
 * traduzimos. Sem isto, quem nunca abriu as configurações via tudo em inglês.
 */
export function userLocale(): string {
  try {
    const salvo = localStorage.getItem("arcadia_lang")
    if (salvo && IDIOMAS.includes(salvo)) return salvo
  } catch {}
  const nav = typeof navigator !== "undefined" ? navigator.language || "" : ""
  if (/^pt/i.test(nav)) return "pt-BR"
  if (/^es/i.test(nav)) return "es-ES"
  return "en-US"
}
