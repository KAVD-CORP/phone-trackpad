// Headless check of the embedded Safari page gesture engine.
// Extracts the <script> from web_page.html, runs it once against a tiny DOM
// shim, completes the PIN gate (so token != null), then feeds pointer
// sequences and asserts the wire messages.
const fs = require('fs');
const html = fs.readFileSync('crates/trackpad-desktop/src/web_page.html', 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];

let clock = 1000;
const listeners = {};
const captured = [];
const pinned = {}; // captures element refs and their wired handlers

const padEl = {
  addEventListener: (k, f) => { (listeners[k] = listeners[k] || []).push(f); },
  setPointerCapture: () => {},
};

function elBy(id) {
  if (id === 'pad') return padEl;
  if (pinned[id]) return pinned[id];
  return pinned[id] = id === 'pin'
    ? { value: '123456' }
    : { textContent: '', style: {}, classList: { add() {}, remove() {} }, onclick: null };
}

const docShim = { getElementById: elBy, addEventListener: () => {} };

new Function('document', 'window', 'performance', 'fetch', 'location', script)(
  docShim,
  { addEventListener: () => {} },
  { now: () => clock },
  (_path, opts) => {
    captured.push(JSON.parse(opts.body));
    return Promise.resolve({ json: () => Promise.resolve({ ok: true, token: 'deadbeef' }) });
  },
  { hash: '' },
);

// Complete the PIN gate so the page permits sends.
const pairResult = pinned.go.onclick();
Promise.resolve(pairResult).then(() => { runGestures(); });

function runGestures() {
const fire = (name, ev) => { for (const f of listeners[name] || []) f(ev); };
const down = (id, x, y, t) => { clock = t; fire('pointerdown', { pointerId: id, clientX: x, clientY: y }); };
const move = (id, x, y, t) => { clock = t; fire('pointermove', { pointerId: id, clientX: x, clientY: y }); };
const up = (id, x, y, t) => { clock = t; fire('pointerup', { pointerId: id, clientX: x, clientY: y }); };
const msgs = (arr, kind) => arr.flatMap(b => b.msgs || []).filter(m => m.t === kind);

// --- 1: three-finger quick tap => exactly one click b=2 ---
captured.length = 0;
down(1, 100, 100, 1000);
down(2, 140, 100, 1010);
down(3, 180, 100, 1020);
up(1, 100, 100, 1080);
up(2, 140, 100, 1090);
up(3, 180, 100, 1100);
const c3 = msgs(captured, 'click');
console.log('3-tap:', JSON.stringify(c3));
if (c3.length !== 1 || c3[0].b !== 2) { console.error('FAIL 3-tap'); process.exit(1); }

// --- 2: slow three-finger press => no click emitted ---
captured.length = 0;
down(1, 100, 100, 2000);
down(2, 140, 100, 2010);
down(3, 180, 100, 2020);
up(1, 100, 100, 2400);
up(2, 140, 100, 2410);
up(3, 180, 100, 2420);
const s3 = msgs(captured, 'click');
if (s3.length !== 0) { console.error('FAIL slow 3-tap:', JSON.stringify(s3)); process.exit(1); }

// --- 3: two-finger quick tap => right click b=1 (regression) ---
captured.length = 0;
down(1, 100, 100, 4000);
down(2, 140, 100, 4030);
up(1, 100, 100, 4090);
up(2, 140, 100, 4110);
const c2 = msgs(captured, 'click');
console.log('2-tap:', JSON.stringify(c2));
if (c2.length !== 1 || c2[0].b !== 1) { console.error('FAIL 2-tap'); process.exit(1); }

// --- 4: one-finger quick tap => left click b=0 (regression) ---
captured.length = 0;
down(1, 100, 100, 6000);
up(1, 101, 101, 6080);
const c1 = msgs(captured, 'click');
console.log('1-tap:', JSON.stringify(c1));
if (c1.length !== 1 || c1[0].b !== 0) { console.error('FAIL 1-tap'); process.exit(1); }

// --- 5: one-finger drag still moves (regression) ---
captured.length = 0;
down(1, 100, 100, 8000);
move(1, 130, 100, 8020);
move(1, 160, 100, 8040);
up(1, 160, 100, 8100);
const mv = msgs(captured, 'move');
console.log('drag moves:', mv.length);
if (mv.length === 0) { console.error('FAIL drag'); process.exit(1); }

// --- 6: two-finger drag still scrolls (regression) ---
captured.length = 0;
down(1, 100, 100, 9000);
down(2, 140, 100, 9010);
move(1, 100, 140, 9030);
move(2, 140, 140, 9040);
up(1, 100, 140, 9200);
up(2, 140, 140, 9210);
const sc = msgs(captured, 'scroll');
console.log('scroll msgs:', JSON.stringify(sc));
if (sc.length === 0) { console.error('FAIL scroll'); process.exit(1); }

console.log('ALL WEB GESTURE CHECKS PASS');
}
