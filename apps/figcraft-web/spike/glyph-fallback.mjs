// Glyph fallback in the raster path (gnoviawan/termul#1421). Renders the issue's text with
// host.renderPng, once per character too, and counts the characters that come out as the
// NOGLYPH box. With FONTS set it first hands the editor those font files over
// host.fallbackFonts, served next to the bundle the way Termul's loopback listener serves them.
//   BUNDLE=<dist/web> FONTS=seguisym.ttf,seguiemj.ttf,msyh.ttc,malgun.ttf OUT=after \
//   PW_DIR=<dir with node_modules/playwright-core> node apps/figcraft-web/spike/glyph-fallback.mjs
// FONT_DIR defaults to C:/Windows/Fonts; PORT (default 8791) and PORT+1 are used.
import { createServer } from 'node:http';
import { readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { basename, extname, join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(join(process.env.PW_DIR || '.', 'x.js'));
const { chromium } = require('playwright-core');
const here = fileURLToPath(new URL('.', import.meta.url));
const bundleDir = resolve(process.env.BUNDLE || join(here, '../../../dist/web'));
const fontDir = resolve(process.env.FONT_DIR || 'C:/Windows/Fonts');
const fonts = (process.env.FONTS || '').split(',').map((f) => f.trim()).filter(Boolean);
const out = process.env.OUT || 'glyph-fallback';
const port = Number(process.env.PORT || 8791);
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm' };

function serve(root, host, p, extra) {
  return new Promise((ready) => {
    const server = createServer(async (req, res) => {
      const path = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^([/\\])+/, '');
      const file = extra?.(path) ?? join(root, path || 'index.html');
      try {
        const body = await readFile(file.endsWith('\\') || file.endsWith('/') ? join(file, 'index.html') : file);
        res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream', 'cache-control': 'no-store' }).end(body);
      } catch {
        res.writeHead(404).end();
      }
    });
    server.listen(p, host, () => ready(server));
  });
}

const hostFont = (path) => {
  const m = /^host-fonts[/\\](.+)$/.exec(path);
  return m && fonts.includes(basename(m[1])) ? join(fontDir, basename(m[1])) : undefined;
};
const servers = [await serve(bundleDir, '127.0.0.1', port, hostFont), await serve(here, 'localhost', port + 1)];

const TEXT = 'Inter: ❯ ✎ ◐ ▾ ✦ █ ◎ 中文 日本語 😀 → ✓';
// Kana and Hangul are not in the issue's text: checked too, to say which CJK scripts are covered.
const EXTRA = 'かカ한';
// U+0378 is unassigned: no font draws it, so its render is the NOGLYPH box.
const NOGLYPH = '\u0378';

const browser = await chromium.launch({ channel: process.env.CHANNEL || 'msedge', headless: !process.env.HEADED, args: ['--enable-unsafe-webgpu'] });
const page = await (await browser.newContext({ viewport: { width: 1600, height: 900 } })).newPage();
await page.goto(`http://localhost:${port + 1}/host.html?kiosk&bundle=${encodeURIComponent(`http://127.0.0.1:${port}`)}`);
await page.evaluate(() => bridge.ready());
const raw = async (method, params = {}, timeoutMs = 10000) => {
  const r = await page.evaluate(([m, p, t]) => bridge.call(m, p, t), [method, JSON.stringify(params), timeoutMs]);
  if (!r.ok) throw new Error(`${method}: ${JSON.stringify(r.error)}`);
  return { result: r.result, ms: r.ms };
};
const exec = async (command, params = {}) => (await raw('engine.execute', { command, params })).result;
const create = async (text, y, fontSize) =>
  (await exec('text.create', { text, x: 40, y, props: { fontFamily: 'Inter', fontSize, fill: '#000000' } })).match(/"id"\s*:\s*"?(\d+)/)[1];
const png = async (id) => Buffer.from(JSON.parse((await raw('host.renderPng', { ids: [id], scale: 1 })).result), 'base64');

if (fonts.length) {
  const loaded = await raw('host.fallbackFonts', { urls: fonts.map((f) => `host-fonts/${f}`) }, 60000);
  console.log(`host.fallbackFonts ${fonts.join(', ')}: ${loaded.result} in ${loaded.ms.toFixed(0)} ms`);
}

const box = await png(await create(NOGLYPH, 400, 28));
const chars = [...new Set([...TEXT.slice('Inter: '.length), ...EXTRA].filter((c) => c.trim()))];
const boxed = [];
for (const c of chars) {
  const id = await create(c, 400, 28);
  if ((await png(id)).equals(box)) boxed.push(c);
  await exec('edit.delete', { ids: [id] });
}
const line = await create(TEXT, 60, 28);
await exec('select.none');
await writeFile(join(here, `${out}.png`), await png(line));
await new Promise((r) => setTimeout(r, 600));
await page.locator('#frame').screenshot({ path: join(here, `${out}-canvas.png`) });
console.log(`NOGLYPH boxes: ${boxed.length}/${chars.length} ${boxed.join(' ')}`);
console.log(`drawn:         ${chars.length - boxed.length}/${chars.length} ${chars.filter((c) => !boxed.includes(c)).join(' ')}`);
await browser.close();
for (const s of servers) s.close();
