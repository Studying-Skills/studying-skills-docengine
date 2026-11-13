// The wasm build of the PDF writer running in a browser: same bytes as native?
import http from 'node:http'; import fs from 'node:fs'; import path from 'node:path'; import crypto from 'node:crypto';
import { createRequire } from 'node:module';
const require = createRequire('/root/projects/studying-skills/studying-skills-frontend/package.json');
const pw = require('playwright');
const dir = path.dirname(new URL(import.meta.url).pathname);
const fonts = path.join(dir, '../../assets/fonts/liberation');
const server = http.createServer((req, res) => { const u = req.url.split('?')[0]; const f = u.startsWith('/fonts/') ? path.join(fonts, u.slice(7)) : path.join(dir, u === '/' ? 'empty.html' : u); fs.readFile(f, (e, b) => { if (e) { res.writeHead(200, { 'content-type': 'text/html' }); res.end('<!doctype html>'); return; } res.writeHead(200, { 'content-type': f.endsWith('.wasm') ? 'application/wasm' : f.endsWith('.js') ? 'text/javascript' : 'application/octet-stream' }); res.end(b); }); }).listen(0);
const b = await pw.chromium.launch(); const p = await b.newPage(); await p.goto(`http://127.0.0.1:${server.address().port}/`);
const r = await p.evaluate(async () => {
  const m = await import('/pkg/s5_pdf.js'); await m.default();
  const get = async (u) => new Uint8Array(await (await fetch(u)).arrayBuffer());
  const text = (await (await fetch('/lines.txt')).text()).replace(/\n$/, '');
  const [r, bo, it] = await Promise.all(['Regular', 'Bold', 'Italic'].map((n) => get(`/fonts/LiberationSerif-${n}.ttf`)));
  const t = performance.now(); const pdf = m.build_pdf(text, r, bo, it); const ms = performance.now() - t;
  return { ms, len: pdf.length, head: new TextDecoder().decode(pdf.slice(0, 8)), bytes: Array.from(pdf) };
});
const wasmPdf = Buffer.from(r.bytes); fs.writeFileSync(path.join(dir, 'out-wasm.pdf'), wasmPdf);
const native = fs.readFileSync(path.join(dir, 'out.pdf'));
const h = (x) => crypto.createHash('sha256').update(x).digest('hex').slice(0, 12);
console.log({ ms: Math.round(r.ms), len: r.len, head: r.head.trim(), nativeLen: native.length, sameBytes: h(wasmPdf) === h(native) });
await b.close(); server.close();
