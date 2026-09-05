import os
import subprocess
import shutil
from PIL import Image

CLOUD = 'M 18 44 C 11 44 8 38 8 33 C 8 28 12 24 17 23 C 18 16 25 12 33 12 C 40 12 46 16 48 22 C 53 22 57 26 57 31 C 57 36 53 44 46 44 Z'

def make_svg(cloud_color, content):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <path d="{CLOUD}" fill="{cloud_color}"/>
  {content}
</svg>'''

# 1. Idle Dark (white cloud, dark cut bars)
idle_bars_dark = '''
  <rect x="25" y="26" width="3" height="12" rx="1.5" fill="#0f172a"/>
  <rect x="30.5" y="21" width="3" height="18" rx="1.5" fill="#0f172a"/>
  <rect x="36" y="26" width="3" height="12" rx="1.5" fill="#0f172a"/>
'''
# 2. Idle Light (dark cloud, white bars)
idle_bars_light = '''
  <rect x="25" y="26" width="3" height="12" rx="1.5" fill="#ffffff"/>
  <rect x="30.5" y="21" width="3" height="18" rx="1.5" fill="#ffffff"/>
  <rect x="36" y="26" width="3" height="12" rx="1.5" fill="#ffffff"/>
'''

# 3. Recording (red dot + ring)
rec_dark = '''
  <circle cx="32" cy="30" r="10" fill="none" stroke="#ef4444" stroke-width="2" stroke-opacity="0.6"/>
  <circle cx="32" cy="30" r="6.5" fill="#ef4444"/>
'''
rec_colored = '''
  <circle cx="32" cy="30" r="10" fill="none" stroke="#ffffff" stroke-width="2" stroke-opacity="0.9"/>
  <circle cx="32" cy="30" r="6.5" fill="#ef4444"/>
'''

# 4. Transcribing (5 bars)
trans_dark = '''
  <rect x="21" y="27" width="3" height="9" rx="1.5" fill="#0284c7"/>
  <rect x="25.5" y="22" width="3" height="16" rx="1.5" fill="#0284c7"/>
  <rect x="30.5" y="18" width="3" height="22" rx="1.5" fill="#0284c7"/>
  <rect x="35.5" y="22" width="3" height="16" rx="1.5" fill="#0284c7"/>
  <rect x="40" y="27" width="3" height="9" rx="1.5" fill="#0284c7"/>
'''
trans_light = '''
  <rect x="21" y="27" width="3" height="9" rx="1.5" fill="#38bdf8"/>
  <rect x="25.5" y="22" width="3" height="16" rx="1.5" fill="#38bdf8"/>
  <rect x="30.5" y="18" width="3" height="22" rx="1.5" fill="#38bdf8"/>
  <rect x="35.5" y="22" width="3" height="16" rx="1.5" fill="#38bdf8"/>
  <rect x="40" y="27" width="3" height="9" rx="1.5" fill="#38bdf8"/>
'''
trans_colored = '''
  <rect x="21" y="27" width="3" height="9" rx="1.5" fill="#ffffff"/>
  <rect x="25.5" y="22" width="3" height="16" rx="1.5" fill="#ffffff"/>
  <rect x="30.5" y="18" width="3" height="22" rx="1.5" fill="#ffffff"/>
  <rect x="35.5" y="22" width="3" height="16" rx="1.5" fill="#ffffff"/>
  <rect x="40" y="27" width="3" height="9" rx="1.5" fill="#ffffff"/>
'''

# 5. Warning badge
def warning_badge(backing):
    return f'''
  <circle cx="48" cy="40" r="9.5" fill="{backing}"/>
  <circle cx="48" cy="40" r="8" fill="#f59e0b"/>
  <line x1="48" y1="35.5" x2="48" y2="40.5" stroke="#ffffff" stroke-width="2.2" stroke-linecap="round"/>
  <circle cx="48" cy="44" r="1.2" fill="#ffffff"/>
'''

specs = {
    'tray_idle.png': make_svg('#ffffff', idle_bars_dark),
    'tray_idle_dark.png': make_svg('#1e293b', idle_bars_light),
    'tray_recording.png': make_svg('#ffffff', rec_dark),
    'tray_recording_dark.png': make_svg('#1e293b', rec_dark),
    'tray_transcribing.png': make_svg('#ffffff', trans_dark),
    'tray_transcribing_dark.png': make_svg('#1e293b', trans_light),
    'tray_idle_warning.png': make_svg('#ffffff', idle_bars_dark + warning_badge('#0f172a')),
    'tray_idle_warning_dark.png': make_svg('#1e293b', idle_bars_light + warning_badge('#ffffff')),
    'handy.png': make_svg('#0ea5e9', idle_bars_light),
    'recording.png': make_svg('#0ea5e9', rec_colored),
    'transcribing.png': make_svg('#0ea5e9', trans_colored),
    'handy_warning.png': make_svg('#0ea5e9', idle_bars_light + warning_badge('#0f172a')),
}

def main():
    repo_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    tmp_build = os.path.join(repo_root, 'tmp_tray_build')
    resources_dir = os.path.join(repo_root, 'src-tauri', 'resources')
    os.makedirs(tmp_build, exist_ok=True)
    os.makedirs(resources_dir, exist_ok=True)

    try:
        for name, svg_content in specs.items():
            svg_path = os.path.join(tmp_build, name.replace('.png', '.svg'))
            with open(svg_path, 'w', encoding='utf-8') as f:
                f.write(svg_content)

            out_dir = os.path.join(tmp_build, 'out_' + name)
            cmd = ['bun', 'run', 'tauri', 'icon', svg_path, '--png', '64', '-o', out_dir]
            subprocess.check_call(cmd, cwd=repo_root, stdout=subprocess.DEVNULL)
            src_png = os.path.join(out_dir, '64x64.png')
            dst_png = os.path.join(resources_dir, name)
            shutil.copyfile(src_png, dst_png)
        print('Successfully generated all 12 tray icon assets in src-tauri/resources/')
    finally:
        shutil.rmtree(tmp_build, ignore_errors=True)

if __name__ == '__main__':
    main()
