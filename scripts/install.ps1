# =============================================================================
# TitanVault — Installer for Windows
# SecuryBlack Storage, Backup and Disaster Recovery Agent
# =============================================================================

[CmdletBinding()]
param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$ServiceName = "TitanVault"
$BinaryName = "titanvault.exe"
$InstallDir = "$env:ProgramFiles\SecuryBlack\TitanVault"
$DataDir = "$env:ProgramData\titanvault"
$Repo = "securyblack/titan-vault"

# Descargar librería compartida de sb-agent-core
$LibUrl = "https://raw.githubusercontent.com/securyblack/sb-agent-core/main/scripts/install-lib.ps1"
$LibPath = "$env:TEMP\sb_install_lib.ps1"
Invoke-WebRequest -Uri $LibUrl -OutFile $LibPath
. $LibPath
Remove-Item -Force $LibPath

sb_require_admin

$Target = "x86_64-pc-windows-msvc"
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = sb_fetch_latest_version $Repo
}

Write-Host "Installing TitanVault $Version for $Target..." -ForegroundColor Cyan

$TempDir = Join-Path $env:TEMP ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
    $AssetUrl = "https://github.com/$Repo/releases/download/$Version/titanvault-$Target.zip"
    $ZipPath = Join-Path $TempDir "asset.zip"
    
    sb_download_and_verify $AssetUrl $ZipPath
    
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }
    
    Expand-Archive -Path $ZipPath -DestinationPath $TempDir -Force
    Copy-Item -Path (Join-Path $TempDir $BinaryName) -Destination (Join-Path $InstallDir $BinaryName) -Force
    
    # Crear config.toml si no existe
    if (-not (Test-Path $DataDir)) {
        New-Item -ItemType Directory -Path $DataDir -Force | Out-Null
    }
    $ConfigFile = Join-Path $DataDir "config.toml"
    if (-not (Test-Path $ConfigFile)) {
        @"
version = "0.1.0"
agent_name = "titanvault"
mode = "standalone"

[schedule]
enabled = true
cron = "0 2 * * *"
hourly_cron = "0 * * * *"

[retention]
keep_hourly = 24
keep_daily = 7
keep_weekly = 4
keep_monthly = 12
keep_yearly = 3

[crypto]
enabled = false
algorithm = "chacha20-poly1305"

[sources]
databases = []
filesystems = []

[targets]
"@ | Set-Content -Path $ConfigFile -Encoding UTF8
    }

    # Registrar Windows Service
    $BinaryPath = Join-Path $InstallDir $BinaryName
    sb_register_windows_service $ServiceName "TitanVault Backup Agent" $BinaryPath
    Start-Service -Name $ServiceName
    
    Write-Host "TitanVault has been successfully installed and started!" -ForegroundColor Green
    Write-Host "Run '$BinaryPath tui' to open the interactive configuration." -ForegroundColor Yellow
} finally {
    Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
}
