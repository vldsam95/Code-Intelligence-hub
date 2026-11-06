param(
  [string]$Hub = "",
  [string]$InstallToken = "",
  [switch]$LocalOnly,
  [string]$Clients = "codex,claude,hermes"
)

$ErrorActionPreference = "Stop"
if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
  throw "WSL2 is required. Install it first with: wsl --install"
}
& wsl.exe sh -lc "command -v systemctl >/dev/null 2>&1 && systemctl --version >/dev/null 2>&1"
if ($LASTEXITCODE -ne 0) {
  throw "The WSL distribution must support systemd. Enable systemd in /etc/wsl.conf and restart WSL."
}
$windowsPath = (Resolve-Path $PSScriptRoot).Path
$linuxPath = (& wsl.exe wslpath -a -u $windowsPath).Trim()
if (-not $linuxPath) { throw "Could not translate the bundle path into WSL." }
$agentUser = (& wsl.exe sh -lc "id -un").Trim()
if ($agentUser -notmatch '^[A-Za-z0-9._-]+$') { throw "Could not identify the default WSL user." }
$installArgs = @("$linuxPath/install-offline.sh", "--clients", $Clients, "--agent-user", $agentUser)
if ($Hub) { $installArgs += @("--hub", $Hub) }
if ($InstallToken -and -not $LocalOnly) {
  $installArgs += @("--install-token", $InstallToken)
} else {
  $installArgs += "--local-only"
}
& wsl.exe -u root -- @installArgs
if ($LASTEXITCODE -ne 0) { throw "Code Intelligence installation inside WSL failed." }
& wsl.exe sh -lc "codeintel doctor"
Write-Host "Code Intelligence Worker installed inside WSL. Run codeintel init only for a project explicitly selected by the owner."
