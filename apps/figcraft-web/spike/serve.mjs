// Serves the built bundle (dist/web) on 127.0.0.1:8767 and this harness on localhost:8768: two
// origins, as in a real embed, so the origin lock is exercised.
//   node apps/figcraft-web/spike/serve.mjs [bundle dir]
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = fileURLToPath(new URL('.', import.meta.url));
const bundleDir = resolve(process.argv[2] || join(here, '../../../dist/web'));
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.png': 'image/png', '.json': 'application/json' };

function serve(root, host, port) {
  createServer(async (req, res) => {
    const path = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^([/\\])+/, '');
    const file = join(root, path || 'index.html');
    if (!file.startsWith(root)) { res.writeHead(403).end(); return; }
    try {
      const body = await readFile(file.endsWith('\\') || file.endsWith('/') ? join(file, 'index.html') : file);
      res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream', 'cache-control': 'no-store' }).end(body);
    } catch {
      res.writeHead(404).end();
    }
  }).listen(port, host, () => console.log(`serving ${root} on http://${host}:${port}`));
}

serve(bundleDir, '127.0.0.1', 8767);
serve(here, 'localhost', 8768);
