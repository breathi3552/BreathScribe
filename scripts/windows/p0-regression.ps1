param(
  [string]$PackageDir = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Assert-True {
  param([bool]$Condition, [string]$Message)
  if (-not $Condition) { throw "P0 regression failed: $Message" }
  Write-Host "PASS - $Message"
}

function Assert-FileExists {
  param([string]$Path)
  Assert-True (Test-Path -LiteralPath $Path -PathType Leaf) "file exists: $Path"
}

$configPath = "src-tauri/tauri.conf.json"
Assert-FileExists $configPath
$config = Get-Content $configPath -Raw | ConvertFrom-Json
Assert-True ($config.productName -eq "Handy Cloud") "Tauri productName is Handy Cloud"
Assert-True ($config.identifier -eq "io.github.breathi3552.handycloud") "Tauri identifier is Handy Cloud identifier"
$endpoint = [string]$config.plugins.updater.endpoints[0]
Assert-True ($endpoint -like "*breathi3552/Handy-Cloud*") "updater points at Handy-Cloud fork"
Assert-True ($endpoint -notlike "*cjpais/Handy*") "updater no longer points at upstream"

$iconSource = "brand/handy-cloud-icon-source.png"
$marker = "brand/P0_ICON_GENERATED.txt"
Assert-FileExists $iconSource
Assert-FileExists $marker
$sourceHash = (Get-FileHash $iconSource -Algorithm SHA256).Hash.ToLowerInvariant()
$markerHash = (Get-Content $marker -Raw).Trim().ToLowerInvariant()
Assert-True ($sourceHash -eq $markerHash) "icon marker matches approved source SHA-256"

$bundleIcons = @($config.bundle.icon)
Assert-True ($bundleIcons.Count -gt 0) "Tauri bundle icon list is non-empty"
foreach ($relative in $bundleIcons) {
  Assert-FileExists (Join-Path "src-tauri" $relative)
}

$criticalBrandAssets = @(
  "src-tauri/icons/32x32.png",
  "src-tauri/icons/64x64.png",
  "src-tauri/icons/128x128.png",
  "src-tauri/icons/128x128@2x.png",
  "src-tauri/icons/icon.png",
  "src-tauri/icons/icon.ico",
  "src-tauri/icons/icon.icns",
  "src-tauri/icons/Square30x30Logo.png",
  "src-tauri/icons/Square44x44Logo.png",
  "src-tauri/icons/Square71x71Logo.png",
  "src-tauri/icons/Square89x89Logo.png",
  "src-tauri/icons/Square107x107Logo.png",
  "src-tauri/icons/Square142x142Logo.png",
  "src-tauri/icons/Square150x150Logo.png",
  "src-tauri/icons/Square284x284Logo.png",
  "src-tauri/icons/Square310x310Logo.png",
  "src-tauri/icons/StoreLogo.png",
  "src-tauri/resources/handy.png",
  "src-tauri/resources/handy_warning.png",
  "src-tauri/resources/recording.png",
  "src-tauri/resources/transcribing.png",
  "src-tauri/resources/tray_idle.png",
  "src-tauri/resources/tray_idle_dark.png",
  "src-tauri/resources/tray_idle_warning.png",
  "src-tauri/resources/tray_idle_warning_dark.png",
  "src-tauri/resources/tray_recording.png",
  "src-tauri/resources/tray_recording_dark.png",
  "src-tauri/resources/tray_transcribing.png",
  "src-tauri/resources/tray_transcribing_dark.png"
)
foreach ($asset in $criticalBrandAssets) { Assert-FileExists $asset }

if (-not (git remote | Select-String -SimpleMatch "upstream" -Quiet)) {
  git remote add upstream https://github.com/cjpais/Handy.git
}
git fetch upstream main --depth=1 --quiet
if ($LASTEXITCODE -ne 0) { throw "Unable to fetch upstream cjpais/Handy for P0 byte comparison" }
foreach ($asset in $criticalBrandAssets) {
  git cat-file -e "upstream/main:$asset" 2>$null
  if ($LASTEXITCODE -eq 0) {
    $localSha = (git hash-object -- $asset).Trim()
    $upstreamSha = (git rev-parse "upstream/main:$asset").Trim()
    Assert-True ($localSha -ne $upstreamSha) "asset differs byte-for-byte from upstream: $asset"
  }
}

$sourcePaths = @(
  "src/components/icons/HandyHand.tsx",
  "src/components/icons/HandyTextLogo.tsx",
  "src/components/settings/about/AboutSettings.tsx",
  "src/overlay/RecordingOverlay.tsx",
  "src-tauri/src/tray.rs"
)
foreach ($path in $sourcePaths) { Assert-FileExists $path }
Assert-True ((Get-Content "src/components/icons/HandyHand.tsx" -Raw) -match "handy-cloud-icon-source\.png") "UI hand icon uses approved Handy Cloud source"
Assert-True ((Get-Content "src/components/icons/HandyTextLogo.tsx" -Raw) -match "Handy Cloud") "text logo displays Handy Cloud"
Assert-True ((Get-Content "src/components/settings/about/AboutSettings.tsx" -Raw) -notmatch "github\.com/cjpais/Handy") "About source link uses fork"
Assert-True ((Get-Content "src-tauri/src/tray.rs" -Raw) -match "Handy Cloud v") "Tray tooltip identifies Handy Cloud"

Assert-True ((Get-Content "src-tauri/Cargo.toml" -Raw) -match 'handy-keys\s*=') "handy-keys dependency remains intact"
Assert-True (Test-Path "LICENSE") "LICENSE/upstream attribution remains present"

# Upstream attribution, technical dependency URLs and dormant reusable signing hooks may remain.
# P0 product ownership is enforced through updater configuration and an explicitly unsigned fork release.
$scanTargets = @("src-tauri/tauri.conf.json", ".github/workflows/release.yml")
$forbidden = @("AZURE_TENANT_ID", "AZURE_CLIENT_ID", "AZURE_CLIENT_SECRET")
foreach ($needle in $forbidden) {
  $matches = @(Get-ChildItem $scanTargets -Recurse -File -ErrorAction SilentlyContinue | Select-String -SimpleMatch $needle)
  Assert-True ($matches.Count -eq 0) "fork release configuration does not request legacy signing: $needle"
}

$releaseText = Get-Content ".github/workflows/release.yml" -Raw
Assert-True ($releaseText -match 'asset-prefix:\s*"handy-cloud"') "release artifact prefix is Handy Cloud branded"
Assert-True ($releaseText -match 'sign-binaries:\s*false') "P0 fork release is unsigned"

$corePaths = @(
  "src-tauri/src/shortcut/handy_keys.rs",
  "src-tauri/src/shortcut/handler.rs",
  "src-tauri/src/input.rs",
  "src-tauri/src/transcription_coordinator.rs",
  "src-tauri/src/clipboard.rs",
  "src-tauri/src/paste_tx/windows.rs",
  "src-tauri/src/managers/audio.rs"
)
foreach ($path in $corePaths) { Assert-FileExists $path }

if ($PackageDir) {
  Assert-True (Test-Path $PackageDir -PathType Container) "package directory exists: $PackageDir"
  $packages = @(Get-ChildItem $PackageDir -Recurse -File | Where-Object { $_.Extension -in @(".exe", ".msi") })
  Assert-True ($packages.Count -gt 0) "Windows EXE/MSI package artifacts exist"
  foreach ($pkg in $packages) {
    Assert-True ($pkg.Name -match "Handy Cloud|Handy.Cloud|Handy_Cloud") "installer filename is Handy Cloud branded: $($pkg.Name)"
  }

  $msi = $packages | Where-Object Extension -eq ".msi" | Select-Object -First 1
  if ($msi) {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase($msi.FullName, 0)
    $view = $database.OpenView("SELECT `Value` FROM `Property` WHERE `Property`='ProductName'")
    $view.Execute()
    $record = $view.Fetch()
    $productName = if ($record) { [string]$record.StringData(1) } else { "" }
    $view.Close()
    Assert-True ($productName -eq "Handy Cloud") "MSI ProductName is Handy Cloud"
  }

  $setup = $packages | Where-Object Extension -eq ".exe" | Select-Object -First 1
  if ($setup) {
    $vi = $setup.VersionInfo
    $metadata = @($vi.ProductName, $vi.FileDescription) -join " | "
    Assert-True ($metadata -match "Handy Cloud") "NSIS EXE version metadata contains Handy Cloud"
  }
}

Write-Host "P0 branding/source regression complete."
