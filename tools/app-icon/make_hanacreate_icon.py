#!/usr/bin/env python3
from pathlib import Path
from PIL import Image, ImageChops, ImageOps

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path.home() / "HanaCreate/Resources/HanaCreateDesign/HanaCreate0.0.1Icon.jpg"
DEST = ROOT / "assets/app-icon"

def main() -> None:
    if not SOURCE.is_file():
        raise SystemExit(f"Missing HanaCreate master logo: {SOURCE}")
    image = Image.open(SOURCE).convert("RGB")
    image = ImageOps.fit(image, (1024, 1024), method=Image.Resampling.LANCZOS)
    DEST.mkdir(parents=True, exist_ok=True)
    for size in (256, 512, 1024):
        image.resize((size, size), Image.Resampling.LANCZOS).save(
            DEST / f"hanacreate-{size}.png", optimize=True
        )
    image.save(
        DEST / "hanacreate.ico",
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    android_res = ROOT / "android/app/src/main/res"
    mark = ImageOps.fit(image, (900, 900), method=Image.Resampling.LANCZOS).convert("RGBA")
    rgb = mark.convert("RGB")
    white = Image.new("RGB", rgb.size, (255, 255, 255))
    alpha = ImageChops.lighter(
        ImageChops.difference(rgb.getchannel("R"), white.getchannel("R")),
        ImageChops.lighter(
            ImageChops.difference(rgb.getchannel("G"), white.getchannel("G")),
            ImageChops.difference(rgb.getchannel("B"), white.getchannel("B")),
        ),
    )
    mark.putalpha(Image.eval(alpha, lambda value: min(255, value * 2)))
    mark_canvas = Image.new("RGBA", (1350, 1350), (0, 0, 0, 0))
    mark_canvas.alpha_composite(mark, (225, 225))
    nodpi = android_res / "drawable-nodpi"
    nodpi.mkdir(parents=True, exist_ok=True)
    mark_canvas.save(nodpi / "hanacreate_mark.png", optimize=True)
    for density, canvas_size in (("mdpi", 108), ("hdpi", 162), ("xhdpi", 216), ("xxhdpi", 324), ("xxxhdpi", 432)):
        foreground = Image.new("RGBA", (canvas_size, canvas_size), (0, 0, 0, 0))
        mark_size = round(canvas_size * 0.72)
        mark = ImageOps.fit(image, (mark_size, mark_size), method=Image.Resampling.LANCZOS).convert("RGBA")
        rgb = mark.convert("RGB")
        white = Image.new("RGB", rgb.size, (255, 255, 255))
        alpha = ImageChops.lighter(
            ImageChops.difference(rgb.getchannel("R"), white.getchannel("R")),
            ImageChops.lighter(
                ImageChops.difference(rgb.getchannel("G"), white.getchannel("G")),
                ImageChops.difference(rgb.getchannel("B"), white.getchannel("B")),
            ),
        )
        mark.putalpha(Image.eval(alpha, lambda value: min(255, value * 2)))
        offset = ((canvas_size - mark_size) // 2, (canvas_size - mark_size) // 2)
        foreground.alpha_composite(mark, offset)
        folder = android_res / f"drawable-{density}"
        folder.mkdir(parents=True, exist_ok=True)
        foreground.save(folder / "ic_launcher_foreground.png", optimize=True)
        monochrome = Image.new("RGBA", foreground.size, (255, 255, 255, 0))
        monochrome.putalpha(foreground.getchannel("A"))
        monochrome.save(folder / "ic_launcher_monochrome.png", optimize=True)

if __name__ == "__main__":
    main()
