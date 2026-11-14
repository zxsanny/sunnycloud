# S12 verified-download kit (AZ-3931) — Safari run on the owner Mac

Spike material, not product code. Results and the Firefox decision are in `_docs/02_tasks/done/AZ-3931_s12_verified_download_spike.md`. The owner chose on 2026-11-14 to measure Safari on the owner Mac (gate S12 option a). Until that run is recorded, E-26/c12 builds the labelled unverified path for Safari.

Files: `server.mjs` (static server; every response carries the exact CSP of 16 §5; `/files/<name>` honours `Range` and an optional `?rate=<MiB/s>`), `filehash.mjs` (the server-side file hash: roll-up of the 2 MiB BLAKE3 block hashes), `www/` (the page, the OPFS worker of candidates A1/A2, the Service Worker of B1/B2).

## Steps on the Mac (Safari 18 or later, Node 22, Rust with `wasm-pack`)

1. Build the WASM package from the repository root:
   `wasm-pack build src/sync-core-wasm --target web --release --out-dir "$PWD/_docs/00_research/s12_download_spike/www/pkg" --out-name sync_core_wasm`
2. Make the test files (outside the repository) and their expected hashes:
   ```
   mkdir -p ~/s12data
   head -c 1073741824 /dev/urandom > ~/s12data/1g.bin
   head -c 2147483648 /dev/urandom > ~/s12data/2g.bin
   node _docs/00_research/s12_download_spike/filehash.mjs _docs/00_research/s12_download_spike/www/pkg ~/s12data/*.bin
   ```
3. Start the server: `node _docs/00_research/s12_download_spike/server.mjs _docs/00_research/s12_download_spike/www ~/s12data 8137`
4. In Safari open, one at a time (replace `<hash>` with the 2 GiB line from step 2):
   - A2: `http://127.0.0.1:8137/?cand=opfs&mode=range&src=/files/2g.bin&h=<hash>&name=2g.bin&run=a2`
   - B2: `http://127.0.0.1:8137/?cand=sw&mode=range&src=/files/2g.bin&h=<hash>&name=2g.bin&run=b2`
   The page starts by itself after 6 s and posts its result to the server; read it with `curl 'http://127.0.0.1:8137/report?run=a2'`.
5. While each run is going, record Safari's memory every few seconds: `while sleep 2; do footprint -p Safari | grep -i "phys_footprint"; done` (or Activity Monitor's Memory column for the Safari web content process); note the peak. Footprint numbers are weak evidence; label them as such.
6. Repeat with `1g.bin` and with one corrupt run per candidate (flip one byte in a copy of `1g.bin`, keep the original hash in the URL): the result must say the hash differs and nothing must be saved under the final name.
7. Record per candidate and size: peak footprint, whether the saved file's hash (step 2 command on the downloaded file) equals the server's, duration, and the corrupt-run outcome. Add them to the Safari row of the AZ-3931 results and to IT-16-8 in `components/16_web_app/tests.md`, then delete `~/s12data`.

Pass rule (16 §5, IT-16-8): peak at most 160 MB, failure above 200 MB, hash equal, corrupt block detected with nothing saved. If Safari's OPFS quota stops A2 below 2 GiB (Playwright WebKit stopped at 896 MiB), record the quota: gate S12 then falls back to option (c), a lower verified limit.
