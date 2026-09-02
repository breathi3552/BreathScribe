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

function Test-BitmapPixelsEqual {
  param(
    [System.Drawing.Bitmap]$Actual,
    [System.Drawing.Bitmap]$Expected
  )
  if ($Actual.Width -ne $Expected.Width -or $Actual.Height -ne $Expected.Height) { return $false }
  for ($y = 0; $y -lt $Actual.Height; $y++) {
    for ($x = 0; $x -lt $Actual.Width; $x++) {
      if ($Actual.GetPixel($x, $y).ToArgb() -ne $Expected.GetPixel($x, $y).ToArgb()) { return $false }
    }
  }
  return $true
}

function Assert-ExecutableIconMatchesBrand {
  param([string]$ExecutablePath, [string]$Label)
  Add-Type -AssemblyName System.Drawing
  $resolvedExe = (Resolve-Path $ExecutablePath).Path
  $resolvedBrand = (Resolve-Path "src-tauri/icons/icon.ico").Path
  $actualIcon = [System.Drawing.Icon]::ExtractAssociatedIcon($resolvedExe)
  Assert-True ($null -ne $actualIcon) "$Label exposes an embedded Windows icon"
  try {
    $brandIcon = [System.Drawing.Icon]::new($resolvedBrand, $actualIcon.Width, $actualIcon.Height)
    try {
      $actualBitmap = $actualIcon.ToBitmap()
      $brandBitmap = $brandIcon.ToBitmap()
      try {
        Assert-True (Test-BitmapPixelsEqual $actualBitmap $brandBitmap) "$Label embedded icon matches Handy Cloud icon.ico"
      } finally {
        $actualBitmap.Dispose()
        $brandBitmap.Dispose()
      }
    } finally {
      $brandIcon.Dispose()
    }
  } finally {
    $actualIcon.Dispose()
  }
}

function Assert-AppExecutableBranding {
  param([System.IO.FileInfo]$Executable, [string]$Label)
  $metadata = @($Executable.VersionInfo.ProductName, $Executable.VersionInfo.FileDescription) -join " | "
  Assert-True ($metadata -match "Handy Cloud") "$Label app executable metadata contains Handy Cloud"
  Assert-ExecutableIconMatchesBrand -ExecutablePath $Executable.FullName -Label "$Label app executable"
}

$configPath = "src-tauri/tauri.conf.json"
Assert-FileExists $configPath
$config = Get-Content $configPath -Raw | ConvertFrom-Json
Assert-True ($config.productName -eq "Handy Cloud") "Tauri productName is Handy Cloud"
Assert-True ($config.identifier -eq "io.github.breathi3552.handycloud") "Tauri identifier is Handy Cloud identifier"
$endpoint = [string]$config.plugins.updater.endpoints[0]
Assert-True ($endpoint -like "*breathi3552/Handy-Cloud*") "updater points at Handy-Cloud fork"
Assert-True ($endpoint -notlike "*cjpais/Handy*") "updater no longer points at upstream"
Assert-True ([string]$config.bundle.windows.nsis.installerIcon -eq "icons/icon.ico") "NSIS installerIcon explicitly uses Handy Cloud icon.ico"
Assert-True ([string]$config.bundle.windows.nsis.uninstallerIcon -eq "icons/icon.ico") "NSIS uninstallerIcon explicitly uses Handy Cloud icon.ico"

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

# Upstream attribution and technical dependency URLs may remain. Product updater ownership is checked above.
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

    $extractDir = Join-Path ([System.IO.Path]::GetTempPath()) ("handy-cloud-msi-" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $extractDir | Out-Null
    try {
      $logFile = Join-Path $extractDir "msi-admin-install.log"
      $msiArgs = "/a `"$($msi.FullName)`" /qn /L*v `"$logFile`" TARGETDIR=`"$extractDir`""
      $proc = Start-Process -FilePath "msiexec.exe" -ArgumentList $msiArgs -Wait -PassThru
      Assert-True ($proc.ExitCode -eq 0) "MSI administrative extraction succeeds"
      $installedExe = Get-ChildItem $extractDir -Filter "handy.exe" -Recurse -File | Select-Object -First 1
      Assert-True ($null -ne $installedExe) "MSI payload contains handy.exe"
      Assert-AppExecutableBranding -Executable $installedExe -Label "MSI payload"
    } finally {
      Remove-Item -Recurse -Force $extractDir -ErrorAction SilentlyContinue
    }
  }

  $setup = $packages | Where-Object Extension -eq ".exe" | Select-Object -First 1
  if ($setup) {
    $vi = $setup.VersionInfo
    $metadata = @($vi.ProductName, $vi.FileDescription) -join " | "
    Assert-True ($metadata -match "Handy Cloud") "NSIS EXE version metadata contains Handy Cloud"
    Assert-ExecutableIconMatchesBrand -ExecutablePath $setup.FullName -Label "NSIS installer"

    $portableDir = Join-Path ([System.IO.Path]::GetTempPath()) ("handy-cloud-nsis-" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $portableDir | Out-Null
    try {
      $proc = Start-Process -FilePath $setup.FullName -ArgumentList @("/S", "/PORTABLE", "/D=$portableDir") -Wait -PassThru
      Assert-True ($proc.ExitCode -eq 0) "NSIS silent portable extraction succeeds"
      $installedExe = Get-ChildItem $portableDir -Filter "handy.exe" -Recurse -File | Select-Object -First 1
      Assert-True ($null -ne $installedExe) "NSIS payload contains handy.exe"
      Assert-AppExecutableBranding -Executable $installedExe -Label "NSIS payload"
    } finally {
      Remove-Item -Recurse -Force $portableDir -ErrorAction SilentlyContinue
    }
  }
}

Write-Host "P0 branding/source regression complete."
