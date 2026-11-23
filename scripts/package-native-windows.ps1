#Requires -Version 5.1
# Empacotamento Windows do Arcadia (Rust + iced).
#
# Gera um .zip portátil em dist/native-windows/ com o arcadia-native.exe,
# boot.mp4, worker de torrent, README e, quando um runtime GStreamer é encontrado, as DLLs e
# plugins em gstreamer/. O SHA256SUMS fica ao lado do .zip.
#
# Uso (PowerShell 5.1+):
#   pwsh -File .\scripts\package-native-windows.ps1
#   pwsh -File .\scripts\package-native-windows.ps1 -SkipBuild
#   pwsh -File .\scripts\package-native-windows.ps1 -Output C:\tmp\arcadia
#   pwsh -File .\scripts\package-native-windows.ps1 -GStreamerRoot C:\gstreamer
#
# Variáveis:
#   ARCADIA_NATIVE_BIN       binário alternativo (útil com -SkipBuild)
#   ARCADIA_NATIVE_VERSION   sobrescreve a versão lida do workspace
#   CARGO_TARGET_DIR         respeitado no build e na busca do binário
#   GSTREAMER_ROOT_X86_64    runtime GStreamer MSVC x86_64; também aceita
#                            GSTREAMER_ROOT e GSTREAMER_1_0_ROOT_MSVC_X86_64
#
# GStreamer no pacote: copia só o runtime (bin\*.dll, lib\gstreamer-1.0\*.dll
# e libexec\gstreamer-1.0\gst-plugin-scanner.exe), nunca headers, .lib nem o
# SDK inteiro. Sem GSTREAMER_ROOT_* o pacote sai sem GStreamer e o README
# instrui a instalar o runtime do sistema; o código Rust não muda.
#
# O .zip usa entradas ordenadas e timestamp fixo para que o mesmo conteúdo
# produza um arquivo estável, independente do relógio do build.

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string]$Output = "",
    [string]$GStreamerRoot = "",
    [string]$Version = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$PackageArch = "x86_64"

function Stop-WithError {
    param([string]$Message)

    Write-Host "erro: $Message" -ForegroundColor Red
    exit 1
}

function Write-WarningLine {
    param([string]$Message)

    Write-Host "aviso: $Message" -ForegroundColor Yellow
}

function Write-TextFile {
    param(
        [string]$Path,
        [string]$Content
    )

    $utf8 = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($Path, $Content, $utf8)
}

function Get-WorkspaceVersion {
    param([string]$CargoToml)

    $inSection = $false
    foreach ($line in Get-Content -LiteralPath $CargoToml) {
        if ($line -match '^\s*\[workspace\.package\]\s*$') {
            $inSection = $true
            continue
        }
        if ($line -match '^\s*\[') {
            $inSection = $false
            continue
        }
        if ($inSection -and $line -match '^\s*version\s*=\s*"([^"]+)"') {
            return $Matches[1]
        }
    }
    return $null
}

function Get-PackageArch {
    $raw = $env:PROCESSOR_ARCHITECTURE
    if ([string]::IsNullOrWhiteSpace($raw)) {
        $raw = "AMD64"
    }
    switch ($raw.ToUpperInvariant()) {
        "AMD64" { return "x86_64" }
        "ARM64" { return "arm64" }
        "X86" { return "x86" }
        default { return $raw.ToLowerInvariant() }
    }
}

# Aceita tanto a raiz do runtime (C:\gstreamer, com bin\ e lib\gstreamer-1.0)
# quanto o layout do instalador oficial antigo (C:\gstreamer\1.0\msvc_x86_64)
# e a raiz pai (C:\gstreamer). A DLL core é o marcador do runtime MSVC.
function Resolve-GStreamerRoot {
    param([string]$Path)

    $archName = if ($PackageArch -eq "x86_64") { "x86_64" } else { $PackageArch }
    $candidates = @(
        $Path,
        (Join-Path $Path "1.0\msvc_$archName"),
        (Join-Path $Path "msvc_$archName")
    )

    foreach ($candidate in $candidates) {
        $bin = Join-Path $candidate "bin"
        $plugins = Join-Path $candidate "lib\gstreamer-1.0"
        if ((Test-Path -LiteralPath (Join-Path $bin "gstreamer-1.0-0.dll") -PathType Leaf) -and
            (Test-Path -LiteralPath $plugins -PathType Container)) {
            return $candidate
        }
    }
    return $null
}

# Zip sem depender do Compress-Archive: entradas ordenadas, separador "/" e
# timestamp fixo (2020-01-01) para o arquivo não variar a cada execução.
function New-PortableZip {
    param(
        [string]$SourceDirectory,
        [string]$DestinationPath,
        [string]$BaseName
    )

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem

    if (Test-Path -LiteralPath $DestinationPath) {
        Remove-Item -LiteralPath $DestinationPath -Force
    }

    $sourceFull = [System.IO.Path]::GetFullPath($SourceDirectory)
    $fixedTime = [DateTimeOffset]::new(2020, 1, 1, 0, 0, 0, [TimeSpan]::Zero)

    $stream = [System.IO.File]::Open($DestinationPath, [System.IO.FileMode]::CreateNew)
    try {
        $archive = [System.IO.Compression.ZipArchive]::new(
            $stream,
            [System.IO.Compression.ZipArchiveMode]::Create,
            $true
        )
        try {
            $files = Get-ChildItem -LiteralPath $sourceFull -Recurse -File | Sort-Object FullName
            foreach ($file in $files) {
                $relative = $file.FullName.Substring($sourceFull.Length)
                $relative = $relative.TrimStart([char[]]@('\', '/'))
                $relative = $relative.Replace('\', '/')
                $entry = $archive.CreateEntry(
                    "$BaseName/$relative",
                    [System.IO.Compression.CompressionLevel]::Optimal
                )
                $entry.LastWriteTime = $fixedTime
                $entryStream = $entry.Open()
                try {
                    $input = [System.IO.File]::OpenRead($file.FullName)
                    try {
                        $input.CopyTo($entryStream)
                    } finally {
                        $input.Dispose()
                    }
                } finally {
                    $entryStream.Dispose()
                }
            }
        } finally {
            $archive.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
}

# --- Versão e diretórios ----------------------------------------------------

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = $env:ARCADIA_NATIVE_VERSION
}
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = Get-WorkspaceVersion -CargoToml (Join-Path $RepoRoot "Cargo.toml")
}
if ([string]::IsNullOrWhiteSpace($Version)) {
    Stop-WithError "não foi possível ler a versão em $RepoRoot\Cargo.toml; defina ARCADIA_NATIVE_VERSION=<versão> ou use -Version"
}

$PackageArch = Get-PackageArch
$PackageName = "arcadia-native-$Version-windows-$PackageArch"

if ([string]::IsNullOrWhiteSpace($Output)) {
    $Output = Join-Path $RepoRoot "dist\native-windows"
} elseif (-not [System.IO.Path]::IsPathRooted($Output)) {
    $Output = Join-Path $RepoRoot $Output
}
$Output = [System.IO.Path]::GetFullPath($Output)

$TargetDir = $env:CARGO_TARGET_DIR
if ([string]::IsNullOrWhiteSpace($TargetDir)) {
    $TargetDir = Join-Path $RepoRoot "target"
} elseif (-not [System.IO.Path]::IsPathRooted($TargetDir)) {
    $TargetDir = Join-Path $RepoRoot $TargetDir
}
$TargetDir = [System.IO.Path]::GetFullPath($TargetDir)

$Bin = $env:ARCADIA_NATIVE_BIN
if ([string]::IsNullOrWhiteSpace($Bin)) {
    $Bin = Join-Path $TargetDir "release\arcadia-native.exe"
} elseif (-not [System.IO.Path]::IsPathRooted($Bin)) {
    $Bin = Join-Path $RepoRoot $Bin
}
$Bin = [System.IO.Path]::GetFullPath($Bin)

# --- Build ------------------------------------------------------------------

if (-not $SkipBuild) {
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($null -eq $cargo) {
        Stop-WithError "cargo não encontrado no PATH; instale o toolchain Rust MSVC (https://rustup.rs)"
    }
    Write-Host "==> cargo build --locked --release -p arcadia-native"
    $env:CARGO_TARGET_DIR = $TargetDir
    Push-Location $RepoRoot
    try {
        & cargo build --locked --release -p arcadia-native
        if ($LASTEXITCODE -ne 0) {
            Stop-WithError "cargo build falhou (exit $LASTEXITCODE)"
        }
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path -LiteralPath $Bin -PathType Leaf)) {
    Write-Host "erro: binário nativo não encontrado em $Bin" -ForegroundColor Red
    if ($SkipBuild) {
        Write-Host "  -SkipBuild foi usado; compile antes com:" -ForegroundColor Red
        Write-Host "    cargo build --locked --release -p arcadia-native" -ForegroundColor Red
    }
    Write-Host "  ou aponte ARCADIA_NATIVE_BIN para outro executável" -ForegroundColor Red
    exit 1
}

# --- Staging ----------------------------------------------------------------

$Stage = Join-Path $Output $PackageName
if (Test-Path -LiteralPath $Stage) {
    Remove-Item -LiteralPath $Stage -Recurse -Force
}
New-Item -ItemType Directory -Path $Stage -Force | Out-Null

Copy-Item -LiteralPath $Bin -Destination (Join-Path $Stage "arcadia-native.exe")
Write-Host "==> binário: $Bin"

# O app procura boot.mp4 ao lado do executável e em resources\boot.mp4.
$Boot = Join-Path $RepoRoot "boot.mp4"
if (Test-Path -LiteralPath $Boot -PathType Leaf) {
    Copy-Item -LiteralPath $Boot -Destination (Join-Path $Stage "boot.mp4")
    Write-Host "==> boot.mp4: $Boot"
} else {
    Write-WarningLine "boot.mp4 não encontrado; o app abre sem a tela de abertura"
}

$Worker = Join-Path $Stage "resources\torrent_rpc"
New-Item -ItemType Directory -Path $Worker -Force | Out-Null
foreach ($file in @("main.py", "torrent_downloader.py")) {
    Copy-Item -LiteralPath (Join-Path $RepoRoot "resources\torrent_rpc\$file") -Destination $Worker
}

# --- GStreamer (opcional) ---------------------------------------------------

$GstRootRequested = $GStreamerRoot
if ([string]::IsNullOrWhiteSpace($GstRootRequested)) {
    foreach ($name in @("GSTREAMER_ROOT_X86_64", "GSTREAMER_ROOT", "GSTREAMER_1_0_ROOT_MSVC_X86_64")) {
        $value = [Environment]::GetEnvironmentVariable($name)
        if (-not [string]::IsNullOrWhiteSpace($value)) {
            $GstRootRequested = $value
            break
        }
    }
}

$GstRoot = $null
if (-not [string]::IsNullOrWhiteSpace($GstRootRequested)) {
    if (-not [System.IO.Path]::IsPathRooted($GstRootRequested)) {
        $GstRootRequested = Join-Path $RepoRoot $GstRootRequested
    }
    $GstRoot = Resolve-GStreamerRoot -Path ([System.IO.Path]::GetFullPath($GstRootRequested))
    if ($null -eq $GstRoot) {
        Stop-WithError @"
GStreamer não encontrado em $GstRootRequested.
  esperado bin\gstreamer-1.0-0.dll e lib\gstreamer-1.0\*.dll (runtime MSVC $PackageArch).
  instale o runtime oficial 1.28 (ou o SDK completo) ou remova GSTREAMER_ROOT_*.
"@
    }
}

$GstBundled = $false
$GstDllCount = 0
$GstPluginCount = 0
$GstBytes = 0

if ($null -ne $GstRoot) {
    $gstBin = Join-Path $GstRoot "bin"
    $gstPlugins = Join-Path $GstRoot "lib\gstreamer-1.0"
    $stageGst = Join-Path $Stage "gstreamer"
    $stageGstBin = Join-Path $stageGst "bin"
    $stageGstPlugins = Join-Path $stageGst "lib\gstreamer-1.0"
    New-Item -ItemType Directory -Path $stageGstBin, $stageGstPlugins -Force | Out-Null

    $binDlls = @(Get-ChildItem -LiteralPath $gstBin -Filter "*.dll" -File)
    if ($binDlls.Count -eq 0) {
        Stop-WithError "nenhuma DLL em $gstBin; o runtime GStreamer está incompleto"
    }
    foreach ($dll in $binDlls) {
        Copy-Item -LiteralPath $dll.FullName -Destination $stageGstBin
        $GstBytes += $dll.Length
    }

    $pluginDlls = @(Get-ChildItem -LiteralPath $gstPlugins -Filter "*.dll" -File)
    if ($pluginDlls.Count -eq 0) {
        Stop-WithError "nenhum plugin em $gstPlugins; o runtime GStreamer está incompleto"
    }
    foreach ($dll in $pluginDlls) {
        Copy-Item -LiteralPath $dll.FullName -Destination $stageGstPlugins
        $GstBytes += $dll.Length
    }

    # Sem o scanner o GStreamer não consegue popular o registry de plugins em
    # uma máquina sem GStreamer instalado. O caminho oficial é
    # libexec\gstreamer-1.0\gst-plugin-scanner.exe.
    $scanner = Join-Path $GstRoot "libexec\gstreamer-1.0\gst-plugin-scanner.exe"
    if (Test-Path -LiteralPath $scanner -PathType Leaf) {
        $stageScannerDir = Join-Path $stageGst "libexec\gstreamer-1.0"
        New-Item -ItemType Directory -Path $stageScannerDir -Force | Out-Null
        Get-ChildItem -LiteralPath (Split-Path -Parent $scanner) -File |
            ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $stageScannerDir }
        $GstBytes += (Get-Item -LiteralPath $scanner).Length
    } else {
        $scannerInBin = Join-Path $gstBin "gst-plugin-scanner.exe"
        if (Test-Path -LiteralPath $scannerInBin -PathType Leaf) {
            Copy-Item -LiteralPath $scannerInBin -Destination $stageGstBin
            $GstBytes += (Get-Item -LiteralPath $scannerInBin).Length
        } else {
            Write-WarningLine "gst-plugin-scanner.exe não encontrado; em máquinas sem GStreamer os plugins podem não carregar"
        }
    }

    $GstDllCount = $binDlls.Count
    $GstPluginCount = $pluginDlls.Count
    $GstBundled = $true
    Write-Host ("==> GStreamer: {0} DLLs, {1} plugins, {2:N1} MB de {3}" -f `
            $GstDllCount, $GstPluginCount, ($GstBytes / 1MB), $GstRoot)
}

# --- Launcher e README ------------------------------------------------------

if ($GstBundled) {
    $launcherLines = @(
        "@echo off",
        "setlocal",
        "set `"HERE=%~dp0`"",
        "set `"PATH=%HERE%gstreamer\bin;%PATH%`"",
        "set `"GST_PLUGIN_PATH=%HERE%gstreamer\lib\gstreamer-1.0`"",
        "set `"GST_PLUGIN_PATH_1_0=%HERE%gstreamer\lib\gstreamer-1.0`"",
        "set `"GST_PLUGIN_SYSTEM_PATH_1_0=%HERE%gstreamer\lib\gstreamer-1.0`"",
        "if exist `"%HERE%gstreamer\libexec\gstreamer-1.0\gst-plugin-scanner.exe`" set `"GST_PLUGIN_SCANNER=%HERE%gstreamer\libexec\gstreamer-1.0\gst-plugin-scanner.exe`"",
        "if exist `"%HERE%gstreamer\bin\gst-plugin-scanner.exe`" set `"GST_PLUGIN_SCANNER=%HERE%gstreamer\bin\gst-plugin-scanner.exe`"",
        "start `"`" /D `"%HERE%`" `"%HERE%arcadia-native.exe`" %*"
    )
    $launcher = ($launcherLines -join "`r`n") + "`r`n"
    Write-TextFile -Path (Join-Path $Stage "run-arcadia-native.cmd") -Content $launcher
}

if ($GstBundled) {
    $gstContent = @'
- `run-arcadia-native.cmd` — launcher que configura `PATH`, `GST_PLUGIN_PATH`
  e `GST_PLUGIN_SCANNER` para a pasta `gstreamer\`;
- `gstreamer\bin\*.dll` — bibliotecas do runtime GStreamer;
- `gstreamer\lib\gstreamer-1.0\*.dll` — plugins (base, good, bad, libav etc.);
- `gstreamer\libexec\gstreamer-1.0\gst-plugin-scanner.exe` — scanner de plugins,
  quando presente no runtime;
'@
    $runInstructions = @'
Dê duplo clique em `run-arcadia-native.cmd` (ou execute-o no terminal). Ele
aponta as variáveis para o runtime embarcado e inicia o app.

Não abra `arcadia-native.exe` direto: as DLLs do GStreamer ficam em
`gstreamer\bin` e não são encontradas sem essas variáveis.
'@
    $gstRequirements = @'
O runtime GStreamer 1.28 MSVC x86_64 vai embutido em `gstreamer\`. Não é
preciso instalar GStreamer na máquina.
'@
} else {
    $gstContent = @'
- **Sem GStreamer embutido** — o build não encontrou
  `GSTREAMER_ROOT_X86_64`/`GSTREAMER_ROOT`; veja "Dependências de runtime".
'@
    $runInstructions = @'
Instale o runtime do GStreamer (veja abaixo) e execute:

```powershell
$gst = "C:\gstreamer\1.0\msvc_x86_64"
$env:PATH = "$gst\bin;$env:PATH"
$env:GST_PLUGIN_PATH = "$gst\lib\gstreamer-1.0"
$env:GST_PLUGIN_SYSTEM_PATH_1_0 = "$gst\lib\gstreamer-1.0"
$env:GST_PLUGIN_SCANNER = "$gst\libexec\gstreamer-1.0\gst-plugin-scanner.exe"
.\arcadia-native.exe
```
'@
    $gstRequirements = @'
Instale o runtime oficial **GStreamer 1.28 MSVC x86_64** (o SDK de
desenvolvimento não é necessário para rodar) e aponte as variáveis acima.
Não misture runtime MinGW com um binário MSVC. Download:

<https://gstreamer.freedesktop.org/data/pkg/windows/>

O GStreamer não é embutido porque o build foi feito sem `GSTREAMER_ROOT_*`.
'@
}

$readme = @'
# Arcadia Native @@VERSION@@ — Windows @@ARCH@@

Arcadia em Rust + iced.

## Conteúdo

- `arcadia-native.exe` — binário do aplicativo;
- `boot.mp4` — vídeo de abertura, procurado ao lado do executável;
- `resources\torrent_rpc\` — worker de download por torrent (Python);
@@GST_CONTENT@@
- `README.md` — este arquivo.

## Como rodar

@@RUN_INSTRUCTIONS@@

O `boot.mp4` é procurado ao lado do `arcadia-native.exe` e em
`resources\boot.mp4`. Sem ele, a interface abre direto.

## Dependências de runtime

@@GST_REQUIREMENTS@@

Também é necessário o Microsoft Visual C++ Redistributable 2015–2022 (x64):
o binário MSVC e as DLLs do GStreamer usam o runtime C++ da Microsoft. O
Windows 10/11 costuma tê-lo, mas não é garantido em uma máquina limpa.

Os downloads por torrent usam Python com `libtorrent`.

Rede é opcional para abrir a biblioteca local; capas, trailers e conta
dependem do backend configurado.

## Verificar o download

```powershell
Get-FileHash -Algorithm SHA256 .\@@ZIP_NAME@@
Get-Content .\SHA256SUMS
```

## Limitações

- Pacote portátil: não há instalador, atalho no menu Iniciar nem auto-update.
- Não há assinatura de código; o SmartScreen pode avisar na primeira execução.
- O GStreamer, quando embarcado, é o runtime oficial MSVC x86_64 1.28
  (LGPL). Se você redistribuir este zip, revise as licenças dos plugins
  incluídos.
- Sem `gstreamer\`, o app depende do GStreamer instalado na máquina, na mesma
  ABI (MSVC x86_64) e em versão compatível.
- Este pacote é x86_64; não há build ARM64 nesta etapa.
'@

$zipName = "$PackageName.zip"
$readme = $readme.Replace("@@VERSION@@", $Version)
$readme = $readme.Replace("@@ARCH@@", $PackageArch)
$readme = $readme.Replace("@@GST_CONTENT@@", $gstContent.TrimEnd())
$readme = $readme.Replace("@@RUN_INSTRUCTIONS@@", $runInstructions.TrimEnd())
$readme = $readme.Replace("@@GST_REQUIREMENTS@@", $gstRequirements.TrimEnd())
$readme = $readme.Replace("@@ZIP_NAME@@", $zipName)
$readme = $readme.TrimEnd() + "`n"
Write-TextFile -Path (Join-Path $Stage "README.md") -Content $readme

# --- Zip determinístico + checksum -----------------------------------------

New-Item -ItemType Directory -Path $Output -Force | Out-Null
$ZipPath = Join-Path $Output $zipName
Write-Host "==> gerando $ZipPath"
New-PortableZip -SourceDirectory $Stage -DestinationPath $ZipPath -BaseName $PackageName

$hash = (Get-FileHash -Algorithm SHA256 -Path $ZipPath).Hash.ToLowerInvariant()
$sumPath = Join-Path $Output "SHA256SUMS"
Write-TextFile -Path $sumPath -Content ("{0}  {1}`n" -f $hash, $zipName)

# --- Relatório --------------------------------------------------------------

$stageFiles = @(Get-ChildItem -LiteralPath $Stage -Recurse -File)
$stageBytes = ($stageFiles | Measure-Object -Property Length -Sum).Sum
$zipItem = Get-Item -LiteralPath $ZipPath
$sumItem = Get-Item -LiteralPath $sumPath

Write-Host ""
Write-Host "==> artefatos"
Write-Host ("    {0,8:N1} MB  {1}" -f ($zipItem.Length / 1MB), $zipItem.FullName)
Write-Host ("    {0,8:N1} MB  {1}" -f ($sumItem.Length / 1MB), $sumItem.FullName)
Write-Host ("==> conteúdo: {0} arquivos, {1:N1} MB (GStreamer: {2})" -f `
        $stageFiles.Count, ($stageBytes / 1MB),
        $(if ($GstBundled) { "$GstDllCount DLLs + $GstPluginCount plugins" } else { "não embarcado" }))

exit 0
