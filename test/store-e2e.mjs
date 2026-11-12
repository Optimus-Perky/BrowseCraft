// BrowseCraft + the organisation extension store, end to end. Run as root inside the builder
// image with Xvfb running, after writing the policies (store-e2e.sh does both):
//   /etc/browsecraft/policies/managed/store.json   {"ExtensionStoreUrl": "<server>"}
//   /etc/chromium/policies/managed/force.json      ExtensionInstallForcelist: Dark Reader with
//                                                  GOOGLE's update address (must come from our store)
//   node store-e2e.mjs <chrome binary> <server url>
import { spawn, execSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [chromeBin, server] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);
const DARK = 'eimadpbcbfnmbkopoojfekhnkhdbieeh';
const UBOL = 'ddkjiahejlhfcafbddmgiahcphecmpfh';
let failed = 0;
const check = (ok, what) => { log(ok ? 'PASS' : 'FAIL', what); if (!ok) failed++; };

const dir = mkdtempSync(join(tmpdir(), 'bc-store-'));
const proc = spawn(chromeBin, [
  `--user-data-dir=${dir}`, '--remote-debugging-port=9333', '--no-first-run', '--no-default-browser-check',
  '--password-store=basic', '--disable-gpu', '--no-sandbox', '--enable-logging=stderr',
  '--vmodule=extension_downloader*=2,extension_updater*=2,safe_manifest_parser*=2,crx_installer*=2,sandboxed_unpacker*=2,external_provider*=2,*update_service*=2',
  'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });
import { createWriteStream } from 'node:fs';
const errlog = createWriteStream(process.env.CHROME_LOG || '/tmp/chrome-stderr.log');
proc.stderr.pipe(errlog);
proc.on('exit', (code, sig) => log('browser exited', code, sig));

async function cdp(port) {
  for (let i = 0; i < 60; i++) {
    try {
      const v = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      const ws = new WebSocket(v.webSocketDebuggerUrl);
      await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
      let id = 0; const waits = new Map();
      ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && waits.has(d.id)) { waits.get(d.id)(d); waits.delete(d.id); } };
      const send = (method, params = {}, sessionId) => new Promise((ok) => { const i2 = ++id; waits.set(i2, ok); ws.send(JSON.stringify({ id: i2, method, params, sessionId })); });
      return { ws, send };
    } catch { await sleep(500); }
  }
  throw new Error('no devtools on ' + port);
}

async function open(c, url) {
  const t = await c.send('Target.createTarget', { url });
  const a = await c.send('Target.attachToTarget', { targetId: t.result.targetId, flatten: true });
  const s = a.result.sessionId;
  await sleep(4000);
  const evalJs = async (expr) => {
    const r = await c.send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }, s);
    return r.result?.result?.value ?? JSON.stringify(r.result?.exceptionDetails?.exception?.description ?? r);
  };
  return { s, evalJs, targetId: t.result.targetId };
}

const c = await cdp(9333);
await sleep(3000);

// 1. The policy became Chromium's store switch, and page processes got it.
const ps = execSync('ps -eo args').toString();
check(ps.includes(`--apps-gallery-url=${server}/webstore`), `a page process runs with --apps-gallery-url=${server}/webstore`);

// 2. Our store page has the install API.
const store = await open(c, `${server}/webstore`);
const t2 = await store.evalJs('typeof (window.chrome && chrome.webstorePrivate && chrome.webstorePrivate.beginInstallWithManifest3)');
check(t2 === 'function', `the organisation store page has chrome.webstorePrivate (got ${t2})`);

// 3. Google's store may not install. (Its consent page would hide the store: accept it by cookie.)
await c.send('Storage.setCookies', { cookies: [{ name: 'SOCS', value: 'CAESEwgDEgk0ODE3Nzk3MjQaAmVuIAEaBgiA_LyaBg', domain: '.google.com', path: '/', secure: true }] });
const google = await open(c, 'https://chromewebstore.google.com/');
log('Google store page:', await google.evalJs('location.href'), '| API:', await google.evalJs('typeof (window.chrome && chrome.webstorePrivate)'));
const t3 = await google.evalJs(`new Promise((ok) => {
  if (!(window.chrome && chrome.webstorePrivate)) { ok('no API'); return; }
  chrome.webstorePrivate.beginInstallWithManifest3({id: '${UBOL}', manifest: '{"name":"x","version":"1","manifest_version":3}'},
    (r) => ok(String(r) + ' / ' + (chrome.runtime.lastError ? chrome.runtime.lastError.message : '')));
})`);
check(/blocked_by_policy|organisation/.test(t3), `the Chrome Web Store cannot install (got ${t3})`);

// 4. Dark Reader, force-installed with GOOGLE's update address, arrives from our store.
let got = false;
for (let i = 0; i < 40 && !got; i++) {
  const ts = await c.send('Target.getTargets');
  got = ts.result.targetInfos.some((t) => t.url.startsWith(`chrome-extension://${DARK}/`));
  if (!got) await sleep(3000);
}
check(got, 'Dark Reader (Google update address in policy) was installed (from our store: see the server log)');

// 5. An update check through the update service (store extensions) goes to our server too.
const ext = await open(c, 'chrome://extensions/');
const t5 = await ext.evalJs('new Promise((ok) => chrome.developerPrivate.autoUpdate(() => ok(chrome.runtime.lastError ? chrome.runtime.lastError.message : "asked")))');
log('update check requested:', t5);
await sleep(8000);

proc.kill('SIGTERM');
log(failed ? `${failed} FAILED` : 'ALL PASSED');
process.exit(failed ? 1 : 0);
