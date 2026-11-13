import http from 'node:http'; import fs from 'node:fs'; import path from 'node:path';
import { createRequire } from 'node:module';
const require = createRequire('/root/projects/studying-skills/studying-skills-frontend/package.json');
const pw = require('playwright');
const dir = path.dirname(new URL(import.meta.url).pathname);
const server = http.createServer((req, res) => { const f = req.url === '/font.ttf' ? path.join(dir, '../../assets/fonts/liberation/LiberationSerif-Regular.ttf') : path.join(dir, 'page.html'); fs.readFile(f, (e, b) => { res.writeHead(200, { 'content-type': req.url === '/font.ttf' ? 'font/ttf' : 'text/html' }); res.end(b); }); }).listen(0);
fs.mkdirSync(path.join(dir, 'shots'), { recursive: true });
for (const name of ['chromium', 'firefox', 'webkit']) {
  const b = await pw[name].launch(); const ctx = await b.newContext({ deviceScaleFactor: 2, viewport: { width: 900, height: 1200 } }); const p = await ctx.newPage();
  await p.goto(`http://127.0.0.1:${server.address().port}/`); await p.evaluate(() => document.fonts.ready); await p.waitForTimeout(300);
  for (const i of [0, 1, 2]) await p.locator(`#p${i}`).screenshot({ path: path.join(dir, 'shots', `${name}-${i}.png`) });
  await b.close();
}
server.close();
