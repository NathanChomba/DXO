# Build a release of DXO in two flavours, each as an installer and a portable zip:
#
#   standard     small; the user installs the .NET 10 Desktop Runtime once
#                  dist\DXO-Setup-<ver>.exe, dist\DXO-<ver>-portable.zip
#   with .NET    bigger; the .NET runtime is bundled, nothing else to install
#                  dist\DXO-Setup-<ver>-with-dotnet.exe, dist\DXO-<ver>-portable-with-dotnet.zip
#
#   .\scripts\package.ps1                       -> both flavours (the default)
#   .\scripts\package.ps1 -Flavour Standard     -> only the small one (builds offline)
#   .\scripts\package.ps1 -Flavour WithDotNet   -> only the bundled one
#   .\scripts\package.ps1 -DownloadUrl <url>    -> also writes dist\latest.json for the update check
#
# The bundled flavour needs a one-time download of the .NET runtime pack
# (about 150 MB) the first time it is built; after that it builds offline too.
# The installers need Inno Setup 6 or 7 (https://jrsoftware.org/isdl.php); without
# it you still get both zips.
param(
    [ValidateSet("Both", "Standard", "WithDotNet")][string]$Flavour = "Both",
    [switch]$SelfContained,   # old name for -Flavour WithDotNet
    [string]$DownloadUrl = "",
    [string]$Notes = ""
)
$ErrorActionPreference = "Stop"
if ($SelfContained) { $Flavour = "WithDotNet" }
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $version = (Select-Xml -Path ui\DXO.UI\DXO.UI.csproj -XPath "//Version").Node.InnerText
    Write-Host "DXO $version ($Flavour)"

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
    if ($LASTEXITCODE -ne 0) { throw "Engine tests failed: fix them before packaging." }
    cargo build --release --workspace
    if ($LASTEXITCODE -ne 0) { throw "Engine build failed ($LASTEXITCODE)" }

    if (Test-Path dist) { Remove-Item dist -Recurse -Force }

    # Find Inno Setup's command-line compiler (ISCC.exe): common folders, then the
    # path it recorded in the registry (covers per-user and non-default installs),
    # then PATH. Set $env:ISCC to override.
    $iscc = @(
        $env:ISCC,
        "$env:ProgramFiles\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 7\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $iscc) {
        $keys = foreach ($v in 7, 6) {
            "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup ${v}_is1"
            "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup ${v}_is1"
            "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup ${v}_is1"
        }
        foreach ($key in $keys) {
            $loc = (Get-ItemProperty -Path $key -Name InstallLocation -ErrorAction SilentlyContinue).InstallLocation
            if ($loc -and (Test-Path (Join-Path $loc 'ISCC.exe'))) { $iscc = Join-Path $loc 'ISCC.exe'; break }
        }
    }
    if (-not $iscc) {
        $iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    }
    if ($iscc) { Write-Host "Inno Setup: $iscc" }
    else { Write-Warning "Inno Setup (6 or 7) not found: the installers will be skipped (the zips are still made). If it's installed, set `$env:ISCC to the full path of ISCC.exe and re-run." }

    # One flavour: publish the app, copy the extras, zip it, make its installer.
    function Build-Flavour([string]$Name, [bool]$Bundle, [string]$Suffix) {
        $dir = "dist\$Name"
        $sc = if ($Bundle) { "true" } else { "false" }
        Write-Host ""
        Write-Host "=== $Name (.NET bundled: $sc) ==="
        dotnet publish ui\DXO.UI\DXO.UI.csproj -c Release -p:Platform=x64 --self-contained $sc -o $dir
        if ($LASTEXITCODE -ne 0) {
            if ($Bundle) { throw "Publishing the bundled flavour failed. It downloads the .NET runtime pack (about 150 MB) the first time: check your connection, or build only the small one with -Flavour Standard." }
            throw "dotnet publish failed ($LASTEXITCODE)"
        }
        Copy-Item target\release\dxo.exe $dir\ -ErrorAction SilentlyContinue  # command-line tool, handy for diagnostics
        Copy-Item installer\REVIEW.txt "$dir\READ ME FIRST.txt" -ErrorAction SilentlyContinue

        $zip = "dist\DXO-$version-portable$Suffix.zip"
        Compress-Archive -Path "$dir\*" -DestinationPath $zip -Force
        Write-Host "wrote $zip"

        if ($script:iscc) {
            & $script:iscc "/DMyVersion=$version" "/DSourceDir=..\$dir" "/DOutputName=DXO-Setup-$version$Suffix" installer\dxo.iss
            if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed ($LASTEXITCODE)" }
            Write-Host "wrote dist\DXO-Setup-$version$Suffix.exe"
        }
    }
    $script:iscc = $iscc

    if ($Flavour -in "Both", "Standard")   { Build-Flavour "DXO"            $false "" }
    if ($Flavour -in "Both", "WithDotNet") { Build-Flavour "DXO-with-dotnet" $true "-with-dotnet" }

    if ($DownloadUrl) {
        # Upload this next to the installer (a secret gist works) and put its raw
        # address in DXO > Settings > Updates.
        @{ version = $version; url = $DownloadUrl; notes = $Notes } | ConvertTo-Json | Set-Content -Encoding UTF8 dist\latest.json
        Write-Host "wrote dist\latest.json"
    }

    Write-Host ""
    Write-Host "Done. In dist\:"
    Get-ChildItem dist -File | ForEach-Object { Write-Host ("  {0,-45} {1,8:N1} MB" -f $_.Name, ($_.Length / 1MB)) }
} finally { Pop-Location }
