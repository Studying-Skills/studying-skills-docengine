// Spike S2: all typing goes into a transparent ProseMirror; the user sees and clicks a separately painted layer.
import { EditorState, Selection, TextSelection } from 'prosemirror-state';
import { EditorView } from 'prosemirror-view';
import { schema } from 'prosemirror-schema-basic';
import { keymap } from 'prosemirror-keymap';
import { history, undo, redo } from 'prosemirror-history';
import { baseKeymap } from 'prosemirror-commands';

const pages = document.getElementById('pages');
const overlay = document.getElementById('overlay');
const hidden = document.getElementById('hidden');
const stage = document.getElementById('stage');

const doc = schema.node('doc', null, [
  schema.node('paragraph', null, [schema.text('Перший абзац звіту. Тут достатньо тексту, щоб він зайняв кілька рядків на вузькій сторінці й перевірив переходи між рядками.')]),
  schema.node('paragraph', null, [schema.text('Другий абзац: English words, цифри 12345 і розділові знаки — «лапки», апостроф у слові п’ять.')]),
  schema.node('paragraph', null, [schema.text('Третій абзац.')]),
]);

/** Painted paragraphs: index → { el, text node, start position of its text in the document }. */
let painted = [];

function paint(state, composing) {
  painted = [];
  const els = [];
  state.doc.forEach((node, offset) => {
    const p = document.createElement('p');
    const t = document.createTextNode(node.textContent || '\u200b');
    p.append(t);
    if (composing) p.dataset.composing = '1';
    els.push(p);
    painted.push({ el: p, text: t, start: offset + 1, size: node.content.size });
  });
  pages.replaceChildren(...els);
}

/** Document position → { node, offset } in the painted layer. This is what the layout engine answers in the real build. */
function domAt(pos) {
  for (const p of painted) if (pos >= p.start && pos <= p.start + p.size) return { node: p.text, offset: pos - p.start };
  const last = painted[painted.length - 1];
  return { node: last.text, offset: last.size };
}

function rectsOf(from, to) {
  const r = document.createRange();
  const a = domAt(from), b = domAt(to);
  r.setStart(a.node, a.offset); r.setEnd(b.node, b.offset);
  return [...r.getClientRects()];
}

function caretRect(pos) {
  const { node, offset } = domAt(pos);
  const r = document.createRange();
  // A collapsed range has no rect in some engines: measure the character before or after.
  if (offset > 0) { r.setStart(node, offset - 1); r.setEnd(node, offset); const q = [...r.getClientRects()].pop(); if (q) return { x: q.right, y: q.top, h: q.height }; }
  r.setStart(node, offset); r.setEnd(node, Math.min(offset + 1, node.length));
  const q = r.getClientRects()[0] ?? node.parentElement.getBoundingClientRect();
  return { x: q.left, y: q.top, h: q.height };
}

/** Viewport point → document position (the engine's hit test in the real build). */
function posAt(x, y) {
  let node, offset;
  if (document.caretPositionFromPoint) { const c = document.caretPositionFromPoint(x, y); if (!c) return null; node = c.offsetNode; offset = c.offset; }
  else { const r = document.caretRangeFromPoint(x, y); if (!r) return null; node = r.startContainer; offset = r.startOffset; }
  const p = painted.find((q) => q.text === node || q.el === node);
  if (!p) return null;
  return p.start + Math.min(node === p.el ? p.size : offset, p.size);
}

function paintSelection(state) {
  const { from, to, head } = state.selection;
  const s = stage.getBoundingClientRect();
  const parts = [];
  if (from !== to) for (const r of rectsOf(from, to)) { const d = document.createElement('div'); d.className = 'sel'; d.style.cssText = `left:${r.left - s.left}px;top:${r.top - s.top}px;width:${r.width}px;height:${r.height}px`; parts.push(d); }
  const c = caretRect(head);
  const caret = document.createElement('div'); caret.id = 'caret'; caret.style.cssText = `left:${c.x - s.left}px;top:${c.y - s.top}px;height:${c.h}px`;
  parts.push(caret);
  overlay.replaceChildren(...parts);
  return c;
}

/** Move the transparent editor so that its own DOM caret sits on the painted caret (IME windows follow the DOM caret). */
function alignHidden(view, painted) {
  hidden.style.transform = 'none';
  let own;
  try { own = view.coordsAtPos(view.state.selection.head); } catch { return; }
  hidden.style.transform = `translate(${painted.x - own.left}px, ${painted.y - own.top}px)`;
}

/**
 * Horizontal movement as commands on the state, not native caret movement: a native arrow key pressed within ~50 ms of
 * the previous selection sync is reverted by ProseMirror in Chromium and WebKit (measured in this spike, see README).
 * `?native=1` switches these off to reproduce that.
 */
function move(dir, extend) {
  return (state, dispatch) => {
    const { selection: sel, doc } = state;
    if (!extend && !sel.empty) { dispatch(state.tr.setSelection(TextSelection.create(doc, dir < 0 ? sel.from : sel.to))); return true; }
    const target = Math.max(0, Math.min(doc.content.size, sel.head + dir));
    const near = Selection.near(doc.resolve(target), dir);
    const head = near.head;
    dispatch(state.tr.setSelection(TextSelection.create(doc, extend ? sel.anchor : head, head)));
    return true;
  };
}
const ownKeys = new URLSearchParams(location.search).has('native') ? {} : { ArrowLeft: move(-1, false), ArrowRight: move(1, false), 'Shift-ArrowLeft': move(-1, true), 'Shift-ArrowRight': move(1, true) };

const log = [];
const view = new EditorView(hidden, {
  state: EditorState.create({ doc, plugins: [history(), keymap(ownKeys), keymap({ 'Mod-z': undo, 'Mod-y': redo, 'Shift-Mod-z': redo }), keymap(baseKeymap)] }),
  attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': 'Звіт', spellcheck: 'false' },
  dispatchTransaction(tr) {
    const state = view.state.apply(tr);
    view.updateState(state);
    refresh();
  },
});

function refresh() {
  paint(view.state, view.composing);
  const c = paintSelection(view.state);
  alignHidden(view, c);
}
refresh();
view.focus();

// Mouse on the painted layer: never let the browser move focus, translate the point to a position.
let anchor = null;
pages.addEventListener('mousedown', (ev) => {
  ev.preventDefault();
  const pos = posAt(ev.clientX, ev.clientY); if (pos == null) return;
  anchor = ev.shiftKey ? view.state.selection.anchor : pos;
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, anchor, pos)));
  view.focus();
});
window.addEventListener('mousemove', (ev) => {
  if (anchor == null) return;
  const pos = posAt(ev.clientX, ev.clientY); if (pos == null) return;
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, anchor, pos)));
});
window.addEventListener('mouseup', () => { anchor = null; view.focus(); });
view.dom.addEventListener('compositionstart', () => log.push('compositionstart'));
view.dom.addEventListener('compositionupdate', (e) => { log.push('compositionupdate:' + e.data); });
view.dom.addEventListener('compositionend', (e) => { log.push('compositionend:' + e.data); setTimeout(refresh, 0); });

// Hooks for the test runner.
window.spike = {
  view, log,
  text: () => view.state.doc.textBetween(0, view.state.doc.content.size, '\n'),
  paintedText: () => painted.map((p) => p.text.data.replace('\u200b', '')).join('\n'),
  selection: () => ({ from: view.state.selection.from, to: view.state.selection.to, head: view.state.selection.head }),
  focused: () => document.activeElement === view.dom,
  carets: () => {
    const s = getSelection(); const own = view.coordsAtPos(view.state.selection.head);
    const c = document.getElementById('caret').getBoundingClientRect();
    return { painted: { x: c.left, y: c.top }, hidden: { x: own.left, y: own.top }, domSelectionInEditor: !!s.anchorNode && view.dom.contains(s.anchorNode) };
  },
  pointOf: (pos) => { const c = caretRect(pos); return { x: c.x, y: c.y + c.h / 2 }; },
  selRects: () => document.querySelectorAll('.sel').length,
};
