// node run.mjs  — serves the spike and measures it in every installed Playwright browser.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
const require = createRequire('/root/projects/studying-skills/studying-skills-frontend/package.json');
const pw = require('playwright');
const dir = path.dirname(new URL(import.meta.url).pathname);
const FONT = path.join(dir, '../../assets/fonts/liberation/LiberationSerif-Regular.ttf');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json' };
const server = http.createServer((req, res) => {
  const u = new URL(req.url, 'http://x');
  const file = u.pathname === '/font.ttf' ? FONT : path.join(dir, u.pathname === '/' ? 'index.html' : u.pathname);
  fs.readFile(file, (e, b) => { if (e) { res.writeHead(404); res.end(); return; } res.writeHead(200, { 'content-type': types[path.extname(file)] ?? 'application/octet-stream' }); res.end(b); });
}).listen(0);
const port = server.address().port;
const out = [];
const only = process.argv[2];
for (const name of ['chromium', 'firefox', 'webkit']) {
  if (only && only !== name) continue;
  let browser;
  try { browser = await pw[name].launch(); } catch (e) { out.push({ browser: name, error: String(e).split('\n')[0] }); continue; }
  for (const dpr of [1, 1.25, 1.5, 2]) for (const rendering of ['auto', 'geometricPrecision']) for (const size of ['18.666666', '16']) {
    const ctx = await browser.newContext({ deviceScaleFactor: dpr });
    const page = await ctx.newPage();
    try {
      await page.goto(`http://127.0.0.1:${port}/?size=${size}&rendering=${rendering}`);
      const r = await page.evaluate(() => window.run());
      out.push({ browser: name, ...r });
      const f = (x) => x.toFixed(3);
      console.log(`${name} dpr=${dpr} ${rendering} ${size}px | word mean ${f(r.word.mean)} p99 ${f(r.word.p99)} max ${f(r.word.max)} | line p99 ${f(r.line.p99)} max ${f(r.line.max)} | worst "${r.worst.word}" b=${f(r.worst.browser)} e=${f(r.worst.engine)} | layout ${r.bench.lines} lines cold ${f(r.bench.coldMs)} warm ${f(r.bench.warmMs)} edit p99 ${f(r.bench.edit.p99)} max ${f(r.bench.edit.max)} | skipped ${r.skipped} | worstLine ${f(r.worstLine.d)} "${r.worstLine.line}"`);
    } catch (e) { console.log(name, dpr, rendering, size, 'ERR', String(e).slice(0,600)); }
    await ctx.close();
  }
  await browser.close();
}
fs.writeFileSync(path.join(dir, 'results.json'), JSON.stringify(out, null, 1));
server.close();
