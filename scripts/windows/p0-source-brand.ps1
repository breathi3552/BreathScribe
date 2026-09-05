param(
  [switch]$ForceIcons
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$changed = $false

function Replace-InFile {
  param([string]$Path, [string]$Old, [string]$New)
  if (-not (Test-Path $Path)) { throw "Brand patch path missing: $Path" }
  $content = Get-Content $Path -Raw
  $updated = $content.Replace($Old, $New)
  if ($updated -ne $content) {
    [System.IO.File]::WriteAllText((Resolve-Path $Path), $updated, (New-Object System.Text.UTF8Encoding($false)))
    $script:changed = $true
    Write-Host "Updated brand string: $Path"
  }
}

Replace-InFile "src-tauri/src/tray.rs" 'format!("Handy v{} (Dev)", env!("CARGO_PKG_VERSION"))' 'format!("Handy Cloud v{} (Dev)", env!("CARGO_PKG_VERSION"))'
Replace-InFile "src-tauri/src/tray.rs" 'format!("Handy v{}", env!("CARGO_PKG_VERSION"))' 'format!("Handy Cloud v{}", env!("CARGO_PKG_VERSION"))'
Replace-InFile "src/components/settings/about/AboutSettings.tsx" 'https://github.com/cjpais/Handy' 'https://github.com/breathi3552/Handy-Cloud'
Replace-InFile "package.json" '"name": "handy-app"' '"name": "handy-cloud-app"'
Replace-InFile "bun.lock" '"name": "handy-app"' '"name": "handy-cloud-app"'
Replace-InFile "src-tauri/Cargo.toml" 'description = "Handy"' 'description = "Handy Cloud"'

if (Test-Path "src-tauri/nsis/installer.nsi") {
  $path = "src-tauri/nsis/installer.nsi"
  $content = Get-Content $path -Raw
  $updated = [regex]::Replace($content, 'Custom NSIS template for Handy(?: Cloud)*', 'Custom NSIS template for Handy Cloud')

  $installerDefine = '!define INSTALLERICON "{{installer_icon}}"'
  $uninstallerDefine = '!define UNINSTALLERICON "{{uninstaller_icon}}"'
  if (-not $updated.Contains($uninstallerDefine)) {
    if (-not $updated.Contains($installerDefine)) { throw "NSIS installer icon placeholder missing from custom template" }
    $updated = $updated.Replace($installerDefine, $installerDefine + "`n" + $uninstallerDefine)
  }

  $muiInstaller = '  !define MUI_ICON "${INSTALLERICON}"'
  $muiUninstaller = '  !define MUI_UNICON "${UNINSTALLERICON}"'
  if (-not $updated.Contains($muiUninstaller)) {
    if (-not $updated.Contains($muiInstaller)) { throw "NSIS MUI installer icon definition missing from custom template" }
    $updated = $updated.Replace($muiInstaller, $muiInstaller + "`n" + $muiUninstaller)
  }

  if ($updated -ne $content) {
    [System.IO.File]::WriteAllText((Resolve-Path $path), $updated, (New-Object System.Text.UTF8Encoding($false)))
    $changed = $true
    Write-Host "Updated Handy Cloud NSIS icon wiring: $path"
  }
}

$localeRoot = "src/i18n/locales"
if (Test-Path $localeRoot) {
  Get-ChildItem $localeRoot -Recurse -File -Filter "*.json" | ForEach-Object {
    $content = Get-Content $_.FullName -Raw
    $updated = [regex]::Replace($content, '(?<![\w-])Handy(?![\w-]| Cloud)', 'Handy Cloud')
    if ($updated -ne $content) {
      [System.IO.File]::WriteAllText($_.FullName, $updated, (New-Object System.Text.UTF8Encoding($false)))
      $changed = $true
      Write-Host "Updated locale branding: $($_.FullName)"
    }
  }
}

$readme = "README.md"
$content = Get-Content $readme -Raw
if ($content.StartsWith("# Handy`n") -or $content.StartsWith("# Handy`r`n")) {
  $updated = [regex]::Replace(
    $content,
    '^# Handy\r?\n',
    "# Handy Cloud`n`n> Handy Cloud is an independent fork of [cjpais/Handy](https://github.com/cjpais/Handy). P0 preserves Handy's local transcription and Windows interaction paths while establishing a separate package identity and build base.`n",
    1
  )
  [System.IO.File]::WriteAllText((Resolve-Path $readme), $updated, (New-Object System.Text.UTF8Encoding($false)))
  $changed = $true
}

$iconSource = "brand/handy-cloud-icon-source.png"
$iconMarker = "brand/P0_ICON_GENERATED.txt"
if (-not (Test-Path $iconSource)) { throw "Missing approved brand icon source: $iconSource" }

$sourceHash = (Get-FileHash $iconSource -Algorithm SHA256).Hash.ToLowerInvariant()
$markerMatches = (Test-Path $iconMarker) -and ((Get-Content $iconMarker -Raw).Trim() -eq $sourceHash)
$criticalIcons = @(
  "src-tauri/icons/32x32.png",
  "src-tauri/icons/128x128.png",
  "src-tauri/icons/128x128@2x.png",
  "src-tauri/icons/icon.ico",
  "src-tauri/icons/icon.icns"
)
$criticalIconsPresent = @($criticalIcons | Where-Object { -not (Test-Path $_) }).Count -eq 0
$needsIconGeneration = $ForceIcons -or (-not $markerMatches) -or (-not $criticalIconsPresent)

if ($needsIconGeneration) {
  Write-Host "Generating Tauri icon matrix from approved Handy Cloud icon..."
  bun run tauri icon $iconSource
  if ($LASTEXITCODE -ne 0) { throw "tauri icon generation failed" }

  Add-Type -AssemblyName System.Drawing

  function Save-ResizedPng {
    param([string]$Source, [string]$Destination, [int]$Size)
    $src = [System.Drawing.Image]::FromFile((Resolve-Path $Source))
    try {
      $bmp = New-Object System.Drawing.Bitmap $Size, $Size
      try {
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        try {
          $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
          $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
          $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
          $g.DrawImage($src, 0, 0, $Size, $Size)
        } finally { $g.Dispose() }
        $bmp.Save($Destination, [System.Drawing.Imaging.ImageFormat]::Png)
      } finally { $bmp.Dispose() }
    } finally { $src.Dispose() }
  }

  $rasterSource = "src-tauri/icons/128x128@2x.png"
  Save-ResizedPng $rasterSource "src-tauri/icons/64x64.png" 64
  Save-ResizedPng $rasterSource "src-tauri/icons/icon.png" 512
  Save-ResizedPng $rasterSource "src-tauri/icons/logo.png" 512

  python scripts/windows/generate-tray-icons.py
  if ($LASTEXITCODE -ne 0) { throw "Failed to generate tray icon matrix" }

  [System.IO.File]::WriteAllText((Join-Path (Get-Location) $iconMarker), $sourceHash, (New-Object System.Text.UTF8Encoding($false)))
  $changed = $true
  Write-Host "Generated Handy Cloud icon, installer, and tray asset matrix."
}

Write-Host "SOURCE_BRAND_CHANGED=$($changed.ToString().ToLowerInvariant())"
