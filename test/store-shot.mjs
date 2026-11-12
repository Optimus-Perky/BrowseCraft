// Screenshot of the store search (for checking): node store-shot.mjs <chrome> <server> <user> <pwfile> <query> <out.png>
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const [chromeBin, server, user, pwFile, query, out] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const proc = spawn(chromeBin, [`--user-data-dir=${mkdtempSync(join(tmpdir(), 'bc-shot-'))}`, '--remote-debugging-port=9337', '--no-first-run',
  '--no-default-browser-check', '--password-store=basic', '--disable-gpu', '--no-sandbox', 'about:blank'], { stdio: 'ignore' });
let c;
for (let i = 0; i < 60 && !c; i++) {
  try {
    const v = await (await fetch('http://127.0.0.1:9337/json/version')).json();
    const ws = new WebSocket(v.webSocketDebuggerUrl);
    await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
    let id = 0; const waits = new Map();
    ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && waits.has(d.id)) { waits.get(d.id)(d); waits.delete(d.id); } };
    c = { send: (method, params = {}, sessionId) => new Promise((ok) => { const i2 = ++id; waits.set(i2, ok); ws.send(JSON.stringify({ id: i2, method, params, sessionId })); }) };
  } catch { await sleep(500); }
}
const t = await c.send('Target.createTarget', { url: `${server}/webstore/` });
const s = (await c.send('Target.attachToTarget', { targetId: t.result.targetId, flatten: true })).result.sessionId;
await sleep(2500);
await c.send('Runtime.evaluate', { expression: `fetch('/webstore/sign-in', {method: 'POST', body: new URLSearchParams({user: ${JSON.stringify(user)}, password: ${JSON.stringify(readFileSync(pwFile, 'utf8').trim())}})})`, awaitPromise: true }, s);
await c.send('Emulation.setDeviceMetricsOverride', { width: 1477, height: 1500, deviceScaleFactor: 1.3, mobile: false }, s);
await c.send('Page.navigate', { url: `${server}/webstore?q=${encodeURIComponent(query)}` }, s);
await sleep(8000);
const shot = await c.send('Page.captureScreenshot', { format: 'png' }, s);
writeFileSync(out, Buffer.from(shot.result.data, 'base64'));
console.log('saved', out);
proc.kill('SIGKILL'); process.exit(0);
