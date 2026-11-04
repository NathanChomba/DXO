# Build a release of DXO, a portable zip, and (if Inno Setup is present) an installer.
#   .\scripts\package.ps1                       -> dist\DXO\, dist\DXO-<ver>-portable.zip [+ dist\DXO-Setup-<ver>.exe]
#   .\scripts\package.ps1 -SelfContained        -> bundle the .NET runtime (bigger; needs a one-time ~150 MB NuGet download)
#   .\scripts\package.ps1 -DownloadUrl <url>    -> also writes dist\latest.json for the update check
# Default is framework-dependent: small, builds offline, and the reviewer installs
# the .NET 10 Desktop Runtime once (https://dotnet.microsoft.com/download/dotnet/10.0).
# The portable zip always works (send it; they unzip and run DXO.UI.exe). The
# installer additionally needs Inno Setup 6 (https://jrsoftware.org/isdl.php).
param([switch]$SelfContained, [string]$DownloadUrl = "", [string]$Notes = "")
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $version = (Select-Xml -Path ui\DXO.UI\DXO.UI.csproj -XPath "//Version").Node.InnerText
    Write-Host "DXO $version"

    # DXO can't be rebuilt while it's running (it locks its own exe). Stop it first.
    $running = Get-Process DXO.UI -ErrorAction SilentlyContinue
    if ($running) {
        Write-Host "Closing running DXO.UI..."
        $running | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
        if (Get-Process DXO.UI -ErrorAction SilentlyContinue) {
            throw "DXO is still running (it runs as administrator, so this script can't stop it). Exit it from the tray (right-click the DXO tray icon -> Exit), then run this again."
        }
    }

    cargo test --workspace
    cargo build --release --workspace

    if (Test-Path dist) { Remove-Item dist -Recurse -Force }
    # Framework-dependent by default (small, builds offline). -SelfContained
    # bundles the .NET runtime instead, but that needs a one-time ~150 MB NuGet
    # download. WinAppSDK is already bundled via the csproj either way.
    $sc = if ($SelfContained) { "true" } else { "false" }
    dotnet publish ui\DXO.UI\DXO.UI.csproj -c Release -p:Platform=x64 --self-contained $sc -o dist\DXO
    Copy-Item target\release\dxo.exe dist\DXO\ -ErrorAction SilentlyContinue  # command-line tool, handy for diagnostics
    Copy-Item installer\REVIEW.txt "dist\DXO\READ ME FIRST.txt" -ErrorAction SilentlyContinue

    # Portable zip: the simplest thing to send a reviewer (unzip, run DXO.UI.exe).
    $zip = "dist\DXO-$version-portable.zip"
    Compress-Archive -Path dist\DXO\* -DestinationPath $zip -Force
    Write-Host "wrote $zip"

    # Find Inno Setup's command-line compiler (ISCC.exe): common folders, then the
    # path it recorded in the registry (covers per-user and non-default installs),
    # then PATH. Set $env:ISCC to override.
    $iscc = @(
        $env:ISCC,
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $iscc) {
        foreach ($key in 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1',
                         'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1',
                         'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1') {
            $loc = (Get-ItemProperty -Path $key -Name InstallLocation -ErrorAction SilentlyContinue).InstallLocation
            if ($loc -and (Test-Path (Join-Path $loc 'ISCC.exe'))) { $iscc = Join-Path $loc 'ISCC.exe'; break }
        }
    }
    if (-not $iscc) {
        $iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    }
    if ($iscc) {
        Write-Host "Inno Setup: $iscc"
        & $iscc "/DMyVersion=$version" installer\dxo.iss
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed ($LASTEXITCODE)" }
    } else {
        Write-Warning "Inno Setup 6 not found: skipped the installer (the zip is still ready). If it's installed, set `$env:ISCC to the full path of ISCC.exe and re-run."
    }

    if ($DownloadUrl) {
        # Upload this next to the installer (a secret gist works) and put its raw
        # address in DXO > Settings > Updates.
        @{ version = $version; url = $DownloadUrl; notes = $Notes } | ConvertTo-Json | Set-Content -Encoding UTF8 dist\latest.json
        Write-Host "wrote dist\latest.json"
    }
} finally { Pop-Location }
