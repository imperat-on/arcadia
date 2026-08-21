"use client"

import { useEffect, useState } from "react"
import type { EmulatorInfo } from "../../global"

/**
 * Links are intentionally kept in the renderer as a small allowlist.  The
 * installer help must never accept a URL supplied by a ROM, profile, or a
 * remote catalog.  `openExternal` is still called through preload, so the
 * renderer does not navigate its own window or execute a download.
 */
export interface EmulatorInstallMetadata {
  id: string
  name: string
  officialUrl: string
  releasesUrl?: string
  /** Official release page containing the Linux AppImage, when published. */
  appImageUrl?: string
  flatpakId?: string
  flatpakUrl?: string
  appImageNote?: string
}

/**
 * Official installation sources for every emulator in the built-in registry.
 * AppImage links intentionally point at the vendor release page rather than a
 * versioned asset URL; release assets change and the user can verify the
 * architecture/checksum before downloading.
 */
export const EMULATOR_INSTALL_METADATA: Record<string, EmulatorInstallMetadata> = {
  duckstation: {
    id: "duckstation",
    name: "DuckStation",
    officialUrl: "https://www.duckstation.org/",
    releasesUrl: "https://github.com/stenzek/duckstation/releases",
    appImageUrl: "https://github.com/stenzek/duckstation/releases/latest",
    flatpakId: "org.duckstation.DuckStation",
    flatpakUrl: "https://flathub.org/en/apps/org.duckstation.DuckStation",
  },
  pcsx2: {
    id: "pcsx2",
    name: "PCSX2",
    officialUrl: "https://pcsx2.net/",
    releasesUrl: "https://github.com/PCSX2/pcsx2/releases",
    appImageUrl: "https://github.com/PCSX2/pcsx2/releases/latest",
    flatpakId: "net.pcsx2.PCSX2",
    flatpakUrl: "https://flathub.org/en/apps/net.pcsx2.PCSX2",
  },
  rpcs3: {
    id: "rpcs3",
    name: "RPCS3",
    officialUrl: "https://rpcs3.net/",
    releasesUrl: "https://github.com/RPCS3/rpcs3-binaries-linux/releases",
    appImageUrl: "https://github.com/RPCS3/rpcs3-binaries-linux/releases/latest",
    flatpakId: "net.rpcs3.RPCS3",
    flatpakUrl: "https://flathub.org/en/apps/net.rpcs3.RPCS3",
  },
  dolphin: {
    id: "dolphin",
    name: "Dolphin",
    officialUrl: "https://dolphin-emu.org/",
    releasesUrl: "https://dolphin-emu.org/download/",
    flatpakId: "org.DolphinEmu.dolphin-emu",
    flatpakUrl: "https://flathub.org/en/apps/org.DolphinEmu.dolphin-emu",
    appImageNote:
      "O site oficial oferece pacotes Linux; não há um AppImage oficial estável indicado aqui.",
  },
  ppsspp: {
    id: "ppsspp",
    name: "PPSSPP",
    officialUrl: "https://www.ppsspp.org/",
    releasesUrl: "https://www.ppsspp.org/download/",
    flatpakId: "org.ppsspp.PPSSPP",
    flatpakUrl: "https://flathub.org/en/apps/org.ppsspp.PPSSPP",
    appImageNote: "Use o pacote Linux publicado no site oficial ou o Flatpak.",
  },
  retroarch: {
    id: "retroarch",
    name: "RetroArch",
    officialUrl: "https://www.retroarch.com/",
    releasesUrl: "https://www.retroarch.com/?page=platforms",
    flatpakId: "org.libretro.RetroArch",
    flatpakUrl: "https://flathub.org/en/apps/org.libretro.RetroArch",
    appImageNote: "Para Linux, o Flatpak e os pacotes da distribuição são as opções recomendadas.",
  },
  melonds: {
    id: "melonds",
    name: "melonDS",
    officialUrl: "https://melonds.kuribo64.net/",
    releasesUrl: "https://melonds.kuribo64.net/downloads.php",
    flatpakId: "net.kuribo64.melonDS",
    flatpakUrl: "https://flathub.org/en/apps/net.kuribo64.melonDS",
    appImageNote: "Use os downloads Linux do projeto ou o Flatpak.",
  },
  desmume: {
    id: "desmume",
    name: "DeSmuME",
    officialUrl: "https://desmume.org/",
    releasesUrl: "https://desmume.org/download/",
    flatpakId: "org.desmume.DeSmuME",
    flatpakUrl: "https://flathub.org/en/apps/org.desmume.DeSmuME",
    appImageNote:
      "O projeto publica pacotes Linux; não há um AppImage oficial estável indicado aqui.",
  },
}

/** Return metadata only for a known built-in emulator. */
export function getEmulatorInstallMetadata(
  emulator: string | Pick<EmulatorInfo, "id">,
): EmulatorInstallMetadata | null {
  const id = typeof emulator === "string" ? emulator : emulator.id
  return (
    EMULATOR_INSTALL_METADATA[
      String(id || "")
        .trim()
        .toLowerCase()
    ] || null
  )
}

export function flatpakInstallCommand(metadata: EmulatorInstallMetadata): string {
  return metadata.flatpakId ? `flatpak install flathub ${metadata.flatpakId}` : ""
}

/** Fixed argv used by the native Linux launcher; no shell is involved. */
export function flatpakLaunchArguments(metadata: EmulatorInstallMetadata): string[] {
  return metadata.flatpakId ? ["run", metadata.flatpakId] : []
}

/**
 * Flatpak itself is not a host executable path.  This tiny, user-created
 * wrapper lets Arcadia's argv-based launcher pass the ROM path to `flatpak
 * run` without adding shell execution to the application.
 */
export function flatpakWrapperScript(metadata: EmulatorInstallMetadata): string {
  if (!metadata.flatpakId) return ""
  return `#!/usr/bin/env sh\nexec flatpak run ${metadata.flatpakId} "$@"\n`
}

export function flatpakFilesystemCommand(metadata: EmulatorInstallMetadata): string {
  return metadata.flatpakId
    ? `flatpak override --user --filesystem=/path/to/roms:ro ${metadata.flatpakId}`
    : ""
}

export interface EmulatorInstallDialogProps {
  item: Pick<EmulatorInfo, "id" | "name"> | EmulatorInfo
  /** Close the whole configuration dialog. */
  onClose?: () => void
  /** Alias useful when this dialog is nested in a configuration wizard. */
  onBack?: () => void
  /** Optional completion callback for a parent wizard. */
  onDone?: () => void | Promise<void>
  /** Re-run the read-only detector after the user installs an emulator. */
  onRefresh?: () => void | Promise<void>
  /** Optional hand-off to the existing executable picker. */
  onManualBrowse?: () => void | Promise<void>
}

/**
 * Installation guidance shown by the setup wizard when an emulator is not
 * found.  This is deliberately guidance, not an installer: distro package
 * managers, AppImages, and Flatpaks have different trust/sandbox rules, and
 * no untrusted URL or command is executed from the renderer.
 */
export function EmulatorInstallDialog({
  item,
  onClose,
  onBack,
  onDone,
  onRefresh,
  onManualBrowse,
}: EmulatorInstallDialogProps) {
  const metadata = getEmulatorInstallMetadata(item)
  const [copied, setCopied] = useState("")
  const [refreshing, setRefreshing] = useState(false)
  const [error, setError] = useState("")
  const [message, setMessage] = useState("")
  const dismiss = onClose || onBack || (() => undefined)

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") dismiss()
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [dismiss])

  const openExternal = async (url: string) => {
    setError("")
    try {
      if (!window.launcherAPI?.openExternal) throw new Error("openExternal indisponível")
      await window.launcherAPI.openExternal(url)
    } catch {
      setError("Não foi possível abrir o link no navegador padrão.")
    }
  }

  const copy = async (value: string, key: string) => {
    setError("")
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(value)
      } else {
        const textarea = document.createElement("textarea")
        textarea.value = value
        textarea.setAttribute("readonly", "")
        textarea.style.position = "fixed"
        textarea.style.opacity = "0"
        document.body.appendChild(textarea)
        textarea.select()
        if (!document.execCommand("copy")) throw new Error("copy failed")
        textarea.remove()
      }
      setCopied(key)
      window.setTimeout(() => setCopied((current) => (current === key ? "" : current)), 1800)
    } catch {
      setError("Não foi possível copiar o comando. Selecione-o manualmente.")
    }
  }

  const refresh = async () => {
    if (!onRefresh) return
    setRefreshing(true)
    setError("")
    setMessage("")
    try {
      await onRefresh()
      setMessage(
        "Detecção atualizada. Se o emulador aparecer, feche esta janela e continue a configuração.",
      )
    } catch {
      setError("Não foi possível atualizar a detecção agora.")
    } finally {
      setRefreshing(false)
    }
  }

  const browse = async () => {
    if (!onManualBrowse) return
    setError("")
    try {
      await onManualBrowse()
    } catch {
      setError("Não foi possível abrir o seletor de executável.")
    }
  }

  const finish = async () => {
    if (!onDone) return
    setError("")
    try {
      await onDone()
    } catch {
      setError("Não foi possível voltar à configuração.")
    }
  }

  const name = metadata?.name || item.name
  const flatpakCommand = metadata ? flatpakInstallCommand(metadata) : ""
  const wrapper = metadata ? flatpakWrapperScript(metadata) : ""
  const filesystemCommand = metadata ? flatpakFilesystemCommand(metadata) : ""

  return (
    <div
      className="fixed inset-0 z-[90] flex items-center justify-center bg-black/75 p-4 backdrop-blur-sm"
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="emulator-install-title"
        className="flex max-h-[92vh] w-[760px] max-w-full flex-col overflow-hidden rounded-2xl border border-white/[0.1] bg-[#0d0d10] shadow-2xl shadow-black/60"
      >
        <div className="flex items-start justify-between border-b border-white/[0.07] px-6 py-5">
          <div>
            <p className="mb-1 text-[11px] font-semibold uppercase tracking-[0.2em] text-white/35">
              Guia de instalação
            </p>
            <h2 id="emulator-install-title" className="text-xl font-semibold text-white">
              Instalar {name} no Linux
            </h2>
            <p className="mt-1 max-w-xl text-xs leading-relaxed text-white/45">
              Escolha uma fonte oficial, instale fora do Arcadia e depois use “Detectar novamente”.
              O Arcadia não baixa BIOS, firmware ou ROMs.
            </p>
          </div>
          <button
            type="button"
            onClick={dismiss}
            className="rounded-lg p-1.5 text-white/45 transition-colors hover:bg-white/[0.07] hover:text-white"
            aria-label="Fechar guia de instalação"
          >
            <CloseIcon />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
          {error && (
            <p
              role="alert"
              className="mb-4 rounded-xl border border-red-300/20 bg-red-400/10 px-3 py-2 text-xs text-red-100"
            >
              {error}
            </p>
          )}
          {message && (
            <p
              role="status"
              className="mb-4 rounded-xl border border-emerald-300/15 bg-emerald-400/[0.07] px-3 py-2 text-xs text-emerald-100"
            >
              {message}
            </p>
          )}

          {!metadata ? (
            <div className="rounded-xl border border-amber-300/15 bg-amber-400/[0.06] p-4 text-sm text-amber-100">
              Não há um guia oficial cadastrado para este emulador. Use o site do projeto e volte
              para apontar o executável manualmente.
            </div>
          ) : (
            <>
              <div className="mb-5 grid gap-3 sm:grid-cols-2">
                <InstallSourceCard
                  title="Site oficial"
                  description={`Downloads e instruções oficiais do ${name}.`}
                  action="Abrir site"
                  onOpen={() => void openExternal(metadata.officialUrl)}
                  tone="neutral"
                />
                <InstallSourceCard
                  title="Releases oficiais"
                  description="Confira a versão, a arquitetura e os checksums antes de baixar."
                  action="Abrir releases"
                  onOpen={() => void openExternal(metadata.releasesUrl || metadata.officialUrl)}
                  tone="neutral"
                />
              </div>

              <section className="mb-5 rounded-xl border border-white/[0.08] bg-white/[0.025] p-4">
                <div className="mb-2 flex items-center justify-between gap-3">
                  <div>
                    <h3 className="text-sm font-medium text-white/85">AppImage</h3>
                    <p className="mt-1 text-xs leading-relaxed text-white/45">
                      {metadata.appImageUrl
                        ? "Baixe o AppImage na página oficial de releases e torne o arquivo executável."
                        : metadata.appImageNote ||
                          "Não há um AppImage oficial indicado; use o site ou o Flatpak."}
                    </p>
                  </div>
                  <span className="rounded-full border border-sky-300/15 bg-sky-300/[0.06] px-2 py-1 text-[10px] font-semibold uppercase tracking-wide text-sky-100/75">
                    Linux
                  </span>
                </div>
                {metadata.appImageUrl && (
                  <button
                    type="button"
                    onClick={() => void openExternal(metadata.appImageUrl!)}
                    className="rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 text-xs font-medium text-white/75 transition-colors hover:border-white/25 hover:bg-white/[0.08] hover:text-white"
                  >
                    Abrir página do AppImage <span aria-hidden="true">↗</span>
                  </button>
                )}
                <ol className="mt-3 space-y-1 text-[11px] leading-relaxed text-white/45">
                  <li>1. Baixe a versão Linux compatível com sua arquitetura.</li>
                  <li>
                    2. No terminal, rode{" "}
                    <code className="rounded bg-black/30 px-1 py-0.5 font-mono text-white/65">
                      chmod +x ~/Downloads/&lt;arquivo&gt;.AppImage
                    </code>
                    .
                  </li>
                  <li>3. Em “Explorar manualmente”, selecione o arquivo AppImage já executável.</li>
                </ol>
              </section>

              {metadata.flatpakId && (
                <section className="mb-5 rounded-xl border border-violet-300/15 bg-violet-400/[0.045] p-4">
                  <div className="mb-3 flex items-start justify-between gap-3">
                    <div>
                      <h3 className="text-sm font-medium text-white/85">Flatpak / Flathub</h3>
                      <p className="mt-1 text-xs leading-relaxed text-white/45">
                        Instalações Flatpak são isoladas. No Arcadia nativo, informe{" "}
                        <code className="rounded bg-black/30 px-1 py-0.5 font-mono text-white/65">
                          flatpak
                        </code>{" "}
                        como executável e use os argumentos fixos{" "}
                        <code className="rounded bg-black/30 px-1 py-0.5 font-mono text-white/65">
                          run
                        </code>{" "}
                        + App ID; se o seletor exigir um arquivo, use o wrapper abaixo.
                      </p>
                    </div>
                    <span className="rounded-full border border-violet-200/15 bg-violet-300/[0.08] px-2 py-1 text-[10px] font-semibold uppercase tracking-wide text-violet-100/75">
                      Sandbox
                    </span>
                  </div>
                  <div className="mb-3 flex flex-wrap gap-2">
                    <button
                      type="button"
                      onClick={() =>
                        void openExternal(
                          metadata.flatpakUrl ||
                            `https://flathub.org/en/apps/${metadata.flatpakId}`,
                        )
                      }
                      className="rounded-lg border border-white/10 bg-white/[0.05] px-3 py-2 text-xs font-medium text-white/75 transition-colors hover:border-white/25 hover:bg-white/[0.1] hover:text-white"
                    >
                      Abrir no Flathub <span aria-hidden="true">↗</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => void copy(flatpakCommand, "install")}
                      className="rounded-lg border border-white/10 px-3 py-2 text-xs text-white/65 transition-colors hover:bg-white/[0.08] hover:text-white"
                    >
                      {copied === "install" ? "Copiado" : "Copiar comando"}
                    </button>
                  </div>
                  <CodeBlock value={flatpakCommand} />
                  <p className="mb-1 mt-4 text-[11px] font-medium text-white/65">
                    Configuração no Arcadia (sem shell)
                  </p>
                  <p className="mb-2 text-[11px] leading-relaxed text-white/40">
                    Se o campo permitir comando, use{" "}
                    <code className="rounded bg-black/30 px-1 py-0.5 font-mono text-white/65">
                      flatpak
                    </code>{" "}
                    como executável e informe cada item abaixo em uma linha nos argumentos
                    adicionais.
                  </p>
                  <CodeBlock value={flatpakLaunchArguments(metadata).join("\n")} multiline />
                  <p className="mb-1 mt-4 text-[11px] font-medium text-white/65">
                    Wrapper para o executável do Arcadia
                  </p>
                  <p className="mb-2 text-[11px] leading-relaxed text-white/40">
                    Salve o bloco como{" "}
                    <code className="rounded bg-black/30 px-1 py-0.5 font-mono text-white/65">
                      ~/.local/bin/arcadia-{metadata.id}
                    </code>
                    , rode{" "}
                    <code className="rounded bg-black/30 px-1 py-0.5 font-mono text-white/65">
                      chmod +x
                    </code>{" "}
                    nesse arquivo e selecione-o no Arcadia.
                  </p>
                  <div className="mb-2 flex justify-end">
                    <button
                      type="button"
                      onClick={() => void copy(wrapper, "wrapper")}
                      className="rounded-lg border border-white/10 px-3 py-1.5 text-[11px] text-white/65 transition-colors hover:bg-white/[0.08] hover:text-white"
                    >
                      {copied === "wrapper" ? "Wrapper copiado" : "Copiar wrapper"}
                    </button>
                  </div>
                  <CodeBlock value={wrapper} multiline />
                  <p className="mb-1 mt-4 text-[11px] font-medium text-white/65">
                    Permissão para ROMs e BIOS
                  </p>
                  <p className="mb-2 text-[11px] leading-relaxed text-white/40">
                    Dê acesso somente às pastas necessárias. Troque{" "}
                    <code className="rounded bg-black/30 px-1 py-0.5 font-mono text-white/65">
                      /path/to/roms
                    </code>{" "}
                    pelo caminho real e repita para a pasta do BIOS, se necessário.
                  </p>
                  <div className="flex items-center justify-end gap-2">
                    <CodeBlock value={filesystemCommand} />
                    <button
                      type="button"
                      onClick={() => void copy(filesystemCommand, "filesystem")}
                      className="shrink-0 rounded-lg border border-white/10 px-3 py-1.5 text-[11px] text-white/65 transition-colors hover:bg-white/[0.08] hover:text-white"
                    >
                      {copied === "filesystem" ? "Copiado" : "Copiar"}
                    </button>
                  </div>
                </section>
              )}

              <div className="rounded-xl border border-amber-300/15 bg-amber-400/[0.055] p-4">
                <p className="text-xs font-medium text-amber-100/85">Importante no Linux</p>
                <ul className="mt-2 space-y-1 text-[11px] leading-relaxed text-amber-100/60">
                  <li>
                    • O Arcadia apenas valida um executável local; ele não instala emuladores, BIOS,
                    firmware ou ROMs por você.
                  </li>
                  <li>
                    • AppImage precisa de permissão de execução; Flatpak precisa de um wrapper e de
                    permissões de filesystem para enxergar suas ROMs.
                  </li>
                  <li>
                    • Use BIOS/firmware obtidos legalmente e configure-os na próxima etapa do
                    assistente.
                  </li>
                </ul>
              </div>
            </>
          )}
        </div>

        <div className="flex flex-wrap items-center justify-end gap-2 border-t border-white/[0.07] px-6 py-4">
          {onManualBrowse && (
            <button
              type="button"
              onClick={() => void browse()}
              className="mr-auto rounded-lg border border-white/10 px-3 py-2 text-xs text-white/65 transition-colors hover:bg-white/[0.07] hover:text-white"
            >
              Explorar manualmente
            </button>
          )}
          <button
            type="button"
            onClick={dismiss}
            className="rounded-lg px-4 py-2 text-xs font-medium text-white/55 transition-colors hover:bg-white/[0.06] hover:text-white"
          >
            Voltar
          </button>
          {(onRefresh || onDone) && (
            <button
              type="button"
              onClick={() => {
                if (onRefresh) void refresh()
                else void finish()
              }}
              disabled={refreshing}
              className="rounded-lg bg-white px-4 py-2 text-xs font-semibold text-black transition-opacity disabled:cursor-not-allowed disabled:opacity-45"
            >
              {refreshing
                ? "Detectando…"
                : onRefresh
                  ? "Já instalei — detectar novamente"
                  : "Voltar à configuração"}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

function InstallSourceCard({
  title,
  description,
  action,
  onOpen,
  tone,
}: {
  title: string
  description: string
  action: string
  onOpen: () => void
  tone: "neutral"
}) {
  return (
    <div
      className={`rounded-xl border p-3 ${tone === "neutral" ? "border-white/[0.08] bg-white/[0.025]" : "border-white/10 bg-white/[0.04]"}`}
    >
      <p className="text-sm font-medium text-white/85">{title}</p>
      <p className="mt-1 min-h-9 text-[11px] leading-relaxed text-white/40">{description}</p>
      <button
        type="button"
        onClick={onOpen}
        className="mt-3 rounded-lg border border-white/10 px-3 py-1.5 text-[11px] text-white/65 transition-colors hover:bg-white/[0.08] hover:text-white"
      >
        {action} <span aria-hidden="true">↗</span>
      </button>
    </div>
  )
}

function CodeBlock({ value, multiline = false }: { value: string; multiline?: boolean }) {
  return (
    <pre
      className={`min-w-0 overflow-x-auto rounded-lg border border-black/20 bg-black/30 px-3 py-2 font-mono text-[11px] leading-relaxed text-white/65 ${multiline ? "whitespace-pre" : "whitespace-nowrap"}`}
    >
      <code>{value}</code>
    </pre>
  )
}

function CloseIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
    >
      <path d="M6 6l12 12M18 6L6 18" />
    </svg>
  )
}
