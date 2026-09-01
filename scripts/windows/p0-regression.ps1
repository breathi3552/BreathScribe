param(
  [switch]$SkipHotkeySmoke
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Assert-True {
  param([bool]$Condition, [string]$Message)
  if (-not $Condition) { throw $Message }
}

function Assert-GitBlob {
  param([string]$Path, [string]$Expected)
  Assert-True (Test-Path $Path) "Missing protected P0 path: $Path"
  $actual = (git hash-object -- $Path).Trim()
  Assert-True ($actual -eq $Expected) "Protected P0 path changed unexpectedly: $Path`nexpected=$Expected`nactual=$actual"
  Write-Host "PASS unchanged: $Path ($actual)"
}

Write-Host "=== Handy Cloud P0 Windows regression ==="

$config = Get-Content "src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json
Assert-True ($config.productName -eq "Handy Cloud") "Tauri productName must be 'Handy Cloud'"
Assert-True ($config.identifier -eq "io.github.breathi3552.handycloud") "Unexpected Tauri identifier"
Assert-True (-not ($config.bundle.windows.PSObject.Properties.Name -contains "signCommand")) "Upstream Azure signCommand must not remain in the fork"
Assert-True ($config.plugins.updater.endpoints.Count -eq 1) "Expected one fork updater endpoint"
Assert-True ($config.plugins.updater.endpoints[0] -match "breathi3552/Handy-Cloud") "Updater must not point back to upstream Handy releases"
Write-Host "PASS branding/package identity"

$package = Get-Content "package.json" -Raw | ConvertFrom-Json
Assert-True ($package.name -eq "handy-cloud-app") "Frontend package name must be handy-cloud-app"
Write-Host "PASS frontend package name"

$theme = Get-Content "src/styles/theme.css" -Raw
Assert-True ($theme -match "#8ed1f7") "Candy light-blue primary theme token missing"
Assert-True ($theme -notmatch "#faa2ca") "Original pink primary theme token still active"
Write-Host "PASS candy-blue theme"

$cargo = Get-Content "src-tauri/Cargo.toml" -Raw
Assert-True ($cargo -match 'handy-keys\s*=\s*"0\.3\.4"') "handy-keys must remain pinned to the current 0.3.4 path"
Write-Host "PASS handy-keys dependency preserved"

$settings = Get-Content "src-tauri/src/settings.rs" -Raw
Assert-True ($settings -match '(?s)#\[cfg\(not\(target_os = "linux"\)\)\]\s*return KeyboardImplementation::HandyKeys;') "Windows/macOS default must remain HandyKeys"
Write-Host "PASS Windows default shortcut backend is HandyKeys"

$hotkeys = Get-Content "src-tauri/src/shortcut/handy_keys.rs" -Raw
Assert-True ($hotkeys -match 'HotkeyManager::new_with_blocking\(\)') "Production HandyKeys path must use the blocking manager"
Assert-True ($hotkeys -match 'event\.state == HotkeyState::Pressed') "Pressed edge handling missing"
Assert-True ($hotkeys -match 'handle_shortcut_event\(&app,\s*binding_id,\s*hotkey_string,\s*is_pressed\)') "Pressed/released dispatch path missing"
Assert-True ($hotkeys -match 'KeyboardListener::new\(\)') "Shortcut recording listener path missing"
Assert-True ($hotkeys -match 'is_key_down:\s*key_event\.is_key_down') "Shortcut recorder must preserve key down/up state"
Write-Host "PASS shortcut registration/recording/edge dispatch contracts"

# These hashes deliberately pin the P0-sensitive interaction paths to the
# upstream fork point. Branding must not silently rewrite input/PTT behavior.
Assert-GitBlob "src-tauri/src/shortcut/handy_keys.rs" "bd8e562e50226d62d0ffedffc5149d96dd1955cd"
Assert-GitBlob "src-tauri/src/transcription_coordinator.rs" "a35bff37eaa6bd3f2fe9dce4403d96746be36e3c"
Assert-GitBlob "src-tauri/src/input.rs" "f479f8b0e145215e7fecab29de1e6cf10fe749b7"
Assert-GitBlob "src-tauri/src/paste_tx/windows.rs" "55f13bf52044a485afa4fae72fa776b4facdff44"
Assert-GitBlob "src/components/settings/HandyKeysShortcutInput.tsx" "5a1b4b58a139d4834f3c9fd436cf52c63715b988"
Write-Host "PASS PTT and text-injection paths preserved byte-for-byte"

if (-not $SkipHotkeySmoke) {
  Write-Host "=== Real Windows super+h blocking-backend smoke ==="
  cargo build --manifest-path "scripts/windows/hotkey-smoke/Cargo.toml" --release
  if ($LASTEXITCODE -ne 0) { throw "Failed to build Windows hotkey smoke helper" }

  $exe = "scripts/windows/hotkey-smoke/target/release/handy-cloud-hotkey-smoke.exe"
  Assert-True (Test-Path $exe) "Hotkey smoke helper not found: $exe"

  $stdout = Join-Path $env:RUNNER_TEMP "handy-cloud-hotkey-smoke.out"
  $stderr = Join-Path $env:RUNNER_TEMP "handy-cloud-hotkey-smoke.err"
  Remove-Item $stdout,$stderr -Force -ErrorAction SilentlyContinue

  $proc = Start-Process -FilePath $exe -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru

  $ready = $false
  for ($i = 0; $i -lt 100; $i++) {
    Start-Sleep -Milliseconds 50
    if (Test-Path $stdout) {
      $text = Get-Content $stdout -Raw -ErrorAction SilentlyContinue
      if ($text -match "READY") { $ready = $true; break }
    }
    if ($proc.HasExited) { break }
  }
  if (-not $ready) {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
    $errText = if (Test-Path $stderr) { Get-Content $stderr -Raw } else { "" }
    throw "Hotkey smoke helper never became ready. $errText"
  }

  Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class HandyCloudKeyInjector {
    [DllImport("user32.dll", SetLastError=true)]
    private static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    private const uint KEYEVENTF_KEYUP = 0x0002;
    public static void SendWinH() {
        keybd_event(0x5B, 0, 0, UIntPtr.Zero); // LWIN down
        System.Threading.Thread.Sleep(40);
        keybd_event(0x48, 0, 0, UIntPtr.Zero); // H down
        System.Threading.Thread.Sleep(120);
        keybd_event(0x48, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); // H up
        keybd_event(0x5B, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); // LWIN up
    }
}
"@

  [HandyCloudKeyInjector]::SendWinH()

  if (-not $proc.WaitForExit(10000)) {
    Stop-Process -Id $proc.Id -Force
    throw "Hotkey smoke helper timed out waiting for press/release"
  }

  $outText = if (Test-Path $stdout) { Get-Content $stdout -Raw } else { "" }
  $errText = if (Test-Path $stderr) { Get-Content $stderr -Raw } else { "" }
  Write-Host $outText
  if ($errText) { Write-Host $errText }

  Assert-True ($proc.ExitCode -eq 0) "Win+H hotkey smoke failed with exit code $($proc.ExitCode)"
  Assert-True ($outText -match "PRESSED") "Win+H press edge was not received"
  Assert-True ($outText -match "RELEASED") "Win+H release edge was not received"
  Assert-True ($outText -match "PASS") "Win+H smoke did not report PASS"
  Write-Host "PASS Win+H registration + pressed/released via blocking HandyKeys backend"
}

Write-Host "=== P0 Windows regression PASS ==="
