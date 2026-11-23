#Requires -Version 5.1
# Gera o instalador Windows (Inno Setup) a partir do estágio portátil.
#
# Uso:
#   pwsh -File .\scripts\package-native-windows-setup.ps1
#   pwsh -File .\scripts\package-native-windows-setup.ps1 -Version 2.0.0
#   pwsh -File .\scripts\package-native-windows-setup.ps1 -StageDir dist\native-windows\arcadia-native-2.0.0-windows-x86_64
#
# Requer Inno Setup 6 (`choco install innosetup -y --no-progress`).

[CmdletBinding()]
param(
    [string]$Version = "",
    [string]$StageDir = "",
    [string]$Output = "",
    [string]$InnoCompiler = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot

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

function Write-TextFile {
    param(
        [string]$Path,
        [string]$Content
    )

    $utf8 = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($Path, $Content, $utf8)
}

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = $env:ARCADIA_NATIVE_VERSION
}
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = Get-WorkspaceVersion -CargoToml (Join-Path $RepoRoot "Cargo.toml")
}
if ([string]::IsNullOrWhiteSpace($Version)) {
    throw "não foi possível ler a versão do workspace; use -Version"
}

if ([string]::IsNullOrWhiteSpace($Output)) {
    $Output = Join-Path $RepoRoot "dist\native-windows"
} elseif (-not [System.IO.Path]::IsPathRooted($Output)) {
    $Output = Join-Path $RepoRoot $Output
}
$Output = [System.IO.Path]::GetFullPath($Output)

if ([string]::IsNullOrWhiteSpace($StageDir)) {
    $StageDir = Join-Path $Output "arcadia-native-$Version-windows-x86_64"
} elseif (-not [System.IO.Path]::IsPathRooted($StageDir)) {
    $StageDir = Join-Path $RepoRoot $StageDir
}
$StageDir = [System.IO.Path]::GetFullPath($StageDir)

$Bin = Join-Path $StageDir "arcadia-native.exe"
if (-not (Test-Path -LiteralPath $Bin -PathType Leaf)) {
    throw "estágio portátil sem arcadia-native.exe: $StageDir"
}
$Icon = Join-Path $RepoRoot "crates\arcadia-native\assets\icons\icon.ico"
if (-not (Test-Path -LiteralPath $Icon -PathType Leaf)) {
    throw "ícone .ico não encontrado: $Icon"
}

# ISCC: aceita o caminho explícito, o PATH ou as instalações padrão do chocolatey.
$candidates = @()
if (-not [string]::IsNullOrWhiteSpace($InnoCompiler)) {
    $candidates += $InnoCompiler
}
$command = Get-Command iscc.exe -ErrorAction SilentlyContinue
if ($null -ne $command) {
    $candidates += $command.Source
}
$candidates += @(
    "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
    "C:\Program Files\Inno Setup 6\ISCC.exe"
)
$Iscc = $candidates |
    Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } |
    Select-Object -First 1
if ($null -eq $Iscc) {
    throw "Inno Setup 6 (ISCC.exe) não encontrado; instale com: choco install innosetup -y --no-progress"
}

# Launcher do app instalado. Diferente do portátil:
# - ARCADIA_PACKAGED=1 faz o updater detectar o canal NSIS;
# - quando o GStreamer foi embarcado, aponta PATH/GST_* para gstreamer\.
$installedLauncher = Join-Path $Output "run-arcadia-native-installed.cmd"
$launcherLines = @(
    "@echo off",
    "setlocal",
    "set `"HERE=%~dp0`""
)
$gstDll = Join-Path $StageDir "gstreamer\bin\gstreamer-1.0-0.dll"
if (Test-Path -LiteralPath $gstDll -PathType Leaf) {
    $launcherLines += @(
        "set `"PATH=%HERE%gstreamer\bin;%PATH%`"",
        "set `"GST_PLUGIN_PATH=%HERE%gstreamer\lib\gstreamer-1.0`"",
        "set `"GST_PLUGIN_PATH_1_0=%HERE%gstreamer\lib\gstreamer-1.0`"",
        "set `"GST_PLUGIN_SYSTEM_PATH_1_0=%HERE%gstreamer\lib\gstreamer-1.0`"",
        "if exist `"%HERE%gstreamer\libexec\gstreamer-1.0\gst-plugin-scanner.exe`" set `"GST_PLUGIN_SCANNER=%HERE%gstreamer\libexec\gstreamer-1.0\gst-plugin-scanner.exe`"",
        "if exist `"%HERE%gstreamer\bin\gst-plugin-scanner.exe`" set `"GST_PLUGIN_SCANNER=%HERE%gstreamer\bin\gst-plugin-scanner.exe`""
    )
}
$launcherLines += @(
    "set `"ARCADIA_PACKAGED=1`"",
    "start `"`" /D `"%HERE%`" `"%HERE%arcadia-native.exe`" %*"
)
$launcher = ($launcherLines -join "`r`n") + "`r`n"
Write-TextFile -Path $installedLauncher -Content $launcher

$env:ARCADIA_VERSION = $Version
$env:ARCADIA_STAGE_DIR = $StageDir
$env:ARCADIA_OUTPUT_DIR = $Output
$env:ARCADIA_ICON = $Icon
$env:ARCADIA_LAUNCHER = $installedLauncher

Write-Host "==> Inno Setup: $Iscc"
Write-Host "==> estágio:    $StageDir"
& $Iscc "/Qp" (Join-Path $RepoRoot "scripts\arcadia-native.iss")
if ($LASTEXITCODE -ne 0) {
    throw "ISCC falhou com exit $LASTEXITCODE"
}

$setup = Join-Path $Output "Arcadia_${Version}_x64-setup.exe"
if (-not (Test-Path -LiteralPath $setup -PathType Leaf)) {
    throw "instalador não foi gerado: $setup"
}

# SHA256SUMS do diretório: zip portátil + instalador.
$files = Get-ChildItem -LiteralPath $Output -File |
    Where-Object { $_.Extension -in ".zip", ".exe" } |
    Sort-Object Name
$lines = foreach ($file in $files) {
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $file.FullName).Hash.ToLowerInvariant()
    "{0}  {1}" -f $hash, $file.Name
}
Write-TextFile -Path (Join-Path $Output "SHA256SUMS") -Content (($lines -join "`n") + "`n")

Write-Host "==> instalador: $setup"
Write-Host ("    {0:N1} MB" -f ((Get-Item -LiteralPath $setup).Length / 1MB))
