---
title: Open the acceptance stack by a LAN address only over https; localhost is the plain-http origin
applies_when: running the Playwright browser tests, or ACC_HOST in scripts/dev/acceptance-up.sh
source: 2026-11-23, browser tests on 192.168.88.100
---

Chromium keeps a `Secure` or `__Host-` cookie only from https or localhost. On `http://192.168.88.100` the sign-in binding cookie is dropped and the callback answers `/login?error=invalid_request`, even with `--unsafely-treat-insecure-origin-as-secure` (the flag also needs the full `chromium` channel, the default headless shell ignores it, and it still does not change cookie storage). Use `ACC_HOST=192.168.88.100 scripts/dev/acceptance-up.sh`: the web server then serves https with a self-signed certificate and Playwright sets `ignoreHTTPSErrors`. `acceptance-down.sh` deletes the recorded host, so name `ACC_HOST` on every `up`.
