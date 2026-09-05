import os
import re
import subprocess
import hashlib
import shutil
from PIL import Image

def main():
    # 1. Read original hand path
    orig_tsx = subprocess.check_output(['git', 'show', '00d2554:src/components/icons/HandyHand.tsx']).decode('utf-8')
    hand_d = re.search(r'd=\"([^\"]+)\"', orig_tsx).group(1)

    # -----------------------------------------------------------------
    # Master App Icon (512x512) - Option 1-B: Doodle Hand + Cloud Cradle
    # -----------------------------------------------------------------
    # Color scheme:
    # Classic candy pink (#FAA2CA) hand with bold doodle stroke (#0f172a)
    # Fluffy white cloud cradle beneath the wrist base
    # Vibrant sky-blue soundwaves (#38bdf8) above fingertips
    # Heavy white die-cut sticker padding + soft ambient shadow

    hand_fill = "#FAA2CA"
    hand_stroke = "#0f172a"
    hand_hl = "#FCE7F3"
    cuff_fill = "#ffffff"
    cuff_shadow = "#e2e8f0"
    wave_color = "#38bdf8"

    # Coordinates:
    # Hand scaled 2.15, translate(118, 95)
    # Hand spans x: 128~378, y: 99~375
    # Soundwaves sit above fingertips: apex y: 22 - 76, centered at x: 256
    # Cloud Cradle sits beneath the wrist: y: 345 - 460, x: 80 - 432
    
    soundwaves_paths = '''
    <path d="M 226 78 C 242 66 270 66 286 78" />
    <path d="M 206 54 C 236 34 276 34 306 54" />
    <path d="M 186 30 C 230 4 282 4 326 30" />
    '''

    # Cloud Cradle beneath wrist
    cloud_path = '''
    M 135 410
    C 105 410 85 385 85 358
    C 85 330 108 308 136 308
    C 142 308 148 310 154 312
    C 166 280 198 258 236 258
    C 268 258 296 274 310 300
    C 322 290 338 284 356 284
    C 388 284 416 308 420 340
    C 436 348 448 366 448 386
    C 448 414 425 436 398 436
    C 390 436 382 434 375 430
    C 358 450 332 462 304 462
    C 276 462 252 450 238 432
    C 226 442 210 448 192 448
    C 166 448 144 432 135 410
    Z
    '''

    svg_content = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
    <defs>
        <filter id="stickerShadow" x="-20%" y="-20%" width="140%" height="140%">
            <feDropShadow dx="0" dy="14" stdDeviation="16" flood-color="#0f172a" flood-opacity="0.26"/>
            <feDropShadow dx="0" dy="4" stdDeviation="5" flood-color="#0f172a" flood-opacity="0.16"/>
        </filter>
    </defs>

    <!-- White Die-Cut Sticker Outline -->
    <g filter="url(#stickerShadow)">
        <g transform="translate(118, 95) scale(2.15)">
            <path d="{hand_d}" fill="#ffffff" stroke="#ffffff" stroke-width="22" stroke-linejoin="round" stroke-linecap="round"/>
        </g>
        <g stroke="#ffffff" stroke-width="36" stroke-linecap="round" fill="none">
            {soundwaves_paths}
        </g>
        <path d="{cloud_path}" fill="#ffffff" stroke="#ffffff" stroke-width="24" stroke-linejoin="round" stroke-linecap="round"/>
    </g>

    <!-- Doodle Hand Layer -->
    <g transform="translate(118, 95) scale(2.15)">
        <path d="{hand_d}" fill="{hand_fill}" stroke="{hand_stroke}" stroke-width="7" stroke-linejoin="round" stroke-linecap="round"/>
        <path d="{hand_d}" fill="none" stroke="{hand_hl}" stroke-width="2.5" opacity="0.75"/>
    </g>

    <!-- Voice Soundwaves Layer -->
    <g stroke="{wave_color}" stroke-width="14" stroke-linecap="round" fill="none">
        {soundwaves_paths}
    </g>

    <!-- Cloud Cradle Layer -->
    <path d="{cloud_path}" fill="{cuff_shadow}" transform="translate(0, 8)" opacity="0.85"/>
    <path d="{cloud_path}" fill="{cuff_fill}" stroke="{hand_stroke}" stroke-width="12" stroke-linejoin="round" stroke-linecap="round"/>
    <path d="M 115 360 A 26 26 0 0 1 150 330" fill="none" stroke="{cuff_shadow}" stroke-width="7" stroke-linecap="round"/>
    <path d="M 195 285 A 40 40 0 0 1 265 285" fill="none" stroke="{cuff_shadow}" stroke-width="8" stroke-linecap="round"/>
    <path d="M 345 315 A 30 30 0 0 1 390 338" fill="none" stroke="{cuff_shadow}" stroke-width="7" stroke-linecap="round"/>
</svg>'''

    svg_path = 'brand/handy-cloud-icon-source.svg'
    with open(svg_path, 'w', encoding='utf-8') as f:
        f.write(svg_content)
    print(f'Wrote {svg_path}')

    # 2. Render SVG to 512x512 PNG via tauri icon or resvg
    # We use bun run tauri icon to generate 512x512.png into a temp folder
    tmp_out = 'brand/tmp_render'
    subprocess.check_call(['bun', 'run', 'tauri', 'icon', svg_path, '--png', '512', '-o', tmp_out])
    src_png = os.path.join(tmp_out, '512x512.png')
    dst_png = 'brand/handy-cloud-icon-source.png'
    shutil.move(src_png, dst_png)
    shutil.rmtree(tmp_out, ignore_errors=True)
    print(f'Rendered {dst_png}')

    # 3. Calculate SHA-256 and update brand/P0_ICON_GENERATED.txt
    with open(dst_png, 'rb') as f:
        sha = hashlib.sha256(f.read()).hexdigest().lower()
    with open('brand/P0_ICON_GENERATED.txt', 'w', encoding='utf-8') as f:
        f.write(sha)
    print(f'Updated brand/P0_ICON_GENERATED.txt: {sha}')

    # 4. Run bun run tauri icon on the PNG to generate all native icons
    print('Generating full platform icon set with Tauri CLI...')
    subprocess.check_call(['bun', 'run', 'tauri', 'icon', dst_png, '-o', 'src-tauri/icons'])
    print('All platform icons generated successfully!')

if __name__ == '__main__':
    main()
