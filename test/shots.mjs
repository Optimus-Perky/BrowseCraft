// Screenshots of BrowseCraft: the whole (virtual) screen via xwd, so the window frame,
// tabs and icon show. node shots.mjs <chrome> <server> <user> <password> <outdir>
import { spawn, execSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const [chromeBin, server, user, password, out] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const shot = (name) => execSync(`xwd -root -silent | convert xwd:- ${out}/${name}.png`);
function launch(dir, port, url) {
  return spawn(chromeBin, [`--user-data-dir=${dir}`, `--remote-debugging-port=${port}`, '--enable-local-sync-backend',
    '--no-first-run', '--no-default-browser-check', '--password-store=basic', '--disable-gpu', '--no-sandbox',
    '--force-device-scale-factor=1.3', '--window-position=0,0', '--window-size=1477,831', url], { stdio: 'ignore' });
}
async function cdp(port) {
  for (let i = 0; i < 60; i++) {
    try {
      const v = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      const ws = new WebSocket(v.webSocketDebuggerUrl);
      await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
      let id = 0; const waits = new Map();
      ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && waits.has(d.id)) { waits.get(d.id)(d); waits.delete(d.id); } };
      const send = (method, params = {}, sessionId) => new Promise((ok) => { const n = ++id; waits.set(n, ok); ws.send(JSON.stringify({ id: n, method, params, sessionId })); });
      return { ws, send };
    } catch { await sleep(500); }
  }
  throw new Error('no devtools');
}
async function firstPage(c) {
  const t = (await c.send('Target.getTargets')).result.targetInfos.find((x) => x.type === 'page');
  const s = (await c.send('Target.attachToTarget', { targetId: t.targetId, flatten: true })).result.sessionId;
  return {
    go: async (url) => { await c.send('Page.navigate', { url }, s); await sleep(2500); },
    ev: async (e) => (await c.send('Runtime.evaluate', { expression: e, awaitPromise: true, returnByValue: true }, s)).result?.result?.value,
  };
}
const dir = mkdtempSync(join(tmpdir(), 'bc-shot-'));
let p = launch(dir, 9401, 'about:blank');
let c = await cdp(9401); await sleep(3000);
let pg = await firstPage(c);
await pg.go('chrome://browsecraft');
await pg.ev(`document.getElementById('server').value=${JSON.stringify(server)};document.getElementById('user').value=${JSON.stringify(user)};true`);
shot('1-sign-in');
await pg.ev(`document.getElementById('password').value=${JSON.stringify(password)};document.getElementById('form').requestSubmit();true`);
await sleep(3000); shot('2-signed-in');
c.ws.close(); p.kill(); await sleep(3000);
p = launch(dir, 9401, 'about:blank');
c = await cdp(9401); await sleep(3000);
pg = await firstPage(c);
await pg.go('chrome://settings/help'); await sleep(3000);
shot('3-about');
await pg.go('chrome://bookmarks'); await sleep(12000);
shot('4-synced-bookmarks');
await pg.go('chrome://sync-internals'); await sleep(2500);
shot('5-sync-internals');
p.kill();
process.exit(0);
