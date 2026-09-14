import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

/**
 * Aviso fixo para o estado "integração na Steam LIGADA, mas sem chave do Hubcap".
 *
 * Esse estado não dá erro: o "Adicionar" entrega o jogo na biblioteca do Arcadia
 * e segue a vida. O problema é que ele entrega SEM injetar na Steam (a injeção
 * precisa do manifesto, e o provedor Hubcap exige a chave) — algo que só um toast
 * momentâneo denunciava. Aqui a pessoa vê o motivo na própria tela, com o que
 * fazer, enquanto a situação durar.
 *
 * `getConfig()` devolve a chave MASCARADA quando existe (auditoria A-06), então a
 * presença dela é o que importa: basta saber se o campo veio preenchido.
 */
export function AvisoSemChaveHubcap({ ativo }: { ativo: boolean }) {
  const { t } = useI18n()
  const [temChave, setTemChave] = useState<boolean | null>(null)

  useEffect(() => {
    let vivo = true
    const ler = () => {
      window.launcherAPI
        ?.getConfig()
        .then((c) => {
          if (vivo) setTemChave(Boolean((c as Record<string, unknown> | undefined)?.hubcap_api_key))
        })
        .catch(() => {})
    }
    ler()
    // A chave pode ser colada em "Loja → Configurar loja" com esta tela já aberta:
    // ao voltar o foco para a janela, relemos e o aviso some sozinho.
    window.addEventListener("focus", ler)
    return () => {
      vivo = false
      window.removeEventListener("focus", ler)
    }
  }, [ativo])

  if (!ativo || temChave !== false) return null

  return (
    <div className="mx-auto max-w-[1400px] px-6 pt-4">
      <p
        role="status"
        className="rounded-lg border border-amber-200/15 bg-amber-200/[0.04] px-3 py-2 text-[12px] text-amber-100/75"
      >
        <strong className="font-semibold text-amber-100">{t("loja.aviso_sem_chave_titulo")}</strong>{" "}
        {t("loja.aviso_sem_chave_texto")}
      </p>
    </div>
  )
}
