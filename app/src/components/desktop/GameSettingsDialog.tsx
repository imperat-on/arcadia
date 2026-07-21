"use client"

import { useEffect, useMemo, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import type { GameSettings, WineVer } from "../../global"
import { useI18n } from "../../i18n/I18nContext"

const ABAS = ["WINE", "OUTROS", "AVANÇADO", "GAMESCOPE", "LEGADO"] as const
type Aba = (typeof ABAS)[number]

const PADRAO: GameSettings = {
  autoDXVK: true,
  autoNVAPI: true,
  autoVKD3D: true,
  esync: true,
  fsync: true,
  wineWayland: false,
  wow64: false,
  fsrHack: false,
  gamescope: false,
  gsWidth: 1920,
  gsHeight: 1080,
  gsFps: 0,
  dxvkHud: "",
  mangohud: false,
  gamemode: false,
  verboseLogs: false,
  gameArgs: "",
  scriptPre: "",
  scriptPost: "",
  wrappers: [],
  envVars: [],
}

export function GameSettingsDialog({ game, onClose }: { game: Game; onClose: () => void }) {
  const { t } = useI18n()
  const [aba, setAba] = useState<Aba>("WINE")
  const [s, setS] = useState<GameSettings>({ ...PADRAO })
  const [defaultPrefix, setDefaultPrefix] = useState("")
  const [wines, setWines] = useState<WineVer[]>([])
  const [ajuda, setAjuda] = useState<string>("")
  const [novoWrap, setNovoWrap] = useState({ cmd: "", args: "" })
  const [novaVar, setNovaVar] = useState({ name: "", value: "" })

  const ABA_LABEL: Record<Aba, string> = {
    WINE: t("gamesettings.wine"),
    OUTROS: t("gamesettings.outros"),
    AVANÇADO: t("gamesettings.avancado"),
    GAMESCOPE: t("gamesettings.gamescope"),
    LEGADO: t("gamesettings.legado"),
  }

  useEffect(() => {
    window.launcherAPI?.gameSettingsGet(game.id).then((r) => {
      setS({ ...PADRAO, ...(r?.settings || {}) })
      setDefaultPrefix(r?.defaultPrefix || "")
    })
    window.launcherAPI?.wineList().then((r) => setWines(r?.installed || []))
  }, [game.id])

  const set = (patch: Partial<GameSettings>) => {
    setS((prev) => ({ ...prev, ...patch }))
    window.launcherAPI?.gameSettingsSet(game.id, patch)
  }

  const wineEscolhido = useMemo(() => {
    const v = wines.find((w) => w.id === s.wineVersion)
    return v?.wine || undefined
  }, [wines, s.wineVersion])

  const prefixoEfetivo = s.prefixPath || defaultPrefix
  const ferramentaOpts = { wine: wineEscolhido, prefix: s.prefixPath || undefined }

  const ferramenta = (tool: "winecfg" | "winetricks" | "wineboot") =>
    window.launcherAPI?.prefixTool(game.id, tool, ferramentaOpts)
  const rodarExe = () => window.launcherAPI?.wineRunExe(game.id, ferramentaOpts)

  const Info = ({ id, label, texto }: { id: string; label: string; texto: string }) => (
    <div className="mb-2">
      <button
        onClick={() => setAjuda(ajuda === id ? "" : id)}
        className="flex items-center gap-2 text-[13px] text-white/75 transition-colors hover:text-white"
      >
        <span className="flex h-[15px] w-[15px] items-center justify-center rounded-full text-[10px] font-bold text-black" style={{ background: "var(--accent)" }}>i</span>
        {label}
      </button>
      {ajuda === id && <p className="ml-6 mt-1 text-[12px] leading-relaxed text-white/45">{texto}</p>}
    </div>
  )

  const Check = ({ k, label, hint, disabled }: { k: keyof GameSettings; label: string; hint?: string; disabled?: boolean }) => (
    <div className={`mb-3.5 flex items-center justify-between ${disabled ? "opacity-40" : ""}`}>
      <label className="flex cursor-pointer items-center gap-3">
        <input
          type="checkbox"
          checked={Boolean(s[k])}
          disabled={disabled}
          onChange={(e) => set({ [k]: e.target.checked } as Partial<GameSettings>)}
          className="h-[18px] w-[18px] cursor-pointer appearance-none rounded-[4px] border border-white/30 bg-transparent transition-colors checked:border-transparent"
          style={
            s[k]
              ? {
                  background: "var(--accent)",
                  backgroundImage: "url(\"data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='black' stroke-width='3.5' stroke-linecap='round' stroke-linejoin='round'><polyline points='20 6 9 17 4 12'/></svg>\")",
                  backgroundSize: "12px",
                  backgroundPosition: "center",
                  backgroundRepeat: "no-repeat",
                }
              : undefined
          }
        />
        <span className="text-[13px] text-white/85">{label}</span>
      </label>
      {hint && (
        <span className="group/tip relative flex h-[15px] w-[15px] shrink-0 cursor-help items-center justify-center rounded-full text-[10px] font-bold text-black" style={{ background: "var(--accent)" }}>
          i
          <span className="pointer-events-none absolute right-0 top-5 z-50 w-56 rounded-lg border border-white/10 bg-[#1a1a20] px-3 py-2 text-left text-[11px] font-normal leading-relaxed text-white/80 opacity-0 shadow-xl transition-opacity duration-100 group-hover/tip:opacity-100">
            {hint}
          </span>
        </span>
      )}
    </div>
  )

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm" onClick={onClose}>
      <div
        className="flex max-h-[88vh] w-[620px] max-w-[94vw] flex-col rounded-2xl border border-white/[0.08] bg-[#0d0d10] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Título */}
        <div className="flex items-start justify-between px-6 pt-5">
          <h2 className="text-lg font-light lowercase tracking-wide text-white">{t("gamesettings.titulo", { title: game.title })}</h2>
          <button onClick={onClose} className="rounded-md p-1 text-white/40 transition-colors hover:bg-white/[0.06] hover:text-white">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        {/* Abas */}
        <div className="mt-3 flex gap-6 border-b border-white/[0.08] px-6">
          {ABAS.map((a) => (
            <button
              key={a}
              onClick={() => setAba(a)}
              className={`pb-2.5 text-[12px] font-semibold tracking-wider transition-colors ${
                aba === a ? "border-b-2 border-white text-white" : "text-white/45 hover:text-white/80"
              }`}
            >
              {ABA_LABEL[a]}
            </button>
          ))}
        </div>

        {/* Conteúdo */}
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
          {aba === "WINE" && game.launcher === "steam" && (
            <div className="flex flex-col items-start gap-3 rounded-xl border border-white/[0.08] bg-white/[0.03] p-5">
              <div className="flex items-center gap-2.5">
                <span className="flex h-6 w-6 items-center justify-center rounded-full text-[13px] font-bold text-black" style={{ background: "var(--accent)" }}>i</span>
                <h3 className="text-[14px] font-semibold text-white">{t("gamesettings.gerenciado_steam")}</h3>
              </div>
              <p className="text-[13px] leading-relaxed text-white/60">
                {t("gamesettings.steam_info1")} <span className="text-white/85">{t("gamesettings.steam_info2")}</span>{t("gamesettings.steam_info3")}
              </p>
              <p className="text-[13px] leading-relaxed text-white/60">
                {t("gamesettings.steam_info4")}
              </p>
            </div>
          )}

          {aba === "WINE" && game.launcher !== "steam" && (
            <>
              <label className="mb-1.5 block text-[13px] text-white/70">{t("gamesettings.versao_wine")}</label>
              <div className="relative mb-3">
                <svg className="absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-white/50" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M8 22h8M12 15v7M7 3h10l-1 7a4 4 0 0 1-8 0L7 3z" />
                </svg>
                <select
                  value={s.wineVersion || ""}
                  onChange={(e) => set({ wineVersion: e.target.value || undefined })}
                  className="w-full appearance-none rounded-lg border border-white/10 bg-white/[0.04] py-2.5 pl-10 pr-9 text-[13px] text-white outline-none transition-colors focus:border-[color:var(--accent)]"
                >
                  <option value="" className="bg-[#16161a]">{t("gamesettings.wine_padrao")}</option>
                  {wines.map((w) => (
                    <option key={w.id} value={w.id} className="bg-[#16161a]">{w.name}</option>
                  ))}
                </select>
                <svg className="pointer-events-none absolute right-3 top-1/2 h-4 w-4 -translate-y-1/2 text-white/40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </div>

              <Info id="proton" label={t("gamesettings.proton_ignorados")} texto={t("gamesettings.proton_ignorados_info")} />
              <Info id="caminho" label={t("gamesettings.caminho_wine")} texto={wineEscolhido || t("gamesettings.caminho_wine_info")} />
              <Info id="ajuda_wine" label={t("gamesettings.ajuda")} texto={t("gamesettings.ajuda_info")} />

              <label className="mb-1.5 mt-4 block text-[13px] text-white/70">{t("gamesettings.pasta_prefixo")}</label>
              <div className="mb-3 flex gap-2">
                <input
                  value={prefixoEfetivo}
                  onChange={(e) => set({ prefixPath: e.target.value === defaultPrefix ? undefined : e.target.value })}
                  spellCheck={false}
                  className="flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors focus:border-[color:var(--accent)]"
                />
                <button
                  onClick={async () => {
                    const r = await window.launcherAPI?.pickFolder()
                    if (r?.ok && r.path) set({ prefixPath: r.path })
                  }}
                  title={t("gamesettings.escolher_pasta")}
                  className="rounded-lg border border-white/10 bg-white/[0.05] px-3 text-white/70 transition-colors hover:bg-white/10 hover:text-white"
                >
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                  </svg>
                </button>
              </div>
              <Info id="pref" label={t("gamesettings.pasta_prefixo")} texto={t("gamesettings.prefixo_info")} />

              <div className="mt-4">
                <Check k="autoDXVK" label={t("gamesettings.auto_dxvk")} hint={t("gamesettings.auto_dxvk_hint")} />
                <Check k="autoNVAPI" label={t("gamesettings.auto_dxvk_nvapi")} hint={t("gamesettings.auto_dxvk_nvapi_hint")} />
                <Check k="autoVKD3D" label={t("gamesettings.auto_vkd3d")} hint={t("gamesettings.auto_vkd3d_hint")} />
                <Check k="esync" label={t("gamesettings.esync")} hint={t("gamesettings.esync_hint")} />
                <Check k="fsync" label={t("gamesettings.fsync")} hint={t("gamesettings.fsync_hint")} />
                <Check k="wineWayland" label={t("gamesettings.wine_wayland")} hint={t("gamesettings.wine_wayland_hint")} />
                <Check k="wow64" label={t("gamesettings.wow64")} hint={t("gamesettings.wow64_hint")} />
                <Check k="fsrHack" label={t("gamesettings.fsr_hack")} hint={t("gamesettings.fsr_hack_hint")} />
              </div>

              <div className="mt-2 flex flex-wrap gap-2.5">
                {(["WINECFG", "WINETRICKS"] as const).map((tt) => (
                  <button
                    key={tt}
                    onClick={() => ferramenta(tt.toLowerCase() as "winecfg" | "winetricks")}
                    className="rounded-lg border px-4 py-2 text-[12px] font-semibold tracking-wide transition-colors hover:bg-white/[0.06]"
                    style={{ borderColor: "color-mix(in srgb, var(--accent) 60%, transparent)", color: "var(--accent)" }}
                  >
                    {tt === "WINECFG" ? t("gamesettings.winecfg") : t("gamesettings.winetricks")}
                  </button>
                ))}
                <button
                  onClick={rodarExe}
                  className="rounded-lg border px-4 py-2 text-[12px] font-semibold tracking-wide transition-colors hover:bg-white/[0.06]"
                  style={{ borderColor: "color-mix(in srgb, var(--accent) 60%, transparent)", color: "var(--accent)" }}
                >
                  {t("gamesettings.executar_exe")}
                </button>
              </div>
            </>
          )}

          {aba === "OUTROS" && (
            <>
              <Check k="gamemode" label={t("gamesettings.gamemode")} hint={t("gamesettings.gamemode_hint")} />
              <Check k="mangohud" label={t("gamesettings.mangohud")} hint={t("gamesettings.mangohud_hint")} />
              <label className="mb-1.5 mt-2 block text-[13px] text-white/70">{t("gamesettings.dxvk_hud")}</label>
              <select
                value={s.dxvkHud || ""}
                onChange={(e) => set({ dxvkHud: e.target.value || undefined })}
                className="w-full appearance-none rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors focus:border-[color:var(--accent)]"
              >
                <option value="" className="bg-[#16161a]">{t("gamesettings.dxvk_hud_off")}</option>
                <option value="fps" className="bg-[#16161a]">{t("gamesettings.dxvk_hud_fps")}</option>
                <option value="frametimes" className="bg-[#16161a]">{t("gamesettings.dxvk_hud_frametimes")}</option>
                <option value="full" className="bg-[#16161a]">{t("gamesettings.dxvk_hud_completo")}</option>
              </select>
            </>
          )}

          {aba === "AVANÇADO" && (
            <>
              <Check k="verboseLogs" label={t("gamesettings.verbose_logs")} hint={t("gamesettings.verbose_logs_hint", { id: game.id })} />

              <label className="mb-1.5 mt-2 block text-[13px] text-white/70">{t("gamesettings.argumentos")}</label>
              <input
                value={s.gameArgs || ""}
                onChange={(e) => set({ gameArgs: e.target.value })}
                spellCheck={false}
                placeholder={t("gamesettings.argumentos_placeholder")}
                className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/25 focus:border-[color:var(--accent)]"
              />
              <div className="mt-2">
                <Info id="args" label={t("gamesettings.ajuda")} texto={t("gamesettings.argumentos_info")} />
              </div>

              <h3 className="mb-2 mt-5 text-[13px] font-medium text-white/80">{t("gamesettings.scripts")}</h3>
              {([
                ["scriptPre", t("gamesettings.script_pre")],
                ["scriptPost", t("gamesettings.script_pos")],
              ] as const).map(([k, label]) => (
                <div key={k} className="mb-4">
                  <label className="mb-1.5 block text-[12px] text-white/55">{label}</label>
                  <div className="flex gap-2">
                    <div className="flex-1 truncate rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white/60">
                      {(s[k] as string) || <span className="text-white/25">{t("gamesettings.script_placeholder")}</span>}
                    </div>
                    {s[k] ? (
                      <button
                        onClick={() => set({ [k]: "" } as Partial<GameSettings>)}
                        title={t("gamesettings.limpar")}
                        className="rounded-lg border border-white/10 bg-white/[0.05] px-3 text-white/60 transition-colors hover:bg-white/10 hover:text-white"
                      >
                        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
                          <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                        </svg>
                      </button>
                    ) : null}
                    <button
                      onClick={async () => {
                        const r = await window.launcherAPI?.pickFile()
                        if (r?.ok && r.path) set({ [k]: r.path } as Partial<GameSettings>)
                      }}
                      title={t("gamesettings.escolher_script")}
                      className="rounded-lg border border-white/10 bg-white/[0.05] px-3 text-white/70 transition-colors hover:bg-white/10 hover:text-white"
                    >
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                      </svg>
                    </button>
                  </div>
                </div>
              ))}

              <h3 className="mb-2 mt-2 text-[13px] font-medium text-white/80">{t("gamesettings.wrapper")}</h3>
              {(s.wrappers || []).length > 0 && (
                <>
                  <div className="mb-1 grid grid-cols-[1fr_1fr_28px] gap-2 text-[12px] font-semibold" style={{ color: "var(--accent)" }}>
                    <span>{t("gamesettings.wrapper_cabecalho")}</span><span>{t("gamesettings.wrapper_argumentos")}</span><span />
                  </div>
                  {(s.wrappers || []).map((w, i) => (
                    <div key={i} className="mb-1.5 grid grid-cols-[1fr_1fr_28px] items-center gap-2">
                      <span className="truncate rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 font-mono text-[12px] text-white/80">{w.cmd}</span>
                      <span className="truncate rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 font-mono text-[12px] text-white/60">{w.args || "—"}</span>
                      <button
                        onClick={() => set({ wrappers: (s.wrappers || []).filter((_, j) => j !== i) })}
                        title={t("gamesettings.remover")}
                        className="flex h-7 w-7 items-center justify-center rounded-md text-white/40 transition-colors hover:bg-white/10 hover:text-white"
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
                          <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                        </svg>
                      </button>
                    </div>
                  ))}
                </>
              )}
              <div className="grid grid-cols-[1fr_1fr_28px] items-center gap-2">
                <input
                  value={novoWrap.cmd}
                  onChange={(e) => setNovoWrap({ ...novoWrap, cmd: e.target.value })}
                  spellCheck={false}
                  placeholder={t("gamesettings.novo_wrapper")}
                  className="rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 text-[12px] text-white outline-none placeholder:text-white/25 focus:border-[color:var(--accent)]"
                />
                <input
                  value={novoWrap.args}
                  onChange={(e) => setNovoWrap({ ...novoWrap, args: e.target.value })}
                  spellCheck={false}
                  placeholder={t("gamesettings.wrapper_args_placeholder")}
                  className="rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 text-[12px] text-white outline-none placeholder:text-white/25 focus:border-[color:var(--accent)]"
                />
                <button
                  onClick={() => {
                    const cmd = novoWrap.cmd.trim()
                    if (!cmd) return
                    set({ wrappers: [...(s.wrappers || []), { cmd, args: novoWrap.args.trim() }] })
                    setNovoWrap({ cmd: "", args: "" })
                  }}
                  disabled={!novoWrap.cmd.trim()}
                  title={t("gamesettings.adicionar_wrapper")}
                  className="flex h-7 w-7 items-center justify-center rounded-md text-black transition-transform enabled:hover:scale-110 disabled:opacity-40"
                  style={{ background: "var(--accent)" }}
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
                    <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
                  </svg>
                </button>
              </div>
              <div className="mt-2">
                <Info id="wrap" label={t("gamesettings.ajuda")} texto={t("gamesettings.wrapper_ajuda")} />
              </div>

              <h3 className="mb-2 mt-5 text-[13px] font-medium text-white/80">{t("gamesettings.env_vars")}</h3>
              {(s.envVars || []).length > 0 && (
                <>
                  <div className="mb-1 grid grid-cols-[1fr_16px_1fr_28px] gap-2 text-[12px] font-semibold" style={{ color: "var(--accent)" }}>
                    <span>{t("gamesettings.env_nome")}</span><span /><span>{t("gamesettings.env_valor")}</span><span />
                  </div>
                  {(s.envVars || []).map((v, i) => (
                    <div key={i} className="mb-1.5 grid grid-cols-[1fr_16px_1fr_28px] items-center gap-2">
                      <span className="truncate rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 font-mono text-[12px] text-white/80">{v.name}</span>
                      <span className="text-center text-white/40">=</span>
                      <span className="truncate rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 font-mono text-[12px] text-white/60">{v.value}</span>
                      <button
                        onClick={() => set({ envVars: (s.envVars || []).filter((_, j) => j !== i) })}
                        title={t("gamesettings.remover")}
                        className="flex h-7 w-7 items-center justify-center rounded-md text-white/40 transition-colors hover:bg-white/10 hover:text-white"
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
                          <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                        </svg>
                      </button>
                    </div>
                  ))}
                </>
              )}
              <div className="grid grid-cols-[1fr_16px_1fr_28px] items-center gap-2">
                <input
                  value={novaVar.name}
                  onChange={(e) => setNovaVar({ ...novaVar, name: e.target.value })}
                  spellCheck={false}
                  placeholder={t("gamesettings.env_placeholder_nome")}
                  className={`rounded-lg border bg-white/[0.04] px-3 py-2 text-[12px] text-white outline-none placeholder:text-white/25 focus:border-[color:var(--accent)] ${
                    novaVar.name && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(novaVar.name) ? "border-red-500/60" : "border-white/10"
                  }`}
                />
                <span className="text-center text-white/40">=</span>
                <input
                  value={novaVar.value}
                  onChange={(e) => setNovaVar({ ...novaVar, value: e.target.value })}
                  spellCheck={false}
                  placeholder={t("gamesettings.env_placeholder_valor")}
                  className="rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 text-[12px] text-white outline-none placeholder:text-white/25 focus:border-[color:var(--accent)]"
                />
                <button
                  onClick={() => {
                    const name = novaVar.name.trim()
                    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) return
                    set({ envVars: [...(s.envVars || []).filter((v) => v.name !== name), { name, value: novaVar.value }] })
                    setNovaVar({ name: "", value: "" })
                  }}
                  disabled={!/^[A-Za-z_][A-Za-z0-9_]*$/.test(novaVar.name.trim())}
                  title={t("gamesettings.adicionar_variavel")}
                  className="flex h-7 w-7 items-center justify-center rounded-md text-black transition-transform enabled:hover:scale-110 disabled:opacity-40"
                  style={{ background: "var(--accent)" }}
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
                    <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
                  </svg>
                </button>
              </div>
              <div className="mt-2">
                <Info id="env" label={t("gamesettings.ajuda")} texto={t("gamesettings.env_ajuda")} />
              </div>
            </>
          )}

          {aba === "GAMESCOPE" && (
            <>
              <Check k="gamescope" label={t("gamesettings.gamescope_label")} hint={t("gamesettings.gamescope_hint")} />
              <div className="mt-2 grid grid-cols-3 gap-3">
                <div>
                  <label className="mb-1.5 block text-[12px] text-white/60">{t("gamesettings.gamescope_largura")}</label>
                  <input
                    type="number"
                    value={s.gsWidth || 1920}
                    onChange={(e) => set({ gsWidth: Number(e.target.value) || 1920 })}
                    className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2.5 text-[13px] text-white outline-none focus:border-[color:var(--accent)]"
                  />
                </div>
                <div>
                  <label className="mb-1.5 block text-[12px] text-white/60">{t("gamesettings.gamescope_altura")}</label>
                  <input
                    type="number"
                    value={s.gsHeight || 1080}
                    onChange={(e) => set({ gsHeight: Number(e.target.value) || 1080 })}
                    className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2.5 text-[13px] text-white outline-none focus:border-[color:var(--accent)]"
                  />
                </div>
                <div>
                  <label className="mb-1.5 block text-[12px] text-white/60">{t("gamesettings.gamescope_fps")}</label>
                  <input
                    type="number"
                    value={s.gsFps || 0}
                    onChange={(e) => set({ gsFps: Number(e.target.value) || 0 })}
                    className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2.5 text-[13px] text-white outline-none focus:border-[color:var(--accent)]"
                  />
                </div>
              </div>
            </>
          )}

          {aba === "LEGADO" && (
            <>
              <p className="mb-4 text-[13px] text-white/55">{t("gamesettings.legado_desc")}</p>
              <div className="flex flex-wrap gap-2.5">
                <button
                  onClick={() => ferramenta("wineboot")}
                  className="rounded-lg border px-4 py-2 text-[12px] font-semibold tracking-wide transition-colors hover:bg-white/[0.06]"
                  style={{ borderColor: "color-mix(in srgb, var(--accent) 60%, transparent)", color: "var(--accent)" }}
                >
                  {t("gamesettings.reparar_prefixo")}
                </button>
                <button
                  onClick={() => ferramenta("winecfg")}
                  className="rounded-lg border px-4 py-2 text-[12px] font-semibold tracking-wide transition-colors hover:bg-white/[0.06]"
                  style={{ borderColor: "color-mix(in srgb, var(--accent) 60%, transparent)", color: "var(--accent)" }}
                >
                  {t("gamesettings.abrir_winecfg")}
                </button>
                <button
                  onClick={() => window.launcherAPI?.openExternal(`file://${prefixoEfetivo}`)}
                  className="rounded-lg border border-white/15 px-4 py-2 text-[12px] font-semibold tracking-wide text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white"
                >
                  {t("gamesettings.abrir_pasta_prefixo")}
                </button>
              </div>
            </>
          )}
        </div>

        {/* Rodapé */}
        <div className="border-t border-white/[0.06] px-6 py-3">
          <p className="text-[12px] text-white/55">{t("gamesettings.salvas_auto")}</p>
          <p className="mt-0.5 font-mono text-[11px] text-white/25">AppName: {game.id}</p>
        </div>
      </div>
    </div>
  )
}
