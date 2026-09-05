import os
from PIL import Image, ImageDraw

def generate_tray_icons():
    resources_dir = 'src-tauri/resources'
    os.makedirs(resources_dir, exist_ok=True)

    # 1. Load the exact upstream hollow hand baseline (64x64)
    orig_path = 'brand/upstream-tray_idle.png'
    if not os.path.exists(orig_path):
        import subprocess, io
        data = subprocess.check_output(['git', 'show', '00d2554:src-tauri/resources/tray_idle.png'])
        orig = Image.open(io.BytesIO(data)).convert('RGBA')
        orig.save(orig_path)
    else:
        orig = Image.open(orig_path).convert('RGBA')

    w, h = orig.size  # 64, 64

    # -------------------------------------------------------------
    # Option 1-B: Exact Upstream Hollow Hand + Floating Cloud Cradle
    # -------------------------------------------------------------
    # In Option 1-B, the upstream hand and flat wrist block remain 100% intact.
    # Beneath the wrist (y=48~61, x=13~53), a soft, puffy 3-lobe cloud cradle supports it.
    
    def make_base_cradle(fg_color):
        base = Image.new('RGBA', (w, h), (0, 0, 0, 0))
        # Draw the hand in fg_color
        for y in range(h):
            for x in range(w):
                r, g, b, a = orig.getpixel((x, y))
                if a > 0:
                    base.putpixel((x, y), (fg_color[0], fg_color[1], fg_color[2], a))

        # Add the cloud cradle beneath the wrist base (y=48 to 61)
        draw = ImageDraw.Draw(base)
        # Left cloud lobe
        draw.ellipse([13, 49, 29, 61], fill=fg_color)
        # Center cloud lobe (largest)
        draw.ellipse([25, 47, 43, 62], fill=fg_color)
        # Right cloud lobe
        draw.ellipse([39, 49, 53, 61], fill=fg_color)
        return base

    white = (255, 255, 255, 255)
    dark_gray = (30, 41, 59, 255) # #1e293b for light taskbar

    base_dark = make_base_cradle(white)      # for dark taskbars (white icons)
    base_light = make_base_cradle(dark_gray) # for light taskbars (dark icons)

    # 1. Idle States
    base_dark.save(os.path.join(resources_dir, 'tray_idle.png'))
    base_light.save(os.path.join(resources_dir, 'tray_idle_dark.png'))

    # 2. Recording States (Vivid red dot & glow at fingertips: x=33, y=8)
    def apply_recording(base_img, is_dark):
        img = base_img.copy()
        draw = ImageDraw.Draw(img)
        # Red aura
        aura_col = (239, 68, 68, 90) if is_dark else (220, 38, 38, 90)
        dot_col = (239, 68, 68, 255) if is_dark else (220, 38, 38, 255)
        draw.ellipse([27, 2, 39, 14], fill=aura_col)
        draw.ellipse([29, 4, 37, 12], fill=dot_col)
        draw.ellipse([32, 7, 34, 9], fill=(255, 255, 255, 255))
        return img

    apply_recording(base_dark, True).save(os.path.join(resources_dir, 'tray_recording.png'))
    apply_recording(base_light, False).save(os.path.join(resources_dir, 'tray_recording_dark.png'))

    # 3. Transcribing States (Electric sky-blue voice waves above fingertips)
    def apply_transcribing(base_img, is_dark):
        img = base_img.copy()
        draw = ImageDraw.Draw(img)
        arc_col1 = (56, 189, 248, 255) if is_dark else (2, 132, 199, 255) # #38bdf8 / #0284c7
        arc_col2 = (14, 165, 233, 255) if is_dark else (3, 105, 161, 255) # #0ea5e9 / #0369a1
        # Two clean arc waves centered over the dual fingertips
        draw.arc([23, 2, 43, 14], start=200, end=340, fill=arc_col1, width=3)
        draw.arc([20, -3, 46, 11], start=200, end=340, fill=arc_col2, width=2)
        return img

    apply_transcribing(base_dark, True).save(os.path.join(resources_dir, 'tray_transcribing.png'))
    apply_transcribing(base_light, False).save(os.path.join(resources_dir, 'tray_transcribing_dark.png'))

    # 4. Warning States (Amber badge at bottom right)
    def apply_warning(base_img):
        img = base_img.copy()
        draw = ImageDraw.Draw(img)
        # Amber badge (circle with exclamation point)
        draw.ellipse([44, 43, 58, 57], fill=(245, 158, 11, 255))
        draw.rectangle([50, 46, 52, 51], fill=(255, 255, 255, 255))
        draw.rectangle([50, 53, 52, 54], fill=(255, 255, 255, 255))
        return img

    apply_warning(base_dark).save(os.path.join(resources_dir, 'tray_idle_warning.png'))
    apply_warning(base_light).save(os.path.join(resources_dir, 'tray_idle_warning_dark.png'))

    # 5. Linux Colored Icons (handy.png, recording.png, transcribing.png)
    # Using classic candy pink hand outline with white cloud cradle
    pink = (250, 162, 202, 255)
    linux_base = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    for y in range(h):
        for x in range(w):
            r, g, b, a = orig.getpixel((x, y))
            if a > 0:
                linux_base.putpixel((x, y), (pink[0], pink[1], pink[2], a))

    draw_linux = ImageDraw.Draw(linux_base)
    draw_linux.ellipse([13, 49, 29, 61], fill=white)
    draw_linux.ellipse([25, 47, 43, 62], fill=white)
    draw_linux.ellipse([39, 49, 53, 61], fill=white)

    linux_base.save(os.path.join(resources_dir, 'handy.png'))
    apply_recording(linux_base, True).save(os.path.join(resources_dir, 'recording.png'))
    apply_transcribing(linux_base, True).save(os.path.join(resources_dir, 'transcribing.png'))

    print("Option 1-B tray icons successfully generated in src-tauri/resources/")

if __name__ == '__main__':
    generate_tray_icons()
