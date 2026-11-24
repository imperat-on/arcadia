#Requires -Version 5.1
# Gera o instalador Windows (Inno Setup) a partir do estágio portátil.
#
# O estágio precisa ter sido gerado por package-native-windows.ps1 (Arcadia.exe,
# arcadia-native.exe e, opcionalmente, gstreamer\). O instalador adiciona
# installed.marker para o launcher nativo selecionar o canal NSIS.
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
$Launcher = Join-Path $StageDir "Arcadia.exe"
if (-not (Test-Path -LiteralPath $Launcher -PathType Leaf)) {
    throw "estágio portátil sem Arcadia.exe; rode scripts\package-native-windows.ps1 antes de gerar o instalador"
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

# Marcador copiado pelo Inno Setup para {app}. O launcher nativo o encontra ao
# lado do Arcadia.exe e define ARCADIA_PACKAGED=1 (canal NSIS). O zip portátil
# não tem o marcador, então continua sendo tratado como portátil.
New-Item -ItemType Directory -Path $Output -Force | Out-Null
$marker = Join-Path $Output "installed.marker"
Write-TextFile -Path $marker -Content "installed`n"

$env:ARCADIA_VERSION = $Version
$env:ARCADIA_STAGE_DIR = $StageDir
$env:ARCADIA_OUTPUT_DIR = $Output
$env:ARCADIA_ICON = $Icon
$env:ARCADIA_INSTALLED_MARKER = $marker

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
