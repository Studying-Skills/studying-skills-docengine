// Shared numbers for both renderers: where every formula box goes (pt). 1 ex = 0.5 em = 7 pt at 14 pt text.
import fs from 'node:fs';
const F = JSON.parse(fs.readFileSync(new URL('./formulas.json', import.meta.url)));
const EX = 7, PER_PAGE = 10, PITCH = 70, X = 120;
const items = F.map((f, i) => {
  const w = f.widthEx * EX, h = f.heightEx * EX, depth = -f.valignEx * EX;
  const baseline = 80 + (i % PER_PAGE) * PITCH;
  return { i, page: Math.floor(i / PER_PAGE), x: X, top: baseline - (h - depth), w, h, baseline, svg: f.svg.replaceAll('currentColor', '#000'), latex: f.latex };
});
fs.writeFileSync(new URL('./layout.json', import.meta.url), JSON.stringify(items));
const pages = [0, 1, 2].map((p) => `<div class="page" id="p${p}">` + items.filter((it) => it.page === p).map((it) =>
  `<span class="n" style="left:60pt;top:${it.baseline - 14}pt">(${it.i + 1})</span>` +
  (process.argv[2] === 'inline'
    ? it.svg.replace(/^<svg style="[^"]*"/, `<svg style="left:${it.x}pt;top:${it.top}pt;width:${it.w}pt;height:${it.h}pt"`).replace(/ width="[^"]*" height="[^"]*"/, ' preserveAspectRatio="none"')
    : `<img style="left:${it.x}pt;top:${it.top}pt;width:${it.w}pt;height:${it.h}pt" src="data:image/svg+xml;base64,${Buffer.from(it.svg).toString('base64')}">`)).join('') + '</div>').join('\n');
fs.writeFileSync(new URL('./page.html', import.meta.url), `<!doctype html><meta charset="utf-8"><style>
@font-face{font-family:LS;src:url(/font.ttf)}
body{margin:0}.page{position:relative;width:595.276pt;height:841.89pt;overflow:hidden;background:#fff}
.page>*{position:absolute}.n{font:14pt/14pt LS;text-rendering:geometricPrecision}
</style>${pages}`);
console.log(items.length, 'items');
