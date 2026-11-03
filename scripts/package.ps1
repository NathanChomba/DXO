# Build a release of DXO and its installer.
#   .\scripts\package.ps1                       -> dist\DXO\ (the app) + dist\DXO-Setup-<version>.exe
#   .\scripts\package.ps1 -DownloadUrl <url>    -> also writes dist\latest.json for the update check
# The installer needs Inno Setup 6 (https://jrsoftware.org/isdl.php). Without it
# you still get dist\DXO, which runs as is (zip it to share).
param([string]$DownloadUrl = "", [string]$Notes = "")
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $version = (Select-Xml -Path ui\DXO.UI\DXO.UI.csproj -XPath "//Version").Node.InnerText
    Write-Host "DXO $version"

    cargo test --workspace
    cargo build --release --workspace

    if (Test-Path dist) { Remove-Item dist -Recurse -Force }
    dotnet publish ui\DXO.UI\DXO.UI.csproj -c Release -p:Platform=x64 -o dist\DXO
    Copy-Item target\release\dxo.exe dist\DXO\ -ErrorAction SilentlyContinue  # command-line tool, handy for diagnostics

    $iscc = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($iscc) {
        & $iscc "/DMyVersion=$version" installer\dxo.iss
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed ($LASTEXITCODE)" }
    } else {
        Write-Warning "Inno Setup 6 not found: skipped the installer. dist\DXO is ready to run."
    }

    if ($DownloadUrl) {
        # Upload this next to the installer (a secret gist works) and put its raw
        # address in DXO > Settings > Updates.
        @{ version = $version; url = $DownloadUrl; notes = $Notes } | ConvertTo-Json | Set-Content -Encoding UTF8 dist\latest.json
        Write-Host "wrote dist\latest.json"
    }
} finally { Pop-Location }
