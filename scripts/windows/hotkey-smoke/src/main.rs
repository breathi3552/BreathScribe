use handy_keys::{Hotkey, HotkeyManager, HotkeyState};
use std::io::{self, Write};
use std::time::{Duration, Instant};

fn main() {
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("This smoke test is Windows-only.");
        std::process::exit(2);
    }

    #[cfg(target_os = "windows")]
    {
        if let Err(err) = run_windows_smoke() {
            eprintln!("HOTKEY_SMOKE_FAILED: {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "windows")]
fn run_windows_smoke() -> Result<(), Box<dyn std::error::Error>> {
    // This is the same blocking mode used by Handy Cloud's production shortcut
    // backend. A registered shortcut is intercepted before Windows/app targets.
    let manager = HotkeyManager::new_with_blocking()?;
    let hotkey: Hotkey = "super+h".parse()?;
    let id = manager.register(hotkey)?;

    println!("READY");
    io::stdout().flush()?;

    let deadline = Instant::now() + Duration::from_secs(8);
    let mut pressed = false;
    let mut released = false;

    while Instant::now() < deadline && !(pressed && released) {
        while let Some(event) = manager.try_recv() {
            if event.id != id {
                continue;
            }
            match event.state {
                HotkeyState::Pressed => {
                    pressed = true;
                    println!("PRESSED");
                    io::stdout().flush()?;
                }
                HotkeyState::Released => {
                    released = true;
                    println!("RELEASED");
                    io::stdout().flush()?;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    manager.unregister(id)?;

    if !pressed || !released {
        return Err(format!(
            "super+h did not deliver both edges (pressed={pressed}, released={released})"
        )
        .into());
    }

    println!("PASS");
    Ok(())
}
