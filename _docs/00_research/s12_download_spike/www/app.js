// Page side of the S12 spike. Query: cand=opfs|sw|ab, src=/files/<name>, h=<expected hex>, name=<save name>.
const q = new URLSearchParams(location.search);
const mode = q.get("mode") ?? "stream";
const cand = q.get("cand"), src = q.get("src"), expected = q.get("h"), name = q.get("name") ?? "download.bin";
const violations = [];
document.addEventListener("securitypolicyviolation", (e) => violations.push(`page: ${e.violatedDirective} ${e.blockedURI}`));
const status = (t) => { document.getElementById("status").textContent = t; };
const s12 = (window.s12 = { ready: false, result: null, violations, events: [] });
const opfsName = `dl-${Date.now()}.part`;
let worker, objectUrl;

function save(href) {
  const a = document.createElement("a");
  a.href = href; a.download = name; document.body.append(a); a.click(); a.remove();
}

async function opfsList() {
  const root = await navigator.storage.getDirectory(); const out = [];
  for await (const [n, h] of root.entries()) out.push(h.kind === "file" ? `${n}:${(await h.getFile()).size}` : `${n}/`);
  return out;
}
s12.opfsList = opfsList;
s12.estimate = async () => { const e = await navigator.storage.estimate(); return { usage: e.usage, quota: e.quota }; };

if (cand === "opfs" || cand === "ab") {
  worker = new Worker(cand === "opfs" ? "/worker-opfs.js" : "/worker-ab.js", { type: "module" });
  worker.onerror = (e) => { s12.result = { ok: false, reason: `worker error: ${e.message}` }; };
  worker.onmessage = async ({ data }) => {
    violations.push(...(data.violations ?? []).map((v) => `worker: ${v}`));
    if (cand === "opfs" && data.ok) {
      // Only now does the file get a user-visible name: the OPFS File is disk-backed, the blob: URL refers to it.
      const root = await navigator.storage.getDirectory();
      const file = await (await root.getFileHandle(opfsName)).getFile();
      objectUrl = URL.createObjectURL(file);
      save(objectUrl);
      data.savedAt = performance.now();
    }
    s12.result = data; status(JSON.stringify(data));
  };
  s12.start = () => { s12.t0 = performance.now(); worker.postMessage({ src, expected, opfsName, mode }); };
  s12.cleanup = async () => {
    if (objectUrl) URL.revokeObjectURL(objectUrl);
    const root = await navigator.storage.getDirectory();
    try { await root.removeEntry(opfsName); } catch (e) { if (e.name !== "NotFoundError") throw e; }
  };
  s12.ready = true; status("ready");
} else if (cand === "sw") {
  const bc = new BroadcastChannel("s12");
  bc.onmessage = ({ data }) => {
    if (data.violation) { violations.push(data.violation); return; }
    if (data.pong) { s12.events.push("pong"); return; }
    s12.result = data; status(JSON.stringify(data));
  };
  navigator.serviceWorker.register("/sw.js", { type: "module", scope: "/" })
    .then(() => navigator.serviceWorker.ready)
    .then(() => { s12.ready = true; status("ready"); })
    .catch((e) => { s12.result = { ok: false, reason: `register: ${e.name}: ${e.message}` }; s12.ready = true; });
  s12.start = (trigger = "iframe") => {
    s12.t0 = performance.now();
    const url = `/dl?id=1&mode=${mode}&src=${encodeURIComponent(src)}&h=${expected}&name=${encodeURIComponent(name)}${q.get("size") ? `&size=${q.get("size")}` : ""}`;
    if (trigger === "iframe") { const f = document.createElement("iframe"); f.hidden = true; f.src = url; document.body.append(f); }
    else if (trigger === "anchor") save(url);
    else location.assign(url);
  };
  s12.cleanup = async () => {};
}

// Un-automated mode (Firefox launched without Playwright): start by itself and report to the server.
const run = q.get("run");
if (run) {
  const send = (phase) => fetch("/report", { method: "POST", body: JSON.stringify({ run, phase, result: s12.result, violations }) });
  const tick = () => {
    if (s12.result) { s12.result.reportedAt = performance.now() - s12.t0; send("result"); setTimeout(() => send("final"), 5000); return; }
    setTimeout(tick, 100);
  };
  const wait = () => {
    if (!s12.ready) { setTimeout(wait, 100); return; }
    if (s12.result) { send("setup"); return; }
    send("ready");
    setTimeout(() => { s12.start(q.get("trigger") ?? "iframe"); tick(); }, Number(q.get("delay") ?? 6000));
  };
  wait();
}
