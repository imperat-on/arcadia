/**
 * Texto puro a partir do HTML que o appdetails devolve.
 *
 * Renderizar HTML de terceiros num app com acesso a IPC é risco desnecessário,
 * então extraímos só o texto. As entidades precisam ser desfeitas na mão: sem
 * isso, uma descrição com aspas aparece como `&quot;Pals&quot;` na tela.
 */
export function semHtml(s: string): string {
  return s
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&nbsp;/g, " ")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    // &amp; por último: desfazer antes recriaria as outras entidades.
    .replace(/&amp;/g, "&")
    .trim()
}
