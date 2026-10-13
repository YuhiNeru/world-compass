$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Push-Location $taskRoot
try {
    npm.cmd run verify
    if ($LASTEXITCODE -ne 0) { throw 'Verification failed' }
    python tools/release/notices.py
    if ($LASTEXITCODE -ne 0) { throw 'License inventory generation failed' }
    npm.cmd run bundle
    if ($LASTEXITCODE -ne 0) { throw 'Installer build failed' }
    $taskArtifacts = Join-Path $taskRoot 'artifacts'
    New-Item -ItemType Directory -Force -Path $taskArtifacts | Out-Null
    $taskVersion = (Get-Content -Raw -LiteralPath (Join-Path $taskRoot 'package.json') | ConvertFrom-Json).version
    $taskInstaller = Get-Item -LiteralPath (Join-Path $taskRoot "apps/desktop/src-tauri/target/release/bundle/nsis/World Compass_${taskVersion}_x64-setup.exe")
    $taskDestination = Join-Path $taskArtifacts "WorldCompass-Setup-${taskVersion}-win-x64.exe"
    Copy-Item -LiteralPath $taskInstaller.FullName -Destination $taskDestination
    Get-FileHash -Algorithm SHA256 -LiteralPath $taskDestination | Format-List
} finally { Pop-Location }
