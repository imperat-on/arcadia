"use client"

import { useEffect, useMemo, useState } from "react"
import type { WineVer, ArtCandidate, EmulatorInfo } from "../../global"
import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"

// Diálogo "Adicionar jogo" (estilo Heroic): adiciona um jogo/app manualmente
// à biblioteca — Windows (via Wine) ou Linux nativo. Salvo em custom_games.json.

function slug(t: string) {
  return t
    .toLowerCase()
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
}

export function AddGameDialog({
  onClose,
  onAdded,
  editGame,
}: {
  onClose: () => void
  onAdded: () => void
  editGame?: Game | null
}) {
  const { t } = useI18n()
  const editando = Boolean(editGame)
  const custom = !editGame || editGame.launcher === "custom" // seções de Wine/exe
  const [titulo, setTitulo] = useState(editGame?.title || "")
  const [descricao, setDescricao] = useState(editGame?.description || "")
  const [platform, setPlatform] = useState<"windows" | "linux" | "emulator">(
    editGame?.platform === "emulator" ? "emulator" : editGame?.platform === "linux" ? "linux" : "windows",
  )
  const [exe, setExe] = useState(editGame?.exe || "")
  const [emulators, setEmulators] = useState<EmulatorInfo[]>([])
  const [emulatorId, setEmulatorId] = useState((editGame as (Game & { emulatorId?: string }) | null)?.emulatorId || "")
  const [romPath, setRomPath] = useState((editGame as (Game & { romPath?: string }) | null)?.romPath || "")
  // Backward-compatible per-game RetroArch override; global core editing lives in Settings > Emulação.
  const [emulatorCorePath, setEmulatorCorePath] = useState("")
  const [emulatorArgs, setEmulatorArgs] = useState("")
  const [prefix, setPrefix] = useState("")
  const [prefixPadrao, setPrefixPadrao] = useState("")
  const [wineVersion, setWineVersion] = useState("")
  const [wines, setWines] = useState<WineVer[]>([])
  const [erro, setErro] = useState("")
  const [busy, setBusy] = useState(false)
  // Busca automática de capa pelo título (SteamGridDB/fontes ligadas).
  const [candidatas, setCandidatas] = useState<ArtCandidate[]>([])
  const [buscandoArte, setBuscandoArte] = useState(false)
  const [capaEscolhida, setCapaEscolhida] = useState("")

  // Editando: o id é o do próprio jogo (preserva configs/arte). Novo: do slug.
  const id = useMemo(() => editGame?.id || `custom:${slug(titulo) || "jogo"}`, [titulo, editGame])

  // Debounce: 700ms após parar de digitar, busca capas e mostra previews.
  useEffect(() => {
    const q = titulo.trim()
    setCandidatas([])
    setCapaEscolhida("")
    if (q.length < 3) return
    setBuscandoArte(true)
    const t = setTimeout(() => {
      window.launcherAPI
        ?.searchArt(id, q, "cover")
        .then((r) => setCandidatas((r?.candidatos || []).slice(0, 4)))
        .finally(() => setBuscandoArte(false))
    }, 700)
    return () => clearTimeout(t)
  }, [titulo, id])

  useEffect(() => {
    window.launcherAPI?.wineList().then((r) => setWines(r?.installed || []))
  }, [])
  useEffect(() => {
    window.launcherAPI?.gameSettingsGet(id).then((r) => {
      setPrefixPadrao(r?.defaultPrefix || "")
      // Editando: preenche prefixo, Wine e perfil de emulador já configurados.
      if (editando) {
        setPrefix(r?.settings?.prefixPath || "")
        setWineVersion(r?.settings?.wineVersion || "")
        setEmulatorId(r?.settings?.emulatorId || "")
        setRomPath(r?.settings?.romPath || "")
        setEmulatorCorePath(r?.settings?.emulatorCorePath || "")
        setEmulatorArgs((r?.settings?.emulatorArgs || []).join(" "))
      }
    })
  }, [id])

  useEffect(() => {
    if (!custom || platform !== "emulator") return
    let active = true
    window.launcherAPI?.emulatorsList().then((r) => {
      if (!active || !r?.ok) return
      const detected = r.emulators || []
      setEmulators(detected)
      if (!emulatorId) {
        const first = detected.find((item) => item.available)
        if (first) setEmulatorId((previous) => previous || first.id)
      }
    })
    return () => { active = false }
  }, [custom, platform])

  const wineEscolhido = wines.find((w) => w.id === wineVersion)?.wine
  const prefixoEfetivo = prefix || prefixPadrao

  const pickExe = async () => {
    const r = await window.launcherAPI?.pickFile()
    if (r?.ok && r.path) setExe(r.path)
  }
  const pickPrefix = async () => {
    const r = await window.launcherAPI?.pickFolder()
    if (r?.ok && r.path) setPrefix(r.path)
  }
  const pickRom = async () => {
    const r = await window.launcherAPI?.pickFile()
    if (r?.ok && r.path) setRomPath(r.path)
  }
  const rodarInstalador = async () => {
    setBusy(true)
    await window.launcherAPI?.customGameRunInstaller({
      appid: id,
      wine: wineEscolhido,
      prefix: prefixoEfetivo || undefined,
    })
    setBusy(false)
  }

  const salvarCapaAutomatica = async () => {
    if (capaEscolhida) return
    const q = titulo.trim()
    if (q.length < 3) return
    const lista = candidatas.length
      ? candidatas
      : (await window.launcherAPI?.searchArt(id, q, "cover"))?.candidatos || []
    const capa = lista[0]
    if (capa?.url) {
      const r = await window.launcherAPI?.downloadArt(id, "cover", capa.url)
      if (r?.ok && r.path) {
        await window.launcherAPI?.setOverride(id, { cover: r.path })
        return
      }
    }
    const s = await window.launcherAPI?.storeSearch(q)
    const loja = (s?.jogos || [])[0]
    const url =
      loja?.cover ||
      (loja?.appid
        ? `https://cdn.cloudflare.steamstatic.com/steam/apps/${loja.appid}/library_600x900.jpg`
        : "")
    if (url) await window.launcherAPI?.setOverride(id, { cover: url })
  }

  const terminar = async () => {
    setErro("")
    if (!titulo.trim()) return setErro(t("addgame.erro_titulo"))
    if (custom && platform !== "emulator" && !exe) return setErro(t("addgame.erro_exe"))
    const args = emulatorArgs.trim() ? emulatorArgs.trim().split(/\s+/).slice(0, 32) : []
    if (custom && platform === "emulator") {
      if (!emulatorId) return setErro("Selecione um emulador.")
      if (!romPath) return setErro("Selecione a ROM/ISO do jogo.")
      const selectedEmulator = emulators.find((item) => item.id === emulatorId)
      if (!selectedEmulator?.profile && !selectedEmulator?.available) return setErro("Configure o emulador em Configurações › Emulação antes de adicionar jogos.")
      if (selectedEmulator?.requiresCore && !selectedEmulator.profile?.corePath) return setErro("Selecione um core do RetroArch em Configurações › Emulação.")
    }
    setBusy(true)
    if (editando && !custom) {
      // Jogo de loja (Steam/Epic/etc.): salva título/descrição via overrides.
      await window.launcherAPI?.setOverride(id, {
        title: titulo.trim(),
        description: descricao || null,
      })
      await salvarCapaAutomatica()
      setBusy(false)
      onAdded()
      onClose()
      return
    }

    if (custom && platform === "emulator") {
      const check = await window.launcherAPI?.emulatorsResolve({
        emulatorId,
        romPath,
        extraArgs: args,
        corePath: emulatorCorePath || undefined,
      })
      if (!check?.ok) {
        setBusy(false)
        return setErro(check?.error || "ROM ou emulador inválido.")
      }
    }

    // Configurações são locais: credenciais e ROMs nunca são enviadas ao backend.
    await window.launcherAPI?.gameSettingsSet(id, {
      prefixPath: platform === "emulator" ? undefined : (prefix || undefined),
      wineVersion: platform === "emulator" ? undefined : (wineVersion || undefined),
      emulatorId: platform === "emulator" ? emulatorId : undefined,
      romPath: platform === "emulator" ? romPath : undefined,
      emulatorArgs: platform === "emulator" ? args : undefined,
      emulatorCorePath: platform === "emulator" ? (emulatorCorePath || undefined) : undefined,
    })
    const r = editando
      ? await window.launcherAPI?.customGameUpdate({
          id,
          title: titulo.trim(),
          exe: platform === "emulator" ? undefined : exe,
          platform,
        })
      : await window.launcherAPI?.customGameAdd({
          id,
          title: titulo.trim(),
          platform,
          exe: platform === "emulator" ? "" : exe,
          emulatorId: platform === "emulator" ? emulatorId : undefined,
          romPath: platform === "emulator" ? romPath : undefined,
          emulatorArgs: platform === "emulator" ? args : undefined,
          emulatorCorePath: platform === "emulator" ? (emulatorCorePath || undefined) : undefined,
        })
    if (!r?.ok) {
      setBusy(false)
      return setErro(
        r?.error || (editando ? t("addgame.erro_falha_salvar") : t("addgame.erro_falha_adicionar")),
      )
    }
    await salvarCapaAutomatica()
    setBusy(false)
    onAdded()
    onClose()
  }

  const ArteBtn = ({ kind, label }: { kind: "cover" | "hero" | "logo"; label: string }) => (
    <button
      onClick={async () => {
        const r = await window.launcherAPI?.pickArt(id, kind)
        if (r?.ok && r.path) await window.launcherAPI?.setOverride(id, { [kind]: r.path })
      }}
      disabled={!titulo.trim()}
      className="rounded-lg border border-white/15 px-3 py-2 text-[12px] text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white disabled:opacity-40"
    >
      {label}
    </button>
  )

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="flex max-h-[88vh] w-[560px] max-w-[94vw] flex-col rounded-2xl border border-white/[0.08] bg-[#0d0d10] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between px-6 pt-5">
          <h2 className="text-lg font-light tracking-wide text-white">
            {editando
              ? t("addgame.editar_jogo", { title: editGame?.title || "" })
              : t("addgame.titulo_jogo")}
          </h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-white/40 transition-colors hover:bg-white/[0.06] hover:text-white"
          >
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
            >
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          <input
            value={titulo}
            onChange={(e) => setTitulo(e.target.value)}
            placeholder={t("addgame.titulo_placeholder")}
            spellCheck={false}
            className="mb-4 w-full rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/30 focus:border-[color:var(--accent)]"
          />

          {/* Imagens */}
          <label className="mb-1.5 block text-[12px] text-white/60">{t("addgame.imagens")}</label>
          <div className="mb-2 flex gap-2">
            <ArteBtn kind="cover" label={t("addgame.capa")} />
            <ArteBtn kind="hero" label={t("addgame.fundo")} />
            <ArteBtn kind="logo" label={t("addgame.logo")} />
          </div>
          {/* Preview da busca automática de capa */}
          {buscandoArte && (
            <p className="mb-3 text-[12px] text-white/40">{t("addgame.buscando_capas")}</p>
          )}
          {!buscandoArte && candidatas.length > 0 && (
            <div className="mb-4 flex gap-2.5">
              {candidatas.map((c) => (
                <button
                  key={c.url}
                  onClick={async () => {
                    const r = await window.launcherAPI?.downloadArt(id, "cover", c.url)
                    if (r?.ok && r.path) {
                      await window.launcherAPI?.setOverride(id, { cover: r.path })
                      setCapaEscolhida(c.url)
                    }
                  }}
                  title={`${c.fonte} · ${c.largura}x${c.altura}`}
                  className={`relative aspect-[2/3] w-[72px] overflow-hidden rounded-lg border-2 transition-all hover:scale-[1.04] ${
                    capaEscolhida === c.url ? "border-[color:var(--accent)]" : "border-transparent"
                  }`}
                >
                  <img
                    src={c.thumb || c.url}
                    alt=""
                    className="h-full w-full object-cover"
                    loading="lazy"
                    draggable={false}
                  />
                  {capaEscolhida === c.url && (
                    <span className="absolute inset-0 flex items-center justify-center bg-black/40">
                      <svg
                        width="20"
                        height="20"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="var(--accent)"
                        strokeWidth="3"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <polyline points="20 6 9 17 4 12" />
                      </svg>
                    </span>
                  )}
                </button>
              ))}
            </div>
          )}

          {custom ? (
            <>
              <label className="mb-1.5 block text-[12px] text-white/60">
                {t("addgame.plataforma")}
              </label>
              <select
                value={platform}
                onChange={(e) => setPlatform(e.target.value as "windows" | "linux" | "emulator")}
                className="mb-4 w-full appearance-none rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none focus:border-[color:var(--accent)]"
              >
                <option value="windows" className="bg-[#16161a]">
                  {t("addgame.windows")}
                </option>
                <option value="linux" className="bg-[#16161a]">
                  {t("addgame.linux_nativo")}
                </option>
                <option value="emulator" className="bg-[#16161a]">
                  Emulador (ROM/ISO)
                </option>
              </select>

              {platform === "windows" && (
                <>
                  <label className="mb-1.5 block text-[12px] text-white/60">
                    {t("addgame.prefixo_wine")}
                  </label>
                  <div className="mb-4 flex gap-2">
                    <input
                      value={prefixoEfetivo}
                      onChange={(e) => setPrefix(e.target.value)}
                      spellCheck={false}
                      className="flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors focus:border-[color:var(--accent)]"
                    />
                    <button
                      onClick={pickPrefix}
                      title={t("install.escolher_pasta")}
                      className="rounded-lg border border-white/10 bg-white/[0.05] px-3 text-white/70 transition-colors hover:bg-white/10 hover:text-white"
                    >
                      <svg
                        width="16"
                        height="16"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="1.8"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                      </svg>
                    </button>
                  </div>

                  <label className="mb-1.5 block text-[12px] text-white/60">
                    {t("addgame.versao_wine")}
                  </label>
                  <div className="relative mb-4">
                    <svg
                      className="absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-white/50"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.8"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    >
                      <path d="M8 22h8M12 15v7M7 3h10l-1 7a4 4 0 0 1-8 0L7 3z" />
                    </svg>
                    <select
                      value={wineVersion}
                      onChange={(e) => setWineVersion(e.target.value)}
                      className="w-full appearance-none rounded-lg border border-white/10 bg-white/[0.04] py-2.5 pl-10 pr-9 text-[13px] text-white outline-none focus:border-[color:var(--accent)]"
                    >
                      <option value="" className="bg-[#16161a]">
                        {t("addgame.padrao_sistema")}
                      </option>
                      {wines.map((w) => (
                        <option key={w.id} value={w.id} className="bg-[#16161a]">
                          {w.name}
                        </option>
                      ))}
                    </select>
                    <svg
                      className="pointer-events-none absolute right-3 top-1/2 h-4 w-4 -translate-y-1/2 text-white/40"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                    >
                      <polyline points="6 9 12 15 18 9" />
                    </svg>
                  </div>
                </>
              )}

              {platform === "emulator" && (
                <div className="mb-3 rounded-xl border border-white/[0.08] bg-black/20 p-3">
                  <p className="mb-2 text-xs font-medium text-white/75">Configuração do emulador</p>
                  <label className="mb-1.5 block text-[12px] text-white/60">Emulador</label>
                  <select
                    value={emulatorId}
                    onChange={(e) => {
                      const next = e.target.value
                      setEmulatorId(next)
                    }}
                    className="mb-3 w-full rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none focus:border-[color:var(--accent)]"
                  >
                    <option value="" className="bg-[#16161a]">Selecione…</option>
                    {emulators.map((item) => (
                      <option key={item.id} value={item.id} className="bg-[#16161a]">
                        {item.name} · {item.systems.join(" / ")}{item.available ? "" : " (não detectado)"}
                      </option>
                    ))}
                  </select>
                  <p className="mb-3 rounded-lg border border-white/[0.06] bg-white/[0.025] px-3 py-2 text-[11px] leading-relaxed text-white/45">
                    O executável, BIOS e core são configurados em <strong className="font-medium text-white/65">Configurações › Emulação</strong>.
                  </p>
                  <label className="mb-1.5 block text-[12px] text-white/60">ROM/ISO</label>
                  <div className="mb-3 flex gap-2">
                    <input value={romPath} readOnly placeholder="Selecione um arquivo" className="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 font-mono text-[12px] text-white/70" />
                    <button type="button" onClick={pickRom} className="rounded-lg border border-white/10 px-3 text-xs text-white/70 hover:bg-white/10">Escolher</button>
                  </div>
                  <label className="block text-[12px] text-white/60">Argumentos adicionais (argv)</label>
                  <input
                    value={emulatorArgs}
                    onChange={(e) => setEmulatorArgs(e.target.value)}
                    placeholder="--fullscreen"
                    maxLength={4096}
                    className="mt-1 w-full rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 font-mono text-[12px] text-white outline-none focus:border-[color:var(--accent)]"
                  />
                  <p className="mt-2 text-[11px] text-white/35">Nenhum comando é interpretado por shell; os argumentos são enviados como array.</p>
                </div>
              )}

              {platform !== "emulator" && (
                <>
                  <label className="mb-1.5 block text-[12px] text-white/60">
                    {t("addgame.selecionar_exe")}
                  </label>
                  <div className="mb-2 flex gap-2">
                    <button
                      onClick={pickExe}
                      className={`flex flex-1 items-center justify-between rounded-lg border px-3.5 py-2.5 text-left text-[13px] transition-colors ${
                        exe ? "border-white/15 text-white/80" : "border-white/10 text-white/35"
                      } bg-white/[0.04] hover:border-white/25`}
                    >
                      <span className="truncate">{exe || t("addgame.selecionar_exe")}</span>
                      <svg
                        width="16"
                        height="16"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="1.8"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        className="shrink-0 text-white/50"
                      >
                        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                      </svg>
                    </button>
                  </div>
                </>
              )}
            </>
          ) : (
            <>
              <label className="mb-1.5 block text-[12px] text-white/60">
                {t("addgame.descricao")}
              </label>
              <textarea
                value={descricao}
                onChange={(e) => setDescricao(e.target.value)}
                rows={6}
                spellCheck={false}
                placeholder={t("addgame.descricao_placeholder")}
                className="mb-2 w-full resize-y rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/30 focus:border-[color:var(--accent)]"
              />
            </>
          )}
          {erro && <p className="mb-2 text-[12px] text-[#ff6b81]">{erro}</p>}
        </div>

        <div className="flex justify-end gap-2.5 border-t border-white/[0.06] px-6 py-4">
          {custom && platform === "windows" && (
            <button
              onClick={rodarInstalador}
              disabled={busy}
              className="rounded-lg px-5 py-2.5 text-[12px] font-bold tracking-wide text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
              style={{ background: "var(--accent)" }}
            >
              {t("addgame.executar_instalador")}
            </button>
          )}
          <button
            onClick={terminar}
            disabled={busy}
            className="rounded-lg border border-white/20 px-5 py-2.5 text-[12px] font-semibold tracking-wide text-white/80 transition-colors hover:bg-white/[0.06] hover:text-white disabled:opacity-50"
          >
            {t("addgame.terminar")}
          </button>
        </div>
      </div>
    </div>
  )
}
