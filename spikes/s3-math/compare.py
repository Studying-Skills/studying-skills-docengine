"""Compare formula ink in the PDF raster (pdftoppm -r 192) with browser screenshots at 2x: bounding boxes and overlap."""
import json, sys
from PIL import Image
items = json.load(open("layout.json"))
K = 192 / 72  # px per pt
def ink(img, box):
    c = img.crop(box).convert("L").point(lambda v: 255 if v < 140 else 0)
    return c, c.getbbox()
rows = []
for name in ["chromium", "firefox", "webkit"]:
    worst = 0; ious = []; bad = []
    for it in items:
        pdf = Image.open(f"shots/pdf-{it['page'] + 1}.png"); dom = Image.open(f"shots/{name}-{it['page']}.png")
        if dom.size != pdf.size: dom = dom.resize(pdf.size)
        pad = 6
        box = (int(it["x"] * K) - pad, int(it["top"] * K) - pad, int((it["x"] + it["w"]) * K) + pad, int((it["top"] + it["h"]) * K) + pad)
        a, ba = ink(pdf, box); b, bb = ink(dom, box)
        if not ba or not bb: bad.append((it["i"] + 1, "no ink", ba, bb)); continue
        d = max(abs(x - y) for x, y in zip(ba, bb)); worst = max(worst, d)
        pa, pb = a.load(), b.load(); inter = union = 0
        for y in range(a.size[1]):
            for x in range(a.size[0]):
                u, v = pa[x, y] > 0, pb[x, y] > 0
                inter += u and v; union += u or v
        iou = inter / union if union else 1; ious.append(iou)
        if d > 2: bad.append((it["i"] + 1, it["latex"][:40], "bbox diff px", d, "iou", round(iou, 2)))
    print(f"{name}: formulas {len(items)}, worst bbox edge diff {worst} px = {worst / K:.2f} pt, ink IoU min {min(ious):.2f} mean {sum(ious) / len(ious):.2f}")
    for x in bad: print("   ", x)
