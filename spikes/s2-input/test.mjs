// node test.mjs [browser] — exercises the hidden-editor input path in every Playwright browser.
import http from 'node:http'; import fs from 'node:fs'; import path from 'node:path';
import { createRequire } from 'node:module';
const require = createRequire('/root/projects/studying-skills/studying-skills-frontend/package.json');
const pw = require('playwright');
const dir = path.dirname(new URL(import.meta.url).pathname);
const types = { '.html': 'text/html', '.js': 'text/javascript' };
const server = http.createServer((req, res) => { const u = new URL(req.url, 'http://x'); const f = path.join(dir, u.pathname === '/' ? 'index.html' : u.pathname); fs.readFile(f, (e, b) => { if (e) { res.writeHead(404); res.end(); return; } res.writeHead(200, { 'content-type': types[path.extname(f)] ?? 'application/octet-stream' }); res.end(b); }); }).listen(0);
const url = `http://127.0.0.1:${server.address().port}/`;
const results = {};
const only = process.argv[2];
const S = (page, fn, ...a) => page.evaluate(([f, args]) => window.spike[f](...args), [fn, a]);

for (const name of ['chromium', 'firefox', 'webkit']) {
  if (only && only !== name) continue;
  const browser = await pw[name].launch();
  const R = (results[name] = {});
  const test = async (id, fn) => {
    const ctx = await browser.newContext(name === 'chromium' ? { permissions: ['clipboard-read', 'clipboard-write'] } : {});
    const page = await ctx.newPage();
    try { await page.goto(url); await page.waitForFunction(() => window.spike); const r = await fn(page, ctx); R[id] = r === true ? 'ok' : `FAIL ${JSON.stringify(r)}`; }
    catch (e) { R[id] = 'ERROR ' + String(e).split('\n')[0].slice(0, 200); }
    await ctx.close();
  };
  const mod = 'Control'; // Linux builds of all three engines
  // Native Shift+Arrow changes the DOM selection at once; ProseMirror's state follows on the next selectionchange.
  const synced = (page, to) => page.waitForFunction((t) => window.spike.selection().to === t, to, { timeout: 2000 });
  const clickAt = async (page, pos) => { const p = await S(page, 'pointOf', pos); await page.mouse.click(p.x, p.y); };

  await test('T1 focus on load, Ukrainian typing reaches the document and the painted layer', async (page) => {
    if (!(await S(page, 'focused'))) return 'not focused';
    await clickAt(page, 1);
    await page.keyboard.type('Привіт, світе! Її Єґ. ');
    const t = await S(page, 'text'), p = await S(page, 'paintedText');
    return t.startsWith('Привіт, світе! Її Єґ. Перший') && p === t ? true : { t: t.slice(0, 40), same: p === t };
  });
  await test('T2 click on the painted page moves the caret and keeps focus in the hidden editor', async (page) => {
    await clickAt(page, 30);
    const s = await S(page, 'selection'), f = await S(page, 'focused');
    await page.keyboard.type('Ж');
    const t = await S(page, 'text');
    return s.head === 30 && f && t[29] === 'Ж' ? true : { s, f, ch: t.slice(25, 33) };
  });
  await test('T3 the hidden DOM caret sits on the painted caret (IME window anchor)', async (page) => {
    const out = [];
    for (const pos of [1, 60, 120, 140, 200, 236]) { await clickAt(page, pos); const c = await S(page, 'carets'); out.push({ pos, dx: Math.abs(c.painted.x - c.hidden.x), dy: Math.abs(c.painted.y - c.hidden.y), dom: c.domSelectionInEditor }); }
    const bad = out.filter((o) => o.dx > 1.5 || o.dy > 3 || !o.dom);
    return bad.length ? bad : true;
  });
  await test('T4 drag selection over painted text selects the range and paints it', async (page) => {
    const a = await S(page, 'pointOf', 5), b = await S(page, 'pointOf', 100);
    await page.mouse.move(a.x, a.y); await page.mouse.down(); await page.mouse.move((a.x + b.x) / 2, b.y, { steps: 4 }); await page.mouse.move(b.x, b.y, { steps: 4 }); await page.mouse.up();
    const s = await S(page, 'selection'), n = await S(page, 'selRects'), f = await S(page, 'focused');
    return s.from === 5 && s.to === 100 && n > 1 && f ? true : { s, n, f };
  });
  await test('T5 keyboard selection, typing replaces it, undo restores', async (page) => {
    await clickAt(page, 1);
    const before = await S(page, 'text');
    for (let i = 0; i < 6; i++) await page.keyboard.press('Shift+ArrowRight');
    await synced(page, 7);
    const s = await S(page, 'selection');
    await page.keyboard.type('ЗАМІНА');
    const mid = await S(page, 'text');
    await page.keyboard.press(`${mod}+z`);
    const after = await S(page, 'text');
    return s.from === 1 && s.to === 7 && mid.startsWith('ЗАМІНА абзац') && after === before ? true : { s, mid: mid.slice(0, 20), undo: after === before };
  });
  await test('T6 Enter and Backspace', async (page) => {
    await clickAt(page, 8);
    await page.keyboard.press('Enter'); const a = (await S(page, 'text')).split('\n').length;
    await page.keyboard.press('Backspace'); const b = (await S(page, 'text')).split('\n').length;
    await page.keyboard.press('Backspace'); const t = await S(page, 'text');
    return a === 4 && b === 3 && t.startsWith('Першийабзац') ? true : { a, b, t: t.slice(0, 20) };
  });
  await test('T7 copy and paste through the hidden editor', async (page) => {
    await clickAt(page, 1);
    for (let i = 0; i < 6; i++) await page.keyboard.press('Shift+ArrowRight');
    await synced(page, 7);
    await page.keyboard.press(`${mod}+c`);
    await page.keyboard.press('ArrowLeft');
    await synced(page, 1);
    await page.keyboard.press(`${mod}+v`);
    const t = await S(page, 'text');
    return t.startsWith('ПершийПерший абзац') ? true : { t: t.slice(0, 30) };
  });
  if (name !== 'firefox') await test('T8 synthetic paste event with HTML (Firefox drops clipboardData of untrusted events; its real paste is T7)', async (page) => {
    await clickAt(page, 1);
    await page.evaluate(() => { const dt = new DataTransfer(); dt.setData('text/html', '<p>Вставлено <b>з Word</b></p>'); dt.setData('text/plain', 'Вставлено з Word'); window.spike.view.dom.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })); });
    const t = await S(page, 'text');
    return t.startsWith('Вставлено з Word') ? true : { t: t.slice(0, 40) };
  });
  await test('T9 the transparent editor is in the accessibility tree as a textbox with the text', async (page) => {
    const snap = await page.locator('#stage').ariaSnapshot();
    return /textbox "Звіт"/.test(snap) && snap.includes('Третій абзац') && !snap.includes('Третій абзац.\n  - paragraph: Третій абзац.') ? true : { snap: snap.slice(0, 300) };
  });
  await test('T14 typing right after Shift+Arrow (no wait) still replaces the right range', async (page) => {
    await clickAt(page, 1);
    for (let i = 0; i < 6; i++) await page.keyboard.press('Shift+ArrowRight');
    await page.keyboard.type('Я');
    const t = await S(page, 'text');
    return t.startsWith('Я абзац звіту') ? true : { t: t.slice(0, 20) };
  });
  await test('T15 word-wise and line-wise native movement reaches the state (Ctrl+Right, End, Home)', async (page) => {
    await clickAt(page, 1);
    await page.keyboard.press('Control+ArrowRight'); await page.waitForTimeout(80); const a = (await S(page, 'selection')).head;
    await page.keyboard.press('End'); await page.waitForTimeout(80); const b = (await S(page, 'selection')).head;
    await page.keyboard.press('Home'); await page.waitForTimeout(80); const c = (await S(page, 'selection')).head;
    return a >= 7 && a <= 8 && b > a && c === 1 ? true : { a, b, c };
  });
  for (const [id, q] of [['T16 own arrow commands: collapse right after a sync, 0 ms between keys', ''], ['T17 the same with native arrows (expected to fail in Chromium and WebKit)', '?native=1']]) {
    await test(id, async (page) => {
      await page.goto(url + q); await page.waitForFunction(() => window.spike);
      await clickAt(page, 1);
      for (let i = 0; i < 6; i++) await page.keyboard.press('Shift+ArrowRight');
      await synced(page, 7);
      await page.keyboard.press('ArrowLeft'); await page.waitForTimeout(300);
      const a = await S(page, 'selection');
      for (let i = 0; i < 10; i++) await page.keyboard.press('ArrowRight');
      await page.waitForTimeout(300);
      const b = await S(page, 'selection');
      return a.to === 1 && b.head === 11 ? true : { afterCollapse: a, afterTenRights: b };
    });
  }
  if (name === 'chromium') {
    await test('T10 IME composition (CDP): composing text is shown, the commit lands once', async (page, ctx) => {
      await clickAt(page, 1);
      const cdp = await ctx.newCDPSession(page);
      await cdp.send('Input.imeSetComposition', { text: 'に', selectionStart: 1, selectionEnd: 1 });
      await cdp.send('Input.imeSetComposition', { text: 'にほ', selectionStart: 2, selectionEnd: 2 });
      const during = await S(page, 'paintedText');
      const c = await S(page, 'carets');
      await cdp.send('Input.insertText', { text: '日本' });
      await page.waitForTimeout(50);
      const t = await S(page, 'text'), p = await S(page, 'paintedText'), log = await page.evaluate(() => window.spike.log);
      const ok = during.startsWith('にほПерший') && t.startsWith('日本Перший') && !t.includes('にほ') && p === t && Math.abs(c.painted.x - c.hidden.x) <= 1.5;
      return ok ? true : { during: during.slice(0, 12), t: t.slice(0, 12), same: p === t, log, c };
    });
    await test('T11 dead key as composition (´ + e → é), then typing continues', async (page, ctx) => {
      await clickAt(page, 1);
      const cdp = await ctx.newCDPSession(page);
      await cdp.send('Input.imeSetComposition', { text: '´', selectionStart: 1, selectionEnd: 1 });
      await cdp.send('Input.insertText', { text: 'é' });
      await page.keyboard.type('ї');
      const t = await S(page, 'text');
      return t.startsWith('éїПерший') ? true : { t: t.slice(0, 12) };
    });
    await test('T12 300 fast keystrokes: nothing lost or reordered', async (page) => {
      await clickAt(page, 1);
      const s = 'швидкийнабіртекстубезпробілів'.repeat(10) + 'кінець.';
      await page.keyboard.type(s, { delay: 0 });
      const t = await S(page, 'text');
      return t.startsWith(s + 'Перший') ? true : { got: t.slice(0, 60) };
    });
    await test('T13 Android emulation (touch, mobile UA): tap places the caret, insertText works', async () => {
      const ctx = await browser.newContext({ ...pw.devices['Pixel 7'] });
      const page = await ctx.newPage(); await page.goto(url); await page.waitForFunction(() => window.spike);
      const p = await S(page, 'pointOf', 30); await page.touchscreen.tap(p.x, p.y);
      const s = await S(page, 'selection'); const f = await S(page, 'focused');
      await page.keyboard.insertText('Ґ'); const t = await S(page, 'text'); await ctx.close();
      return f && t.includes('Ґ') ? true : { s, f, t: t.slice(20, 40) };
    });
  }
  await browser.close();
}
server.close();
const ids = [...new Set(Object.values(results).flatMap((r) => Object.keys(r)))];
for (const id of ids) { console.log(id); for (const b of Object.keys(results)) if (results[b][id]) console.log(`   ${b.padEnd(9)} ${results[b][id]}`); }
fs.writeFileSync(path.join(dir, 'results.json'), JSON.stringify(results, null, 1));
