$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$releaseDir = Join-Path $projectRoot "src-tauri\target\release"
$bundleDir = Join-Path $releaseDir "bundle"
$portableDir = Join-Path $bundleDir "portable"
$stagingDir = Join-Path $portableDir "staging"
$exePath = Join-Path $releaseDir "gachatrackerapp.exe"

if (-not (Test-Path $exePath)) {
  throw "Release binary not found at '$exePath'. Run 'npm run release:windows' first."
}

$configPath = Join-Path $projectRoot "src-tauri\tauri.conf.json"
$config = Get-Content $configPath -Raw | ConvertFrom-Json
$safeProductName = ($config.productName -replace '[\\/:*?"<>|]', '').Trim()
$version = $config.version
$zipName = "${safeProductName}_${version}_x64_portable.zip"
$zipPath = Join-Path $portableDir $zipName
$portableExePath = Join-Path $stagingDir "${safeProductName}.exe"

New-Item -ItemType Directory -Path $portableDir -Force | Out-Null
if (Test-Path $stagingDir) {
  Remove-Item $stagingDir -Recurse -Force
}
New-Item -ItemType Directory -Path $stagingDir -Force | Out-Null
Copy-Item -Path $exePath -Destination $portableExePath -Force

if (Test-Path $zipPath) {
  Remove-Item $zipPath -Force
}
Compress-Archive -Path (Join-Path $stagingDir "*") -DestinationPath $zipPath -CompressionLevel Optimal
Remove-Item $stagingDir -Recurse -Force

Write-Host "Portable bundle created: $zipPath"
