param(
  [switch]$ForceIcons
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$changed = $false

$iconSource = "brand/breath-scribe-icon-source.svg"
$iconMarker = "brand/P0_ICON_GENERATED.txt"
if (-not (Test-Path $iconSource)) { throw "Missing approved brand icon source: $iconSource" }

$sourceContent = [System.IO.File]::ReadAllText((Resolve-Path $iconSource)).Replace("`r`n", "`n")
$sha = [System.Security.Cryptography.SHA256]::Create()
$sourceBytes = [System.Text.Encoding]::UTF8.GetBytes($sourceContent)
$sourceHash = ([System.BitConverter]::ToString($sha.ComputeHash($sourceBytes))).Replace("-", "").ToLowerInvariant()
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
  Write-Host "Generating Tauri icon matrix from approved BreathScribe icon..."
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
  Write-Host "Generated BreathScribe icon, installer, and tray asset matrix."
}

Write-Host "SOURCE_BRAND_CHANGED=$($changed.ToString().ToLowerInvariant())"
