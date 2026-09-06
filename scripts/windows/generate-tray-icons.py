import os
import shutil
import subprocess
import tempfile

TRAY_SVGS = {
    "tray_idle.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="none" stroke="#38bdf8" stroke-width="4.5" stroke-linejoin="round"/>
  <line x1="26" y1="36" x2="26" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="31" x2="32" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="34" x2="38" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
</svg>""",
    "tray_idle_dark.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="none" stroke="#0f172a" stroke-width="5" stroke-linejoin="round"/>
  <line x1="26" y1="36" x2="26" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="31" x2="32" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="34" x2="38" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
</svg>""",
    "tray_recording.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="none" stroke="#38bdf8" stroke-width="4.5" stroke-linejoin="round"/>
  <line x1="26" y1="36" x2="26" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="31" x2="32" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="34" x2="38" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <circle cx="48" cy="18" r="7" fill="#ef4444"/>
</svg>""",
    "tray_recording_dark.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="none" stroke="#0f172a" stroke-width="5" stroke-linejoin="round"/>
  <line x1="26" y1="36" x2="26" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="31" x2="32" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="34" x2="38" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
  <circle cx="48" cy="18" r="7" fill="#dc2626"/>
</svg>""",
    "tray_transcribing.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="#0284c7" fill-opacity="0.4" stroke="#38bdf8" stroke-width="4.5" stroke-linejoin="round"/>
  <line x1="26" y1="33" x2="26" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="28" x2="32" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="31" x2="38" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
</svg>""",
    "tray_transcribing_dark.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="#0284c7" fill-opacity="0.2" stroke="#0f172a" stroke-width="5" stroke-linejoin="round"/>
  <line x1="26" y1="33" x2="26" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="28" x2="32" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="31" x2="38" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
</svg>""",
    "tray_idle_warning.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="none" stroke="#38bdf8" stroke-width="4.5" stroke-linejoin="round"/>
  <line x1="26" y1="36" x2="26" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="31" x2="32" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="34" x2="38" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <circle cx="48" cy="18" r="7" fill="#f59e0b"/>
  <line x1="48" y1="14" x2="48" y2="18" stroke="#000000" stroke-width="2" stroke-linecap="round"/>
  <circle cx="48" cy="21.5" r="1" fill="#000000"/>
</svg>""",
    "tray_idle_warning_dark.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="none" stroke="#0f172a" stroke-width="5" stroke-linejoin="round"/>
  <line x1="26" y1="36" x2="26" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="31" x2="32" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="34" x2="38" y2="42" stroke="#0284c7" stroke-width="3.5" stroke-linecap="round"/>
  <circle cx="48" cy="18" r="7" fill="#d97706"/>
  <line x1="48" y1="14" x2="48" y2="18" stroke="#ffffff" stroke-width="2" stroke-linecap="round"/>
  <circle cx="48" cy="21.5" r="1" fill="#ffffff"/>
</svg>""",
    "handy.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <defs>
    <linearGradient id="cloudGrad" x1="0%" y1="100%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#0284c7" />
      <stop offset="100%" stop-color="#38bdf8" />
    </linearGradient>
  </defs>
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="url(#cloudGrad)"/>
  <line x1="26" y1="36" x2="26" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="31" x2="32" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="34" x2="38" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
</svg>""",
    "recording.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <defs>
    <linearGradient id="cloudGrad" x1="0%" y1="100%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#0284c7" />
      <stop offset="100%" stop-color="#38bdf8" />
    </linearGradient>
  </defs>
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="url(#cloudGrad)"/>
  <line x1="26" y1="36" x2="26" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="31" x2="32" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="34" x2="38" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <circle cx="48" cy="18" r="7" fill="#ef4444"/>
</svg>""",
    "transcribing.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <defs>
    <linearGradient id="cloudGrad" x1="0%" y1="100%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#0284c7" />
      <stop offset="100%" stop-color="#38bdf8" />
    </linearGradient>
  </defs>
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="url(#cloudGrad)"/>
  <line x1="26" y1="33" x2="26" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="28" x2="32" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="31" x2="38" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
</svg>""",
    "handy_warning.png": """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <defs>
    <linearGradient id="cloudGrad" x1="0%" y1="100%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#0284c7" />
      <stop offset="100%" stop-color="#38bdf8" />
    </linearGradient>
  </defs>
  <path d="M18 46 C12 46, 10 40, 13 36 C12 30, 17 27, 22 28 C25 21, 35 21, 40 27 C45 25, 52 28, 51 34 C56 37, 54 46, 47 46 Z" fill="url(#cloudGrad)"/>
  <line x1="26" y1="36" x2="26" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="32" y1="31" x2="32" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <line x1="38" y1="34" x2="38" y2="42" stroke="#ffffff" stroke-width="3.5" stroke-linecap="round"/>
  <circle cx="48" cy="18" r="7" fill="#f59e0b"/>
  <line x1="48" y1="14" x2="48" y2="18" stroke="#000000" stroke-width="2" stroke-linecap="round"/>
  <circle cx="48" cy="21.5" r="1" fill="#000000"/>
</svg>""",
}

def generate_tray_icons():
    dest_dir = os.path.abspath("src-tauri/resources")
    os.makedirs(dest_dir, exist_ok=True)
    temp_dir = tempfile.mkdtemp(prefix="tray_gen_")

    try:
        for filename, svg_content in TRAY_SVGS.items():
            svg_path = os.path.join(temp_dir, f"{filename}.svg")
            out_dir = os.path.join(temp_dir, f"out_{filename}")
            os.makedirs(out_dir, exist_ok=True)
            with open(svg_path, "w", encoding="utf-8") as f:
                f.write(svg_content)

            cmd = ["bun", "run", "tauri", "icon", "--png", "64", svg_path, "-o", out_dir]
            res = subprocess.run(cmd, capture_output=True, text=True, shell=True)
            if res.returncode != 0:
                raise RuntimeError(f"tauri icon failed for {filename}: {res.stderr}")

            generated_png = os.path.join(out_dir, "64x64.png")
            if not os.path.exists(generated_png):
                raise FileNotFoundError(f"Expected generated PNG not found: {generated_png}")

            dest_path = os.path.join(dest_dir, filename)
            shutil.copyfile(generated_png, dest_path)
            print(f"Generated {filename} -> {dest_path}")
    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)

if __name__ == "__main__":
    generate_tray_icons()
