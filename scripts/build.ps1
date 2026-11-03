# Build the Rust engine, then the WinUI 3 app (which copies the DLL in).
# Usage:  .\scripts\build.ps1            (release)
#         .\scripts\build.ps1 -Run       (build and launch)
param([switch]$Run)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    cargo test --workspace
    cargo build --release --workspace
    dotnet build ui\DXO.UI\DXO.UI.csproj -c Release -p:Platform=x64
    if ($Run) {
        $exe = Get-ChildItem -Recurse ui\DXO.UI\bin\x64\Release -Filter DXO.UI.exe | Select-Object -First 1
        Start-Process $exe.FullName  # DXO asks for admin rights itself (UAC prompt)
    }
} finally { Pop-Location }
