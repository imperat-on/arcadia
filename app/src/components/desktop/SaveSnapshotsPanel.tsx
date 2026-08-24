"use client"

import { useCallback, useEffect, useState } from "react"
import type { SaveSnapshot } from "../../global"
import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"
import { userLocale } from "../../i18n/locale"

/**
 * Pequeno painel por jogo para os snapshots locais de saves.
 *
 * Os caminhos são escolhidos somente no momento da operação e nunca são
 * persistidos pelo renderer. O processo principal continua sendo responsável
 * por validar/copiar/restaurar os diretórios.
 */
export function SaveSnapshotsPanel({ game }: { game: Game }) {
  const { t } = useI18n()
  const [snapshots, setSnapshots] = useState<SaveSnapshot[]>([])
  const [sourceDir, setSourceDir] = useState("")
  const [targetDir, setTargetDir] = useState("")
  const [label, setLabel] = useState("")
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState("")
  const [message, setMessage] = useState<{ kind: "ok" | "error"; text: string } | null>(null)

  const carregar = useCallback(async () => {
    if (!window.launcherAPI?.savesList) {
      setSnapshots([])
      setLoading(false)
      return
    }
    setLoading(true)
    try {
      const result = await window.launcherAPI.savesList(game.id)
      setSnapshots(Array.isArray(result) ? result : [])
    } catch {
      setMessage({ kind: "error", text: t("gamesettings.salvamentos_erro_listar") })
    } finally {
      setLoading(false)
    }
  }, [game.id])

  useEffect(() => {
    let ativo = true
    setLoading(true)
    const request = window.launcherAPI?.savesList?.(game.id)
    if (!request) {
      setSnapshots([])
      setLoading(false)
      return () => {
        ativo = false
      }
    }
    request.then(
      (result) => {
        if (!ativo) return
        setSnapshots(Array.isArray(result) ? result : [])
        setLoading(false)
      },
      () => {
        if (!ativo) return
        setMessage({ kind: "error", text: t("gamesettings.salvamentos_erro_listar") })
        setLoading(false)
      },
    )
    return () => {
      ativo = false
    }
  }, [game.id])

  const escolherPasta = async (tipo: "origem" | "destino") => {
    const result = await window.launcherAPI?.pickFolder?.()
    if (!result?.ok || !result.path) return
    if (tipo === "origem") setSourceDir(result.path)
    else setTargetDir(result.path)
    setMessage(null)
  }

  const criar = async () => {
    if (!sourceDir || !window.launcherAPI?.savesCreate) return
    setBusy("create")
    setMessage(null)
    try {
      const result = await window.launcherAPI.savesCreate({
        gameId: game.id,
        sourceDir,
        label: label.trim() || undefined,
      })
      if (!result?.ok) {
        setMessage({
          kind: "error",
          text: result?.error || t("gamesettings.salvamentos_erro_criar"),
        })
        return
      }
      setLabel("")
      setMessage({ kind: "ok", text: t("gamesettings.salvamentos_criado") })
      await carregar()
    } catch {
      setMessage({ kind: "error", text: t("gamesettings.salvamentos_erro_criar") })
    } finally {
      setBusy("")
    }
  }

  const restaurar = async (snapshot: SaveSnapshot) => {
    if (!targetDir || !window.launcherAPI?.savesRestore) return
    const nome = snapshot.label || snapshot.source_name || snapshot.id
    if (
      !window.confirm(t("gamesettings.salvamentos_confirmar_restaurar", { nome, pasta: targetDir }))
    )
      return
    setBusy(snapshot.id)
    setMessage(null)
    try {
      const result = await window.launcherAPI.savesRestore({
        gameId: game.id,
        snapshotId: snapshot.id,
        targetDir,
        backup: true,
      })
      if (!result?.ok) {
        setMessage({
          kind: "error",
          text: result?.error || t("gamesettings.salvamentos_erro_restaurar"),
        })
        return
      }
      setMessage({
        kind: "ok",
        text: result.backupPath
          ? t("gamesettings.salvamentos_restaurado_backup")
          : t("gamesettings.salvamentos_restaurado"),
      })
    } catch {
      setMessage({ kind: "error", text: t("gamesettings.salvamentos_erro_restaurar") })
    } finally {
      setBusy("")
    }
  }

  const remover = async (snapshot: SaveSnapshot) => {
    if (!window.launcherAPI?.savesDelete) return
    const nome = snapshot.label || snapshot.source_name || snapshot.id
    if (!window.confirm(t("gamesettings.salvamentos_confirmar_remover", { nome }))) return
    setBusy(snapshot.id)
    setMessage(null)
    try {
      const result = await window.launcherAPI.savesDelete({
        gameId: game.id,
        snapshotId: snapshot.id,
      })
      if (!result?.ok) {
        setMessage({
          kind: "error",
          text: result?.error || t("gamesettings.salvamentos_erro_remover"),
        })
        return
      }
      setMessage({ kind: "ok", text: t("gamesettings.salvamentos_removido") })
      await carregar()
    } catch {
      setMessage({ kind: "error", text: t("gamesettings.salvamentos_erro_remover") })
    } finally {
      setBusy("")
    }
  }

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1.5 text-[17px] font-semibold text-white">
          {t("gamesettings.salvamentos_titulo")}
        </h3>
        <p className="text-[13px] leading-relaxed text-white/55">
          {t("gamesettings.salvamentos_desc")}
        </p>
      </div>

      <div className="rounded-xl border border-white/[0.08] bg-white/[0.025] p-3.5">
        <h4 className="mb-3 text-[12px] font-semibold uppercase tracking-wider text-white/70">
          {t("gamesettings.salvamentos_criar")}
        </h4>
        <DiretorioSelecionado
          label={t("gamesettings.salvamentos_origem")}
          value={sourceDir}
          placeholder={t("gamesettings.salvamentos_origem_placeholder")}
          chooseLabel={t("gamesettings.escolher_pasta")}
          onChoose={() => escolherPasta("origem")}
        />
        <label className="mt-3 block text-[12px] text-white/55" htmlFor="snapshot-label">
          {t("gamesettings.salvamentos_rotulo")}
        </label>
        <input
          id="snapshot-label"
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          maxLength={120}
          placeholder={t("gamesettings.salvamentos_rotulo_placeholder")}
          spellCheck={false}
          className="mt-1.5 w-full rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/25 focus:border-[color:var(--accent)]"
        />
        <button
          onClick={criar}
          disabled={!sourceDir || busy !== ""}
          className="mt-3 rounded-lg px-4 py-2.5 text-[12px] font-bold tracking-wide text-black transition-transform enabled:hover:scale-[1.02] disabled:cursor-not-allowed disabled:opacity-40"
          style={{ background: "var(--accent)" }}
        >
          {busy === "create"
            ? t("gamesettings.salvamentos_trabalhando")
            : t("gamesettings.salvamentos_criar_botao")}
        </button>
      </div>

      <div className="rounded-xl border border-white/[0.08] bg-white/[0.025] p-3.5">
        <h4 className="mb-1 text-[12px] font-semibold uppercase tracking-wider text-white/70">
          {t("gamesettings.salvamentos_restaurar")}
        </h4>
        <p className="mb-3 text-[12px] leading-relaxed text-white/45">
          {t("gamesettings.salvamentos_restaurar_desc")}
        </p>
        <DiretorioSelecionado
          label={t("gamesettings.salvamentos_destino")}
          value={targetDir}
          placeholder={t("gamesettings.salvamentos_destino_placeholder")}
          chooseLabel={t("gamesettings.escolher_pasta")}
          onChoose={() => escolherPasta("destino")}
        />

        <div className="mt-4 flex flex-col gap-2">
          {loading ? (
            <p className="text-[12px] text-white/40">{t("gamesettings.salvamentos_carregando")}</p>
          ) : snapshots.length === 0 ? (
            <p className="rounded-lg border border-dashed border-white/10 px-3 py-4 text-center text-[12px] text-white/35">
              {t("gamesettings.salvamentos_vazio")}
            </p>
          ) : (
            snapshots.map((snapshot) => {
              const nome = snapshot.label || snapshot.source_name || snapshot.id
              const data = new Date(snapshot.created_at)
              const dataTexto = Number.isNaN(data.getTime())
                ? snapshot.created_at
                : data.toLocaleString(userLocale())
              const ocupado = busy === snapshot.id
              return (
                <div
                  key={snapshot.id}
                  className="flex items-center gap-3 rounded-lg border border-white/[0.07] bg-black/20 px-3 py-2.5"
                >
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-[13px] text-white/85" title={nome}>
                      {nome}
                    </p>
                    <p className="truncate text-[11px] text-white/35" title={snapshot.source_name}>
                      {dataTexto} · {snapshot.source_name}
                    </p>
                  </div>
                  <button
                    onClick={() => restaurar(snapshot)}
                    disabled={!targetDir || busy !== ""}
                    className="shrink-0 rounded-md border border-[color:color-mix(in_srgb,var(--accent)_55%,transparent)] px-2.5 py-1.5 text-[11px] font-semibold text-[color:var(--accent)] transition-colors enabled:hover:bg-[color:color-mix(in_srgb,var(--accent)_12%,transparent)] disabled:cursor-not-allowed disabled:opacity-35"
                  >
                    {ocupado
                      ? t("gamesettings.salvamentos_trabalhando")
                      : t("gamesettings.salvamentos_restaurar_botao")}
                  </button>
                  <button
                    onClick={() => remover(snapshot)}
                    disabled={busy !== ""}
                    title={t("gamesettings.salvamentos_remover")}
                    className="shrink-0 rounded-md border border-white/10 px-2 py-1.5 text-white/45 transition-colors enabled:hover:bg-red-500/10 enabled:hover:text-red-300 disabled:cursor-not-allowed disabled:opacity-35"
                  >
                    <svg
                      width="13"
                      height="13"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                    >
                      <polyline points="3 6 5 6 21 6" />
                      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                    </svg>
                  </button>
                </div>
              )
            })
          )}
        </div>
      </div>

      {message && (
        <p
          className={`text-[12px] ${message.kind === "error" ? "text-red-300" : "text-emerald-300"}`}
        >
          {message.text}
        </p>
      )}
    </div>
  )
}

function DiretorioSelecionado({
  label,
  value,
  placeholder,
  chooseLabel,
  onChoose,
}: {
  label: string
  value: string
  placeholder: string
  chooseLabel: string
  onChoose: () => void
}) {
  return (
    <div>
      <label className="mb-1.5 block text-[12px] text-white/55">{label}</label>
      <div className="flex gap-2">
        <div
          className={`min-w-0 flex-1 truncate rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] ${value ? "text-white/75" : "text-white/25"}`}
          title={value || placeholder}
        >
          {value || placeholder}
        </div>
        <button
          onClick={onChoose}
          className="flex shrink-0 items-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-3.5 text-[12px] text-white/80 transition-colors hover:bg-white/10 hover:text-white"
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
          </svg>
          {chooseLabel}
        </button>
      </div>
    </div>
  )
}
