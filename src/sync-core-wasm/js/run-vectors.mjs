// WASM leg of the vector gate (AZ-3969): runs every case of the shared vector file through the
// wasm-bindgen exports and returns the differences, each naming its case. No Node or DOM API is used, so
// the same code runs in Node and in a browser Worker. Expectations come only from the file (native).

const MIB = 1024 * 1024;

const hex = (bytes) => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");

function unhex(text) {
  const out = new Uint8Array(text.length / 2);
  for (let at = 0; at < out.length; at += 1) {
    out[at] = parseInt(text.slice(at * 2, at * 2 + 2), 16);
  }
  return out;
}

const list = (text) => (text === "-" ? [] : text.split(",").map(Number));

/** Same grammar as tests/common/vector_file.rs; a malformed line throws naming it. */
export function parseVectors(text) {
  const cases = [];
  for (const line of text.split("\n")) {
    if (line.trim() === "" || line.startsWith("#")) continue;
    const f = line.split(" ");
    const shape = `${f[0]}/${f.length}`;
    if (shape === "file/5") {
      cases.push({ kind: "file", name: f[1], length: Number(f[2]), fileHash: f[3], blocks: f[4] === "-" ? [] : f[4].split(",") });
    } else if (shape === "oversize/3") {
      cases.push({ kind: "oversize", name: f[1], length: Number(f[2]) });
    } else if (shape === "client_key/6") {
      cases.push({ kind: "client_key", name: f[1], device: f[2], asset: f[3], fileHash: f[4], key: f[5] });
    } else if (shape === "commit_id/5") {
      cases.push({ kind: "commit_id", name: f[1], unixMs: Number(f[2]), random: f[3], id: f[4] });
    } else if (shape === "planner/10") {
      cases.push({
        kind: "planner", name: f[1], blockCount: Number(f[2]), need: list(f[3]), lanesMax: Number(f[4]),
        retryBaseMs: Number(f[5]), retryMaxMs: Number(f[6]), maxAttempts: Number(f[7]),
        events: f[8] === "-" ? [] : f[8].split(";"), expected: f[9],
      });
    } else {
      throw new Error(`malformed vector line: ${line}`);
    }
  }
  return cases;
}

/** Byte i of a generated file is (i mod 251); fills `buf` with the bytes at file offset `offset`. */
export function fillTestBytes(buf, offset) {
  let value = offset % 251;
  for (let j = 0; j < buf.length; j += 1) {
    buf[j] = value;
    value = value === 250 ? 0 : value + 1;
  }
}

function throwsNamed(fn, name) {
  try {
    fn();
  } catch (error) {
    return error instanceof Error && error.name === name ? null : `threw ${error?.name}: ${error?.message}`;
  }
  return "did not throw";
}

function runFile(wasm, c, block, diffs) {
  const blockSize = wasm.block_size();
  const hashes = new Uint8Array(Math.ceil(c.length / blockSize) * 32);
  let k = 0;
  for (let offset = 0; offset < c.length; offset += blockSize, k += 1) {
    const view = block.subarray(0, Math.min(blockSize, c.length - offset));
    fillTestBytes(view, offset);
    const got = wasm.hash_block(view);
    hashes.set(got, k * 32);
    if (hex(got) !== c.blocks[k]) diffs.push(`file ${c.name}: block ${k} hash expected ${c.blocks[k]}, got ${hex(got)}`);
  }
  if (k !== c.blocks.length) diffs.push(`file ${c.name}: ${c.blocks.length} blocks expected, ${k} computed`);
  const file = hex(wasm.rollup(hashes));
  if (file !== c.fileHash) diffs.push(`file ${c.name}: file hash expected ${c.fileHash}, got ${file}`);
}

const actionText = (a) => (a.kind === "SendBlock" || a.kind === "GiveUp" ? `${a.kind}:${a.i}` : a.kind === "Wait" ? `Wait:${a.ms}` : a.kind);

function nextList(planner) {
  const actions = planner.next_actions();
  const text = actions.length === 0 ? "-" : actions.map(actionText).join(",");
  for (const action of actions) action.free();
  return text;
}

function applyEvent(planner, event) {
  const f = event.split(":");
  const ms = (text) => (text === "-" ? undefined : Number(text));
  switch (f[0]) {
    case "Acked": case "HashRejected": case "SessionGone":
      planner.on_result(Number(f[1]), { kind: f[0] });
      break;
    case "Transient": case "BlockConflict":
      planner.on_result(Number(f[1]), { kind: f[0], retry_after_ms: ms(f[2]) });
      break;
    case "Session":
      planner.on_session(new Uint32Array(list(f[1])));
      break;
    case "Commit":
      planner.on_commit(f[1] === "NeedBlocks" ? { kind: f[1], list: list(f[2]) } : { kind: f[1] });
      break;
    default:
      throw new Error(`unknown planner event ${event}`);
  }
}

function runPlanner(wasm, c) {
  const planner = new wasm.UploadPlanner(c.blockCount, new Uint32Array(c.need), c.lanesMax, c.retryBaseMs, c.retryMaxMs, c.maxAttempts);
  try {
    const lists = [nextList(planner)];
    for (const event of c.events) {
      applyEvent(planner, event);
      lists.push(nextList(planner));
    }
    return lists.join("/");
  } finally {
    planner.free();
  }
}

function runOne(wasm, c, block, diffs) {
  if (c.kind === "file") {
    runFile(wasm, c, block, diffs);
  } else if (c.kind === "oversize") {
    const failure = throwsNamed(() => wasm.hash_block(new Uint8Array(c.length)), "BlockTooLarge");
    if (failure) diffs.push(`oversize ${c.name}: ${failure}`);
  } else if (c.kind === "client_key") {
    const got = wasm.client_key_for_mobile(c.device, c.asset, unhex(c.fileHash));
    if (got !== c.key) diffs.push(`client_key ${c.name}: expected ${c.key}, got ${got}`);
  } else if (c.kind === "commit_id") {
    const got = wasm.new_commit_id(c.unixMs, unhex(c.random));
    if (got !== c.id) diffs.push(`commit_id ${c.name}: expected ${c.id}, got ${got}`);
  } else {
    const got = runPlanner(wasm, c);
    if (got !== c.expected) diffs.push(`planner ${c.name}: expected ${c.expected}, got ${got}`);
  }
}

/**
 * Runs every case. `wasm` is the module namespace (hash_block, rollup, ...), `memory` its
 * WebAssembly.Memory. Oversize cases run first and files in ascending size, so the AC-4 memory reading
 * after the 6 MiB file and after the 100 MiB file sees only block-by-block hashing in between.
 */
export function runVectors(wasm, memory, text) {
  const cases = parseVectors(text);
  const order = { oversize: 0, file: 1, client_key: 2, commit_id: 3, planner: 4 };
  cases.sort((a, b) => order[a.kind] - order[b.kind] || (a.kind === "file" ? a.length - b.length : 0));
  const diffs = [];
  const memoryAfter = {};
  const block = new Uint8Array(wasm.block_size());
  for (const c of cases) {
    try {
      runOne(wasm, c, block, diffs);
    } catch (error) {
      diffs.push(`${c.kind} ${c.name}: threw ${error?.name}: ${error?.message}`);
    }
    if (c.kind === "file") memoryAfter[c.length] = memory.buffer.byteLength;
  }
  diffs.push(...fixedChecks(wasm));
  const memoryCheck = checkMemory(memoryAfter, wasm.block_size());
  if (memoryCheck.failure) diffs.push(memoryCheck.failure);
  return { cases: cases.length, diffs, memory: memoryCheck };
}

/** AC-4: memory after the 100 MiB file exceeds that after the 6 MiB file by at most one block. */
function checkMemory(memoryAfter, blockSize) {
  const six = memoryAfter[6 * MIB];
  const hundred = memoryAfter[100 * MIB];
  if (six === undefined || hundred === undefined) {
    return { failure: "memory: the 6 MiB or the 100 MiB file case is missing" };
  }
  const growth = hundred - six;
  const failure = growth > blockSize ? `memory: grew ${growth} bytes from the 6 MiB to the 100 MiB case (limit ${blockSize})` : null;
  return { afterSixMib: six, afterHundredMib: hundred, growth, failure };
}

/** Error names the Worker relies on, beyond the vector file. */
function fixedChecks(wasm) {
  const checks = [
    ["rollup of 33 bytes", () => wasm.rollup(new Uint8Array(33)), "InvalidLength"],
    ["commit id with 9 random bytes", () => wasm.new_commit_id(0, new Uint8Array(9)), "InvalidLength"],
    ["commit id at a fractional time", () => wasm.new_commit_id(0.5, new Uint8Array(10)), "InvalidTime"],
    ["client key of a non-UUID device", () => wasm.client_key_for_mobile("x", "a", new Uint8Array(32)), "InvalidUuid"],
    ["planner result of unknown kind", () => {
      const planner = new wasm.UploadPlanner(1, new Uint32Array([0]), 4, 500, 30000, 5);
      try { planner.on_result(0, { kind: "Lost" }); } finally { planner.free(); }
    }, "InvalidResult"],
  ];
  return checks
    .map(([name, fn, error]) => [name, throwsNamed(fn, error), error])
    .filter(([, failure]) => failure)
    .map(([name, failure, error]) => `error ${name}: expected ${error}, ${failure}`);
}
