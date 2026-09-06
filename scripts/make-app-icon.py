#!/usr/bin/env python3
"""產生 app icon（佔位版）。

刻意用腳本產生而不是丟一個來歷不明的 .ico 進 repo：
圖是程式碼的產物，任何人都能重跑、能改、能 review diff。

圖形：深色圓角方塊 + 一張帶鋸齒撕邊的收據 + 一點綠色（代表「印出來了」）。
選這個主題是因為這個專案的第一原則就是「單一定要印得出來、而且要對」。

用法：
    python scripts/make-app-icon.py
輸出：
    src-tauri/icons/icon.ico
    src-tauri/icons/{32x32,128x128,128x128@2x,256x256}.png
"""

from __future__ import annotations

import pathlib

from PIL import Image, ImageDraw

OUT = pathlib.Path(__file__).resolve().parent.parent / "src-tauri" / "icons"

BG = (15, 23, 42, 255)  # slate-900
PAPER = (248, 250, 252, 255)  # slate-50
INK = (100, 116, 139, 255)  # slate-500
ACCENT = (34, 197, 94, 255)  # green-500


def render(size: int) -> Image.Image:
    """在 4 倍解析度下畫再縮小，得到乾淨的邊緣（PIL 沒有內建反鋸齒繪圖）。"""
    s = size * 4
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # 底：圓角方塊
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=int(s * 0.22), fill=BG)

    # 收據本體
    pad_x, top = int(s * 0.28), int(s * 0.18)
    right, bottom = s - pad_x, int(s * 0.74)
    d.rectangle([pad_x, top, right, bottom], fill=PAPER)

    # 撕邊（三角鋸齒）
    teeth = 6
    w = (right - pad_x) / teeth
    h = int(s * 0.06)
    for i in range(teeth):
        x0 = pad_x + i * w
        d.polygon(
            [(x0, bottom), (x0 + w / 2, bottom + h), (x0 + w, bottom)],
            fill=PAPER,
        )

    # 明細行
    line_h = max(2, int(s * 0.022))
    y = top + int(s * 0.10)
    for frac in (0.72, 0.56, 0.64):
        d.rounded_rectangle(
            [pad_x + int(s * 0.06), y, pad_x + int((right - pad_x) * frac), y + line_h],
            radius=line_h // 2,
            fill=INK,
        )
        y += int(s * 0.09)

    # 合計那一條用綠色 —— 代表這張單真的印出去了
    d.rounded_rectangle(
        [pad_x + int(s * 0.06), y, pad_x + int((right - pad_x) * 0.40), y + line_h],
        radius=line_h // 2,
        fill=ACCENT,
    )

    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    for name, size in [
        ("32x32.png", 32),
        ("128x128.png", 128),
        ("128x128@2x.png", 256),
        ("256x256.png", 256),
    ]:
        render(size).save(OUT / name)

    # Windows 需要 icon.ico；tauri-build 會用它產生資源檔。
    sizes = [16, 24, 32, 48, 64, 128, 256]
    render(256).save(OUT / "icon.ico", sizes=[(n, n) for n in sizes])

    # macOS 用 .icns，Linux 用 png。沒有 macOS 環境時 tauri 會在打包階段自己處理，
    # 這裡先備妥最大張的 png 即可。
    render(512).save(OUT / "icon.png")

    print(f"已寫入 {OUT}")


if __name__ == "__main__":
    main()
