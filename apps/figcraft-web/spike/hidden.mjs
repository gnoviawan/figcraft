// Bridge latency while the embedding page is really hidden (document.hidden), which Playwright's
// own emulation never lets happen: launches Edge directly and drives it over raw CDP.
//   node apps/figcraft-web/spike/serve.mjs &
//   EDGE=<path to msedge.exe> PROFILE=<scratch dir> node apps/figcraft-web/spike/hidden.mjs
import { spawn } from 'node:child_process';

const edge = process.env.EDGE || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const port = 9333;
const proc = spawn(edge, [
  `--remote-debugging-port=${port}`, `--user-data-dir=${process.env.PROFILE}`, '--no-first-run', '--window-size=1400,900',
  // A window covered by others counts as hidden on Windows; only minimising should hide it here.
  '--disable-backgrounding-occluded-windows', 'http://localhost:8768/host.html'], { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function json(path) {
  for (let i = 0; i < 100; i++) {
    try { return await (await fetch(`http://127.0.0.1:${port}${path}`)).json(); } catch { await sleep(200); }
  }
  throw new Error(`no CDP on ${port}`);
}
function connect(url) {
  const ws = new WebSocket(url);
  let next = 1;
  const waiting = new Map();
  ws.onmessage = (e) => { const m = JSON.parse(e.data); if (m.id && waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); } };
  const opened = new Promise((r) => { ws.onopen = r; });
  return {
    opened,
    send: (method, params = {}) => new Promise((resolve) => { const id = next++; waiting.set(id, resolve); ws.send(JSON.stringify({ id, method, params })); }),
    close: () => ws.close(),
  };
}

const version = await json('/json/version');
let target;
for (let i = 0; i < 100 && !target; i++) {
  target = (await json('/json/list')).find((t) => t.type === 'page' && t.url.includes('host.html'));
  if (!target) await sleep(200);
}
const browserWs = connect(version.webSocketDebuggerUrl);
const pageWs = connect(target.webSocketDebuggerUrl);
await Promise.all([browserWs.opened, pageWs.opened]);
const evaluate = async (expression) => {
  const r = await pageWs.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.error || r.result?.exceptionDetails) throw new Error(JSON.stringify(r.error || r.result.exceptionDetails));
  return r.result.result.value;
};

while ((await evaluate('typeof bridge')) !== 'object') await sleep(200);
await evaluate('bridge.ready()');
const { windowId } = (await browserWs.send('Browser.getWindowForTarget', { targetId: target.id })).result;
const setState = (windowState) => browserWs.send('Browser.setWindowBounds', { windowId, bounds: { windowState } });

const run = (mode) => evaluate(`(async () => {
  bridge.frame.className = ${JSON.stringify(mode)};
  await new Promise((r) => setTimeout(r, 1500));
  const s = { visibility: document.visibilityState, inspect: [], execute: [], ui: [] };
  for (let i = 0; i < 30; i++) {
    const a = await bridge.call('document.inspect', '{"depth":1}');
    if (a.ok) s.inspect.push(a.ms);
    const b = await bridge.call('engine.execute', '{"command":"select.none"}');
    if (b.ok) s.execute.push(b.ms);
  }
  for (let i = 0; i < 3; i++) {
    const c = await bridge.call('ui.inspect', '{}', 3000);
    s.ui.push(c.ok ? c.ms : c.error.code);
  }
  bridge.frame.className = '';
  return s;
})()`);
const summary = (xs) => { const s = [...xs].sort((a, b) => a - b); return s.length ? { n: s.length, p50: +s[Math.floor(s.length / 2)].toFixed(2), max: +s[s.length - 1].toFixed(2) } : null; };
const out = {};
for (const [label, mode] of [['visible', ''], ['minimised', ''], ['minimised + offscreen 1x1', 'offscreen'], ['minimised + display:none', 'none']]) {
  await setState(label.startsWith('minimised') ? 'minimized' : 'normal');
  await sleep(1000);
  const r = await run(mode);
  out[label] = { visibility: r.visibility, documentInspect: summary(r.inspect), execute: summary(r.execute), ui: r.ui };
}
await setState('normal');
console.log(JSON.stringify(out, null, 2));
browserWs.close();
pageWs.close();
proc.kill();
