# =============================================================================
# TitanVault — Installer for Windows
# SecuryBlack Storage, Backup and Disaster Recovery Agent
# =============================================================================

[CmdletBinding()]
param(
    [string]$Version = ""
)

# Windows Server 2019 / PS 5.1: force TLS 1.2+ and allow dot-sourcing the shared lib.
# Must stay below param(): nothing but comments may precede it.
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls11 -bor [Net.SecurityProtocolType]::Tls
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force -ErrorAction SilentlyContinue

$ErrorActionPreference = "Stop"
$ServiceName = "TitanVault"
$BinaryName = "titanvault.exe"
$InstallDir = "$env:ProgramFiles\SecuryBlack\TitanVault"
$DataDir = "$env:ProgramData\titanvault"
$Repo = "securyblack/titan-vault"

# Descargar librería compartida de sb-agent-core
$SbAgentLabel = "titanvault"
$LibUrl = "https://raw.githubusercontent.com/securyblack/sb-agent-core/master/scripts/install-lib.ps1"
$LibPath = Join-Path ([System.IO.Path]::GetTempPath()) "sb-agent-core-install-lib.ps1"
Invoke-WebRequest -Uri $LibUrl -OutFile $LibPath -UseBasicParsing
. $LibPath

Assert-SbAdmin

# Las releases solo publican el binario x86_64 para Windows.
$Target = "x86_64-pc-windows-msvc"
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = Get-SbLatestVersion -GithubRepo $Repo
}

Write-SbInfo "Installing TitanVault $Version for $Target..."

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
    # Descarga, verifica el .sha256, para el servicio anterior si existe y copia el binario.
    $ZipPath = Get-SbReleaseAsset -GithubRepo $Repo -Version $Version -AssetName "titanvault-$Target.zip" -TmpDir $TempDir
    Install-SbBinaryFromZip -ZipPath $ZipPath -BinaryName $BinaryName -InstallDir $InstallDir -ServiceName $ServiceName

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

    # Registrar y arrancar el Windows Service
    $BinaryPath = Join-Path $InstallDir $BinaryName
    Register-SbWindowsService -ServiceName $ServiceName -DisplayName "TitanVault Backup Agent" `
        -BinaryPath $BinaryPath `
        -Description "SecuryBlack TitanVault backup agent. See https://github.com/$Repo"

    Write-Host "TitanVault has been successfully installed and started!" -ForegroundColor Green
    Write-Host "Run '$BinaryPath tui' to open the interactive configuration." -ForegroundColor Yellow
} finally {
    Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
}
