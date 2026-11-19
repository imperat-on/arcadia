# Captura a janela do Arcadia (Tauri) para validação visual.
# Uso: powershell -ExecutionPolicy Bypass -File tools/screenshot.ps1 [-Out caminho]
param(
    [string]$Out = "$env:TEMP\arcadia-shot.png"
)

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -Name User32 -Namespace Native -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
'@

$proc = Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowTitle -match "Arcadia" -and $_.MainWindowHandle -ne 0 } |
    Select-Object -First 1

if (-not $proc) {
    Write-Error "janela do Arcadia não encontrada"
    exit 1
}

[Native.User32]::ShowWindow($proc.MainWindowHandle, 9) | Out-Null
[Native.User32]::SetForegroundWindow($proc.MainWindowHandle) | Out-Null
Start-Sleep -Milliseconds 800

$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
$graphics = [System.Drawing.Graphics]::FromImage($bmp)
$graphics.CopyFromScreen($screen.Location, [System.Drawing.Point]::Empty, $screen.Size)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bmp.Dispose()

Write-Output $Out
