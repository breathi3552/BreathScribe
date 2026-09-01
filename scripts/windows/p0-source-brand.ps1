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

# Tray tooltip/menu title is hardcoded in Rust, so change only the visible
# product label. The tray state machine and click behavior remain untouched.
Replace-InFile "src-tauri/src/tray.rs" 'format!("Handy v{} (Dev)", env!("CARGO_PKG_VERSION"))' 'format!("Handy Cloud v{} (Dev)", env!("CARGO_PKG_VERSION"))'
Replace-InFile "src-tauri/src/tray.rs" 'format!("Handy v{}", env!("CARGO_PKG_VERSION"))' 'format!("Handy Cloud v{}", env!("CARGO_PKG_VERSION"))'

# User-facing source link now points to the fork; upstream remains credited in README.
Replace-InFile "src/components/settings/about/AboutSettings.tsx" 'https://github.com/cjpais/Handy' 'https://github.com/breathi3552/Handy-Cloud'

# JS package metadata follows the new project name. Dependency versions and lock data stay unchanged.
Replace-InFile "package.json" '"name": "handy-app"' '"name": "handy-cloud-app"'
Replace-InFile "bun.lock" '"name": "handy-app"' '"name": "handy-cloud-app"'

# Internal crate/binary names intentionally stay `handy` for compatibility with
# established autostart/package/runtime checks; OS package identity is Tauri's identifier.
Replace-InFile "src-tauri/Cargo.toml" 'description = "Handy"' 'description = "Handy Cloud"'

if (Test-Path "src-tauri/nsis/installer.nsi") {
  Replace-InFile "src-tauri/nsis/installer.nsi" "Custom NSIS template for Handy" "Custom NSIS template for Handy Cloud"
}

# Brand-copy only: replace standalone Handy in locale JSON. This does not touch
# HandyKeys or machine identifiers because the regex requires a standalone word.
$localeRoot = "src/locales"
if (Test-Path $localeRoot) {
  Get-ChildItem $localeRoot -Recurse -File -Filter "*.json" | ForEach-Object {
    $content = Get-Content $_.FullName -Raw
    $updated = [regex]::Replace($content, '(?<![\w-])Handy(?![\w-])', 'Handy Cloud')
    if ($updated -ne $content) {
      [System.IO.File]::WriteAllText($_.FullName, $updated, (New-Object System.Text.UTF8Encoding($false)))
      $changed = $true
      Write-Host "Updated locale branding: $($_.FullName)"
    }
  }
}

# README: mark the fork at the top while preserving upstream history/credit below.
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

# Generate all official Tauri icon slots from the approved Handy Cloud C-hand
# SVG once. Tauri 2 accepts squared PNG or SVG input and produces the Windows
# ICO plus the cross-platform PNG matrix.
$iconSource = "brand/handy-cloud-icon-source.svg"
$iconMarker = "brand/P0_ICON_GENERATED.txt"
if (-not (Test-Path $iconSource)) { throw "Missing approved brand icon source: $iconSource" }

$sourceHash = (Get-FileHash $iconSource -Algorithm SHA256).Hash.ToLowerInvariant()
$markerMatches = (Test-Path $iconMarker) -and ((Get-Content $iconMarker -Raw).Trim() -eq $sourceHash)
if (-not $markerMatches) {
  Write-Host "Generating Tauri icon matrix from approved Handy Cloud icon..."
  bun run tauri icon $iconSource
  if ($LASTEXITCODE -ne 0) { throw "tauri icon generation failed" }

  New-Item -ItemType Directory -Force -Path "src/assets" | Out-Null
  Copy-Item "src-tauri/icons/128x128.png" "src/assets/handy-cloud-icon.png" -Force

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

  # Tauri already rendered the SVG to PNG; use that raster as the source for
  # legacy files retained by the upstream tree so System.Drawing never has to
  # parse SVG itself.
  $rasterSource = "src-tauri/icons/128x128@2x.png"
  Save-ResizedPng $rasterSource "src-tauri/icons/64x64.png" 64
  Save-ResizedPng $rasterSource "src-tauri/icons/icon.png" 512
  Save-ResizedPng $rasterSource "src-tauri/icons/logo.png" 512

  function Save-TrayVariant {
    param([string]$Destination, [string]$Badge)
    $src = [System.Drawing.Image]::FromFile((Resolve-Path "src-tauri/icons/32x32.png"))
    try {
      $bmp = New-Object System.Drawing.Bitmap 32, 32
      try {
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        try {
          $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
          $g.DrawImage($src, 0, 0, 32, 32)
          if ($Badge) {
            $badgeColor = switch ($Badge) {
              "warning" { [System.Drawing.Color]::FromArgb(255, 245, 158, 11) }
              "recording" { [System.Drawing.Color]::FromArgb(255, 239, 68, 68) }
              "transcribing" { [System.Drawing.Color]::FromArgb(255, 59, 130, 246) }
              default { throw "Unknown tray badge: $Badge" }
            }
            $white = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::White)
            $brush = New-Object System.Drawing.SolidBrush $badgeColor
            try {
              $g.FillEllipse($white, 20, 20, 12, 12)
              $g.FillEllipse($brush, 22, 22, 8, 8)
            } finally {
              $white.Dispose()
              $brush.Dispose()
            }
          }
        } finally { $g.Dispose() }
        $bmp.Save($Destination, [System.Drawing.Imaging.ImageFormat]::Png)
      } finally { $bmp.Dispose() }
    } finally { $src.Dispose() }
  }

  $trayVariants = @{
    "src-tauri/resources/handy.png" = ""
    "src-tauri/resources/handy_warning.png" = "warning"
    "src-tauri/resources/recording.png" = "recording"
    "src-tauri/resources/transcribing.png" = "transcribing"
    "src-tauri/resources/tray_idle.png" = ""
    "src-tauri/resources/tray_idle_dark.png" = ""
    "src-tauri/resources/tray_idle_warning.png" = "warning"
    "src-tauri/resources/tray_idle_warning_dark.png" = "warning"
    "src-tauri/resources/tray_recording.png" = "recording"
    "src-tauri/resources/tray_recording_dark.png" = "recording"
    "src-tauri/resources/tray_transcribing.png" = "transcribing"
    "src-tauri/resources/tray_transcribing_dark.png" = "transcribing"
  }
  foreach ($entry in $trayVariants.GetEnumerator()) {
    Save-TrayVariant $entry.Key $entry.Value
  }

  Set-Content -Path $iconMarker -Value $sourceHash -NoNewline
  $changed = $true
  Write-Host "Generated Handy Cloud icon, installer, and tray asset matrix."
}

Write-Host "SOURCE_BRAND_CHANGED=$($changed.ToString().ToLowerInvariant())"
