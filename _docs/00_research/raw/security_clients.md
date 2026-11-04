# Research: Auth, E2EE vault, desktop tray/settings UI (sunnycloud)

Date: 2026-11-02. Method: web search plus page fetches. Many search summaries were secondary; claims resting only on them are marked [limited source]. Dates "n/d" = not shown on the fetched page (all fetched 2026-11-02).

## Sources

| # | URL | Title | Tier | Date | Summary |
|---|-----|-------|------|------|---------|
| 1 | https://datatracker.ietf.org/doc/html/rfc8252 | RFC 8252 OAuth for Native Apps | L1 | 2017 | External browser only; PKCE; loopback 127.0.0.1 any port, private-use scheme (reverse-DNS), claimed https |
| 2 | https://datatracker.ietf.org/doc/html/rfc8628 | RFC 8628 Device Authorization Grant | L1 | 2019 | Device/user code, polling, slow_down, user-code entropy (8 chars base-20 ~34.5 bit), rate-limit and phishing notes |
| 3 | https://datatracker.ietf.org/doc/html/rfc9700 | RFC 9700 OAuth 2.0 Security BCP | L1 | 2025 | PKCE for public clients, refresh rotation or sender-constraint, exact redirect match, iss/mix-up |
| 4 | https://datatracker.ietf.org/doc/html/rfc9106 | RFC 9106 Argon2 | L1 | 2021 | Recommended: t=1,p=4,m=2 GiB; else t=3,p=4,m=64 MiB; 128-bit salt |
| 5 | https://developers.google.com/identity/protocols/oauth2/limited-input-device | Google OAuth for TVs/limited-input devices | L1 | n/d | Device flow allows only openid/email/profile, drive.appdata/file, youtube; needs "TVs and Limited Input" client type; client_secret sent |
| 6 | https://developers.facebook.com/docs/facebook-login/for-devices | Facebook Login for Devices | L1 | n/d | Device flow exists; enable "Login from Devices"; code expires 420 s; token up to 60 days |
| 7 | https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps | GitHub: authorizing OAuth apps | L1 | n/d | Device flow must be enabled per app; no client_secret; 15 min code; slow_down; PKCE S256; opt-in expiring tokens |
| 8 | https://developer.apple.com/app-store/review/guidelines/ | App Store Review Guidelines (4.8) | L1 | n/d | Apps using third-party/social login for primary account must offer an equivalent privacy-preserving login option |
| 9 | https://developer.apple.com/help/account/configure-app-capabilities/configure-sign-in-with-apple-for-the-web | Configure Sign in with Apple for the web | L1 | n/d | Needs Services ID tied to a primary App ID; domains/return URLs (10 individual, 100 org); no verification file upload needed |
| 10 | https://developer.apple.com/help/app-review/guideline-reference/5-1-1-account-deletion | Guideline 5.1.1(v) account deletion | L1 | n/d | In-app account deletion required; SIWA apps should revoke tokens via REST API |
| 11 | https://better-auth.com/docs/authentication/apple ; https://docs.propelauth.com/sso/social-login/apple ; https://docs.rs/crate/fraiseql-auth/2.15.0/source/src/providers/apple.rs | Third-party Apple integration docs | L3 | n/d | Client secret = ES256 JWT, exp <= iat+15,777,000 s (6 months), sub=Services ID, aud=appleid.apple.com, .p8 downloadable once; private-relay emails (Apple's own page not retrievable) |
| 12 | https://v1.arcticjs.dev/providers/apple ; https://hexdocs.pm/ueberauth_apple/readme.html | Arctic / Ueberauth Apple docs | L3 | n/d | With name/email scope Apple uses form_post; name only on first auth; state cookie needs SameSite=None |
| 13 | https://developers.facebook.com/documentation/resp-plat-initiatives/individual-processes/data-use-checkup/tutorial ; https://developers.facebook.com/docs/permissions/ | Meta Data Use Checkup; Permissions | L1 | n/d | Apps with live use case or Advanced Access must complete data-handling checkup; Business Verification required for Advanced Access; public_profile auto-granted |
| 14 | https://developers.facebook.com/blog/post/2021/06/08/introducing-facebook-graph-api-v11-marketing-api-v11/ ; https://developers.secure.facebook.com/blog/post/2018/05/01/facebook-login-updates-further-protect-privacy/ | Meta blog posts | L2 | 2018/2021 | New consumer apps must be Live; privacy policy URL + data deletion callback/URL required |
| 15 | https://support.google.com/cloud/answer/10311615 ; https://developers.google.com/health/setup | Google OAuth consent screen / setup | L1 | n/d | Testing status: refresh tokens expire after 7 days; unverified clients capped at 100 users; brand verification |
| 16 | https://dexidp.io/docs/connectors/ | Dex connectors | L1 | n/d | GitHub stable; OIDC beta; Google alpha; no Facebook, no Apple connector |
| 17 | https://docs.goauthentik.io/users-sources/sources/social-logins/ | authentik social login sources | L1 | n/d | Apple, Facebook, GitHub, Google guides (plus others) |
| 18 | https://www.keycloak.org/docs/latest/server_admin/index.html | Keycloak server admin guide | L1 | n/d | Built-in social IdPs: Facebook, GitHub, Google, etc.; Apple not seen in TOC (truncated page) |
| 19 | https://skycloak.io/blog/keycloak-production-sizing-ram-cpu-requirements/ | Keycloak sizing | L3 | n/d | ~1.25 GB per pod requested; 1-1.5 GB small production |
| 20 | https://docs.immich.app/administration/oauth ; https://docs.immich.app/administration/system-settings | Immich OAuth + settings | L1 | n/d | Generic OIDC (issuer URL) only; mobile uses custom scheme app.immich:///oauth-callback plus forwarder override; password login can be disabled |
| 21 | https://docs.gitea.com/usage/authentication | Gitea authentication | L1 | n/d | Built-in OAuth2 providers incl. GitHub, Google, Facebook + generic OIDC; Apple not listed |
| 22 | https://docs.nextcloud.com/server/latest/admin_manual/configuration_user/user_auth_oidc.html | Nextcloud user_oidc | L1 | n/d | OIDC client app; social login delegated to an IdP |
| 23 | https://zitadel.com/docs/category/apis/admin/identity-providers | Zitadel IdP API | L2 | n/d | Google, GitHub, Apple, generic OIDC/OAuth providers (Facebook not confirmed) [limited source] |
| 24 | https://docs.rs/openidconnect/latest/openidconnect/ | openidconnect crate | L1 | 4.1.0 | Discovery, PKCE, device flow types; docs say disable redirect-following (SSRF); release dates shown look unreliable [limited source] |
| 25 | https://docs.rs/oauth2/latest/oauth2/ | oauth2 crate | L1 | 5.0.0 | Auth code + PKCE, device code, refresh; owner ramosbugs |
| 26 | https://docs.rs/axum-login/latest/axum_login/ | axum-login 0.18 | L1 | n/d | Session auth middleware over tower-sessions; no OAuth built in |
| 27 | https://doc.libsodium.org/secret-key_cryptography/secretstream | libsodium secretstream | L1 | n/d | XChaCha20-Poly1305 stream; ordering/truncation/reorder detection; TAG_FINAL; ~256 GB per message |
| 28 | https://docs.cryptomator.org/security/architecture/ ; https://docs.cryptomator.org/security/vault/ | Cryptomator architecture, vault format 8 | L1 | n/d | scrypt N=32768 r=8 -> KEK wraps 2x256-bit keys (RFC 3394); per-file content key in 68-byte header; AES-GCM 32 KiB chunks, random nonce, AAD=chunk no + header nonce; names AES-SIV with parent dir ID |
| 29 | https://ente.com/architecture/ | Ente architecture | L2 | n/d | masterKey wrapped by password KEK (Argon2id, libsodium); recoveryKey<->masterKey cross-wrapped; collection/file keys; secretstream XChaCha20; crypto_box_seal sharing |
| 30 | https://proton.me/blog/protondrive-security | ProtonDrive security model | L2 | n/d | Per-node keys, blocks <=4 MB, passphrases signed to stop server forging tree; share-URL password never reaches server |
| 31 | https://bitwarden.com/help/send-encryption/ | Bitwarden Send encryption | L2 | n/d | 128-bit key in URL fragment; optional password is access control only, not part of encryption |
| 32 | https://eprint.iacr.org/2024/1616 | E2EE Cloud Storage in the Wild: A Broken Ecosystem (CCS 2024) | L1 | 2024 | Sync, pCloud, Icedrive, Seafile, Tresorit under malicious server: file injection, tampering, key replacement/overwriting; common failure patterns |
| 33 | https://eprint.iacr.org/2022/959 ; https://www.theregister.com/2022/06/22/megas_encryption_broken/ | MEGA: Malleable Encryption Goes Awry (S&P 2023) | L1/L3 | 2022 | RSA key recovery via login (>=512 logins), plaintext recovery, framing, Bleichenbacher variant; lack of key separation |
| 34 | https://nextcloud.com/encryption/ ; https://github.com/nextcloud/end_to_end_encryption_rfc/blob/master/RFC.md ; https://help.nextcloud.com/t/experience-e2e-in-nc-20-21/118882 | Nextcloud E2EE | L2/L4 | 2018-2021 | Folder-level; server features (web, preview, search, versioning) lost; slow/poor with large files, whole folder offline; group/single-file sharing out of scope [limited source: from search summary] |
| 35 | https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html | OWASP Password Storage | L2 | n/d | Argon2id minimums: 46 MiB/t1, 19 MiB/t2, 12 MiB/t3, 9 MiB/t4, 7 MiB/t5 (p=1) |
| 36 | https://developer.mozilla.org/en-US/docs/Web/API/SubtleCrypto | MDN SubtleCrypto | L1 | 2025-09 | AES-GCM, Ed25519, X25519 listed; no ChaCha20, Argon2 |
| 37 | https://wicg.github.io/webcrypto-modern-algos/ ; https://groups.google.com/a/chromium.org/g/blink-dev/c/TF8y175J3YU | WICG Modern Algorithms; Chromium Intent to Experiment | L1/L2 | 2026 | ChaCha20-Poly1305, Argon2, ML-KEM proposed (unofficial draft); Chromium experiment (Jun 2026) not shipped; XChaCha not proposed |
| 38 | https://github.com/RustCrypto/AEADs/blob/master/chacha20poly1305/README.md ; .../aes-gcm/README.md | RustCrypto chacha20poly1305, aes-gcm | L2 | n/d | One NCC Group audit, no significant findings; XChaCha20Poly1305 included; AES-NI/CLMUL intrinsics, constant time |
| 39 | https://github.com/fzyzcjy/flutter_rust_bridge | flutter_rust_bridge 2.12 | L2 | n/d | Dart<->Rust codegen; Android, iOS, desktop, web |
| 40 | https://pub.dev/packages/sodium_libs ; https://pub.dev/packages/cryptography_plus | Dart crypto packages | L3 | n/d | sodium_libs (native libsodium); cryptography_plus has XChaCha20 and Argon2id incl. pure Dart paths; no benchmarks found |
| 41 | https://github.com/antelle/argon2-browser ; https://github.com/Daninet/hash-wasm | Argon2 WASM benchmarks | L2 | n/d | Argon2 WASM feasible; only tiny-parameter benchmarks (no 64 MiB data) |
| 42 | https://pub.dev/packages/flutter_appauth/changelog ; https://pub.dev/packages/antinvestor_auth_runtime/changelog | Flutter OAuth packages | L3 | 2026 | flutter_appauth (AppAuth, PKCE); pod conflict with google_sign_in_ios drove one project to flutter_web_auth_2 |
| 43 | https://v2.tauri.app/learn/system-tray/ | Tauri 2 system tray | L1 | n/d | Linux: tray click events not emitted; context menu works |
| 44 | https://v2.tauri.app/plugin/updater/ ; https://v2.tauri.app/distribute/ | Tauri updater / distribution | L1 | n/d | Signed updates mandatory; updater artifacts: AppImage, NSIS/MSI, macOS .app.tar.gz; bundles: deb, rpm, AppImage, flatpak, snap, dmg, msi/nsis |
| 45 | https://v2.tauri.app/plugin/autostart/ | Tauri autostart | L1 | n/d | MacosLauncher::LaunchAgent; per-platform mechanics not documented |
| 46 | https://github.com/tauri-apps/tauri/issues/5889 | Tauri memory vs Electron | L2 | closed 2024-05 | WebKit-based Tauri used more memory than Electron on Linux for heavy web apps (370-580 MB); WebView2 similar to Electron; maintainers: smoke test only |
| 47 | https://github.com/tauri-apps/tray-icon | tray-icon crate | L2 | n/d | Linux backends: libappindicator (GTK3, needs event loop) or ksni (pure D-Bus SNI, own thread) |
| 48 | https://github.com/ubuntu/gnome-shell-extension-appindicator ; https://linuxiac.com/how-to-enable-system-tray-icons-in-gnome/ ; https://github.com/ublue-os/bluefin/discussions/4574 | GNOME tray status | L2/L3/L4 | 2017-2026 | GNOME dropped tray in 3.26; AppIndicator extension needed; GNOME 50 on Fedora 44 disabled the extension until upstream v63/v64 |
| 49 | https://docs.rs/crate/rfd/latest | rfd 0.17 | L2 | n/d | Native dialogs: Win32, AppKit, Linux xdg-desktop-portal (default) or GTK3; portal pick_folder falls back to zenity on failure |
| 50 | https://slint.dev/pricing | Slint licensing | L1 | n/d | GPLv3 for open source; royalty-free license for proprietary desktop apps |
| 51 | dev.to "Rust GUI from 135 MB to 30 MB" via https://www.pistack.xyz/posts/2026-08-25-rust-gui-frameworks-egui-iced-slint-comparison/ | egui vs Slint RSS | L3 | 2026-06/08 | One uncontrolled case: egui+GPU ~135 MB RSS vs Slint software renderer ~30 MB; no iced data [limited source] |
| 52 | https://github.com/notify-rs/notify | notify crate | L2 | n/d | inotify / FSEvents / ReadDirectoryChangesW backends; limits not covered |
| 53 | https://devclass.com/2026/01/14/code-signing-windows-apps-may-be-easier-and-more-secure-with-new-azure-artifact-service/ ; https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options | Windows code signing 2026 | L2 | 2026-01 | Azure Artifact Signing ~US$9.99/mo (US/CA/EU orgs; individuals paused Mar 2026 per one blog); EV no longer gives instant SmartScreen trust |
| 54 | https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html ; .../File_Upload_Cheat_Sheet.html | OWASP SSRF, File Upload | L2 | n/d | Allowlist, no redirects, DNS-rebinding defence; random server filenames, size limits, separate storage |
| 55 | https://inthewild.io/vuln/CVE-2023-4863 | libwebp CVE-2023-4863 | L2 | 2023-09 | Heap overflow in libwebp, exploited in the wild, CISA KEV; hits any server-side WebP decode |
| 56 | https://cure53.de/audit-report_ente-crypto.pdf ; https://www.credativ.de/en/blog/credativ-inside/cryptomator-securely-use-public-cloud-storage/ | Ente and Cryptomator audits | L2/L3 | n/d | Both have public Cure53 reports (content not machine-readable here) [limited source] |
| 57 | https://help.nextcloud.com/t/maximum-number-of-files-that-the-windows-client-can-handle/67910 | Sync-client scale anecdotes | L4 | n/d | Dropbox client crashes at ~900k files; no 100k-file RAM benchmark found for any client |

## Fact cards

### A. Social login for self-hosted software

1. Each operator must register their own provider apps; there is no shared central client ID in a self-hosted install. (Problem restriction; consistent with #20, #21.) High.
2. Immich ships a generic OIDC client only (issuer URL + client ID/secret), documents Google as an example, never documents Apple, and can disable password login. (#20) High.
3. Gitea has built-in OAuth2 providers (GitHub, Google, Facebook, ...) plus generic OIDC; Apple not listed. (#21) High.
4. Nextcloud delegates to an OIDC IdP via the user_oidc app; social login is not built in. (#22) High.
5. Brokers: Authentik documents Apple, Facebook, GitHub, Google (#17). Keycloak has built-in Facebook/GitHub/Google (#18; Apple unconfirmed). Dex has no Facebook or Apple connector and Google is "alpha" (#16). Zitadel supports Google, GitHub, Apple (#23). High (Dex/Authentik/Keycloak), Med (Zitadel).
6. A broker is heavy for "hundreds of users": Keycloak ~1-1.5 GB RAM per small production pod (#19, L3). It adds a second admin console and a second set of provider registrations to AC-22's 30-minute budget. Med.
7. Common pattern in the apps surveyed: built-in OIDC/OAuth client for simple cases (Immich, Gitea, Nextcloud), broker only for enterprise SSO. Our need (4 named providers incl. non-OIDC Facebook and GitHub) is closest to Gitea's built-in model. Med (inference).
8. Apple: paid Developer Program; Services ID must be tied to a primary App ID; domains/return URLs limited to 10 (individual) or 100 (org); Apple's page says no verification-file upload is needed. (#9) High.
9. Apple client secret is an ES256 JWT valid at most 15,777,000 s (~6 months); expired secret = `invalid_client` for all users; the .p8 key is downloadable once. (#11, L3 only; Apple's page not retrievable) Med.
10. Apple web flow with name/email scope returns via cross-site `form_post`; name arrives only on first authorization; a SameSite=Lax state cookie is not sent, so state/nonce/PKCE must live server-side keyed by `state` (or the cookie must be SameSite=None). (#12, L3) Med.
11. Apple "hide my email" yields @privaterelay.appleid.com addresses that never match other providers' emails, so link accounts on (provider, sub), not email. (#11) Med.
12. Facebook: apps requesting only public_profile + email generally need no App Review but must be Live mode with privacy-policy URL and a data-deletion callback/URL; Business Verification is required for Advanced Access; sources conflict whether public_profile needs Advanced Access for non-role users. (#13, #14) Med [limited source].
13. Google: consent screen in Testing status gives 7-day refresh tokens; unverified clients are capped at 100 users (#15); a 500-user install must be "In production". Whether openid/email/profile-only apps skip verification is not confirmed by fetched pages [limited source].
14. GitHub: OAuth App or GitHub App both work for sign-in; device flow must be enabled per app, needs no client_secret, 15 min code, 8-char user code, `slow_down` +5 s, PKCE S256 supported. OAuth-App tokens do not expire unless opted in. (#7) High.
15. Device-flow availability: GitHub yes (#7); Facebook yes but separate "Login from Devices" setting and client-token (#6); Google yes but only for scopes openid/email/profile etc. and a separate "TVs and Limited Input" client type (#5); Apple no device flow found in any source [limited source - absence of evidence, Apple's REST docs not retrievable]. Using four different provider device flows would mean four code paths. High (Google/GitHub/Facebook), Low (Apple absence).
16. RFC 8628 treats device clients as public clients; user codes are low-entropy so servers must rate-limit; attackers can phish by sending a code to a victim, so the approval page must show the device name and ask the user to confirm. (#2) High.
17. RFC 8252: native apps must use an external user agent plus PKCE; loopback `127.0.0.1` with any port, reverse-DNS custom scheme, or claimed https are the sanctioned redirects; claimed https preferred. (#1) High.
18. RFC 9700: public clients MUST use PKCE (S256); refresh tokens for public clients MUST be sender-constrained or rotated; exact redirect URI matching; `iss` check needed when talking to more than one AS. (#3) High.
19. Native provider SDKs (Google Sign-In, Sign in with Apple on iOS) bind to the app publisher's bundle ID/team, not the operator's server; a generic App Store/Play client talking to arbitrary operators' servers cannot rely on them. Immich avoids this with a browser flow and a custom-scheme redirect plus an https forwarder. (#20; inference for the SDK point) Med.
20. Flutter: flutter_appauth works with PKCE but conflicts with google_sign_in_ios pods; one 2026 project moved to flutter_web_auth_2. (#42, L3) Med.
21. App Store 4.8: an app using Google/Facebook login for its primary account must also offer an equivalent option (limited to name+email, can hide email, no ad tracking); Sign in with Apple is the usual answer. If an operator enables only Google, a shared App Store build would violate 4.8 unless Apple is available. (#8) High (rule), Med (consequence).
22. Guideline 5.1.1(v): in-app account deletion is required, and SIWA users' tokens should be revoked via Apple's REST API on deletion. (#10) High.
23. Rust crates: `openidconnect` 4.x (discovery, PKCE, id-token verification, device types) and `oauth2` 5.x (PKCE, device code, refresh) are the maintained de-facto pair; docs warn to disable HTTP redirect-following (SSRF). `axum-login` 0.18 has no OAuth support and sits on tower-sessions, which an opaque-token design does not need. (#24-#26) Med-High; version/date strings from docs.rs looked odd [limited source].
24. GitHub and Facebook are OAuth2, not full OIDC for web login (profile via REST `/user`, `/user/emails`, Graph `/me`); Google and Apple are OIDC with discovery. General knowledge [limited source].

### B. Sessions and tokens

25. RFC 9700 allows either rotation or sender-constraint for public-client refresh tokens; rotation with reuse detection is the simplest. (#3) High.
26. Revocation <= 5 s (AC-21) is trivial when every API call checks a server-side session row (opaque token, hashed in Postgres); JWT access tokens would need a denylist or <=5 s lifetimes anyway, so they add parts without benefit at this scale. Inference from AC and scale (hundreds of users, 100 concurrent). Med.
27. Provider outage or no internet blocks only new sign-ins; issued sessions keep working if sessions are server-local. Provider JWKS must be cached. Consistent with restriction "no internet = cannot authenticate". Med (inference).

### C. E2EE vault

28. Cryptomator vault format 8: per-file random content key in a 68-byte header, AES-256-GCM 32 KiB chunks with random 96-bit nonce and AAD (chunk number + header nonce), scrypt (N=32768, r=8) -> KEK wraps masterkeys, filenames AES-SIV bound to parent directory ID. (#28) High.
29. Ente: random masterKey wrapped by password-derived key (Argon2id via libsodium, parameters degraded stepwise if memory is short), recovery key and masterKey wrap each other, per-collection and per-file keys, file data in libsodium secretstream (XChaCha20-Poly1305), sharing by crypto_box_seal. (#29) High. Cure53 audit exists (#56) [limited source].
30. Proton Drive: per-node keys; passphrases signed with the user's address key to stop a malicious server from forging the folder tree; blocks <= 4 MB. (#30) High.
31. libsodium secretstream detects truncation, reordering, duplication and modification, with a mandatory TAG_FINAL; message size ~256 GB. (#27) High.
32. Random 96-bit AES-GCM nonces are only safe under a per-key invocation limit (NIST SP 800-38D, 2^32); per-file keys make this a non-issue at <=100 MiB, but a single shared key would need counter nonces. NIST page text not retrievable [limited source]. Med.
33. RustCrypto `chacha20poly1305` (includes XChaCha20-Poly1305) and `aes-gcm` each had one NCC Group audit with no significant findings; AES-GCM uses AES-NI/CLMUL, both constant time. (#38) High. The audit predates current versions [limited source].
34. Web Crypto: AES-GCM, Ed25519, X25519 available; ChaCha20-Poly1305 and Argon2 are only in an unofficial WICG draft, Chromium experiment as of Jun 2026, not shipped; XChaCha is not proposed. So a browser needs WASM (Rust or libsodium.js) for XChaCha and for Argon2id. (#36, #37) High.
35. Argon2id parameters: RFC 9106 (t=3, p=4, m=64 MiB when memory is limited) and OWASP minimum (m=19 MiB, t=2, p=1). Browsers cannot use p>1 without cross-origin isolation/threads [limited source], so p=1 is the portable choice. No data found on 64 MiB Argon2id timing in browsers or on low-end phones. (#4, #35, #41) High (values), Low (timing).
36. Dart: libsodium via `sodium_libs` or Argon2id/XChaCha in `cryptography_plus`; no benchmarks found; formats must match byte for byte across Rust/Dart/Web, which is the main risk of three separate implementations. (#40) Med.
37. `flutter_rust_bridge` 2.x lets Flutter call a single Rust crypto core on Android/iOS/desktop; the same crate can target WASM for the web. (#39 plus general Rust/wasm-bindgen knowledge) Med.
38. ETH CCS 2024: Sync, pCloud, Icedrive, Seafile, Tresorit (malicious server) had file injection, data tampering, key replacement/overwrite; patterns repeat across independent designs. (#32) High. MEGA 2022: key recovery via login, plaintext recovery, framing, and "lack of key separation". (#33) High.
39. Lessons from #32/#33: authenticate every wrapped key and metadata blob with AEAD+AAD (vault ID, object ID, version), never accept unauthenticated key material from the server, use key separation (distinct subkeys per purpose), avoid public-key sharing between users in v1 (the largest attack surface in these papers). Inference from papers; Med.
40. Nextcloud E2EE loses web access, preview, search, versioning, is slow with large files and needs whole folders offline; sharing is limited. (#34, L2/L4 [limited source]). Lesson: keep the vault a separate, simple feature (as the problem statement does).
41. Share link with key in URL fragment: Bitwarden Send uses a 128-bit fragment key; its optional password is only access control. (#31) High. Web E2EE inherent caveat: the server delivers the JS that decrypts, so a malicious server can steal the fragment [general knowledge, limited source].
42. Peer file-size limits: Proton states no file-size limits on its pages (proton.me/drive/file-sharing/send-large-files, L2 marketing) [limited source]; no reliable limits found for Ente/Tresorit/Filen/Mega. Cryptomator streams via a virtual drive with no stated cap. Nextcloud warns of poor large-file behavior (#34). So 100 MiB (AC-14) is conservative, not a technical limit of chunked AEAD.

### D. Desktop tray and settings

43. Tauri 2 provides tray, dialog (rfd-based), autostart (LaunchAgent on macOS), signed updater, bundles for deb/rpm/AppImage/msi/nsis/dmg. Updater artifacts: AppImage, NSIS/MSI, macOS app.tar.gz; deb/rpm not covered by updater. (#43-#45) High.
44. Tauri on Linux: tray click events are not emitted, menu works. (#43) High.
45. Memory: for heavy web apps, WebKit-based Tauri used more memory than Electron on Linux (370-580 MB), WebView2 ~ Electron (#46). Blog figures of 30-100 MB idle for simple Tauri apps are unmeasured marketing-grade (L3 blogs, e.g. tech-insider.org/tauri-vs-electron-2026). No source gives RSS for a tray-only Tauri app or says whether closing a window frees WebKitGTK processes. [limited source] Must be measured on the three OSes.
46. `tray-icon` on Linux: libappindicator needs GTK3 and an event loop; `ksni` backend is pure D-Bus StatusNotifierItem with its own thread. (#47) High.
47. GNOME has had no tray since 3.26; users need the AppIndicator extension; on Fedora 44/GNOME 50 it was silently disabled until upstream v63/v64. KDE, XFCE, Cinnamon, MATE support SNI natively [general knowledge, limited source]. (#48) High (GNOME), Med (breakage).
48. rfd: portal-based folder picker by default on Linux with zenity fallback; native Win32/AppKit dialogs elsewhere; satisfies AC-17 without drawing a file browser. Tauri's dialog plugin uses it. (#49) Med.
49. Slint is GPLv3 for open-source apps or royalty-free for proprietary desktop apps (license choice follows sunnycloud's license, still undecided). egui (GPU) vs Slint (software renderer) measured ~135 vs ~30 MB RSS in one uncontrolled case; no iced data. (#50, #51) Med/Low.
50. Windows: a Windows *service* runs in session 0 and cannot show a tray icon; the tray needs a logon task or HKCU Run entry in the user session [general knowledge, limited source]. Restriction text allows "Windows service or logon task": only the logon task fits a tray daemon.
51. Code signing: macOS Developer ID + notarization (US$99/yr, Apple program); Windows Azure Artifact Signing ~US$9.99/mo (eligibility limits) or OV cert ~US$130-300/yr; EV no longer yields instant SmartScreen reputation. (#44, #53) Med.
52. No public RAM figure for any sync client at 100k files was found (#57). AC-15's 100 MB must be proven by our own measurement.

### E. Threat-model references
53. SSRF: allowlist, no redirects, resolve-then-pin IPs against DNS rebinding. File uploads: random server filenames, size limits, serve from separate origin/storage. (#54) High.
54. Server-side image decoders are an attack surface: libwebp CVE-2023-4863 was exploited in the wild and is on CISA KEV. (#55) High.

## Options comparison

### 1. Auth architecture
| Rank | Option | Pros | Cons | Verdict |
|------|--------|------|------|---------|
| 1 | Built-in client in the Rust API: `openidconnect` for Google + Apple, `oauth2` + REST profile for Facebook + GitHub; own opaque sessions | One component (AC-22); full control of device flow, mobile redirect, session revocation; matches Gitea precedent; operator only registers provider apps | We own 4 provider quirks (Apple form_post and secret JWT, Facebook Graph, GitHub emails); security-critical code we must test | Recommended |
| 2 | Embedded broker (Authentik/Zitadel) as optional compose service | Provider quirks handled by maintained product; Apple supported | 2nd product, DB, admin UI; Authentik/Zitadel RAM and upgrade burden; our API still speaks OIDC to it | Fallback if provider quirks keep breaking |
| 3 | Dex | Light | No Facebook, no Apple, Google alpha (#16) | Rejected |
| 4 | Keycloak | Mature, Facebook/GitHub/Google built-in | ~1-1.5 GB RAM (#19), heavy ops, Apple unconfirmed | Rejected for this scale |
| 5 | Generic OIDC only (Immich model) | Smallest code | Facebook and GitHub are not OIDC; violates AC-19 | Rejected |

### 2. Desktop login mechanism
| Rank | Option | Pros | Cons |
|------|--------|------|------|
| 1 | Server-issued RFC 8628 device grant on our own API (`/api/auth/device/code`, browser page `/device` behind normal social login, daemon polls `/api/auth/device/token`) | Provider-independent (Apple has no device flow); works headless/SSH; matches restriction "device-code flow (browser approval)"; token never touches provider | We must implement user-code entropy, rate limits, phishing UX (#2) |
| 2 | Loopback redirect RFC 8252 to our own API's auth-code+PKCE | No code typing; standard | Needs local port + browser on same machine; fails headless/remote; firewalls |
| 3 | Per-provider device flows | No server UI | 4 code paths; Apple none; Google restricted client type (#5); Facebook separate setup (#6) |
| 4 | Custom URL scheme | Simple on macOS | Scheme hijack risk (#1), awkward on Linux |

### 3. Vault cipher and format
| Rank | Option | Pros | Cons |
|------|--------|------|------|
| 1 | Shared Rust core; per-file random key; chunked XChaCha20-Poly1305 in STREAM-style (RustCrypto `aead::stream`, final-chunk flag), chunk 64 KiB-1 MiB fixed so ciphertext offsets are computable for resume (AC-3) | Random 192-bit nonces safe; fast and constant time without AES hardware; one implementation for daemon/Flutter(FFI)/web(WASM); audited crate (#38) | Not Web Crypto-native (WASM needed); custom file format to specify and test; no off-the-shelf third-party reader |
| 2 | libsodium secretstream (Ente-proven format) via libsodium-sys/sodium_libs/libsodium.js | Widely reviewed format, readers exist in many languages; built-in truncation protection (#27) | Three native bindings instead of one core; secretstream state is sequential (resume needs checkpoints or fixed chunk framing) |
| 3 | AES-256-GCM chunked (Cryptomator style) | Web Crypto native, AES-NI/ARMv8 speed, no WASM for the cipher | Must hand-build truncation/ordering protection (AAD with chunk index + final flag); nonce discipline needed; Web Crypto is not streaming |
Rejected: AES-CBC/CTR+HMAC hand-rolled, XTS, per-file shared nonces, ECB.

### 4. KDF and key hierarchy
| Item | Recommendation | Basis |
|------|----------------|-------|
| KDF | Argon2id, m=64 MiB, t=3, p=1, 128-bit salt, 256-bit output; parameters stored in vault header (upgradable) | RFC 9106 second profile with p=1 for browsers; above OWASP minimum (#4, #35) |
| Hierarchy | Passphrase -> KDF -> KEK -> wraps random 256-bit Vault Key (VK). Recovery Key (random 256 bits, shown once as grouped base32 or words) wraps VK separately. Per-file random File Key wrapped by VK. Subkeys via HKDF for names/metadata/files (key separation) | Ente/Cryptomator (#28, #29); #33 lesson |
| Wrap | AEAD with AAD = vault ID + object ID + format version | #32 key-replacement class |
| Names/metadata | Encrypt names, sizes-in-metadata, timestamps in one authenticated metadata blob per entry, AAD binds parent ID (Cryptomator SIV-with-parent-ID idea); server still sees ciphertext sizes, counts, tree shape, access times | #28, #30 |
| Sharing | Per-file link: File Key in URL fragment (128-256 bit), optional link password as server-side access control only | #31 |
| Rotation | Passphrase change = rewrap VK (no file re-encryption). VK/File-Key compromise = re-encrypt; link revocation stops serving ciphertext but cannot un-leak a key already used | inference |

### 5. Desktop UI toolkit
| Rank | Option | Pros | Cons |
|------|--------|------|------|
| 1 | Two processes: headless daemon `sunnycloudd` (tray via `tray-icon`/`muda` on mac/Win, `ksni` on Linux; notify; SQLite index) + `sunnycloud-settings` as Tauri 2 + Svelte launched on demand from tray menu | Reuses Svelte skills/components; webview RAM only while window open (AC-15 stays daemon+tray only); rfd dialogs (AC-17); Tauri bundler/updater | Two binaries/toolchains; WebKitGTK runtime dep on Linux; WebView2 cold start vs AC-16 must be measured |
| 2 | Same daemon + settings in Slint (software renderer) on demand | Smallest RAM (~30 MB in one case), no webview dependency, fast start | New UI stack, GPLv3 vs royalty-free license decision (#50); no Svelte reuse; installer/updater we build ourselves |
| 3 | Everything inside one Tauri process (tray + daemon + on-demand window) | One binary | Webview lifecycle may inflate daemon RSS (unmeasured); Linux tray events missing (#43) |
| 4 | egui/eframe or iced | MIT/Apache | GPU stack RAM (egui ~135 MB, #51); iced unmeasured; less accessibility; no packaging help |
| 5 | Electron | Mature | RAM and size; contradicts "native Rust" |
IPC: one local JSON-RPC socket (Unix domain socket in `$XDG_RUNTIME_DIR` / macOS app dir with 0600; Windows named pipe with user-only ACL) for status, get/set config, pause, log tail; config persisted as a file the daemon reloads. Avoid loopback TCP (any local process can connect).
Auto-update: Tauri updater for AppImage/NSIS/macOS; deb/rpm via a package repo; Flatpak optional. Windows autostart via logon task (tray needs user session).

## AC realism table

| Criterion | Our value | Researched value | Note |
|-----------|-----------|------------------|------|
| AC-13 server stores only ciphertext | byte-different, decrypts on client | Achievable (Cryptomator/Ente do it). Real risk is not confidentiality of content but metadata leakage and malicious-server attacks (#32, #33) | Add tests: server cannot decrypt, tampered chunk/truncation/reorder rejected, wrong-vault ciphertext rejected. State what leaks: sizes, counts, timing, tree shape |
| AC-14 vault file <= 100 MiB | 100 MiB, larger rejected | Peers: Proton advertises no limit; Cryptomator streams (no cap); Nextcloud warns about large files (#34). Chunked AEAD has no 100 MiB barrier; the cap is about browser memory (Web Crypto/WASM buffers) | Reasonable default; make it config. Excludes most 4K video and large RAW, so users must be told vault suits documents and photos. Cost low: chunk size fixed keeps AC-3 resume (10 MiB) workable |
| AC-15 (context for UI) 100k files, <=1% CPU, <=100 MB, scan <=60 s | as stated | No published numbers for any client at 100k files (#57). Dropbox reportedly fails at ~900k (L4). Metadata-only scan of 100k files in 60 s is plausible; content hashing is not | Clarify: scan = stat/metadata; hashing incremental; index in SQLite, not RAM; state whether settings process counts. Linux inotify watch limit must be handled (watch directories, raise/handle ENOSPC) [limited source] |
| AC-16 settings window <=1 s; setting applies <=5 s | 1 s / 5 s | Window open: Slint/native ~ tens of ms; Tauri/WebView2 typically sub-second warm, cold start on slow Windows or WebKitGTK unmeasured [limited source]. Applying in 5 s: trivial with IPC push or file-watch | Measure cold start on the weakest supported machine. If Tauri misses 1 s, show a native splash or pre-spawn hidden window on tray hover |
| AC-19 sign in with all four | each of 4 works | Possible, but operator cost differs: GitHub minutes; Google consent screen must be In production (Testing = 7-day refresh, 100-user cap, #15); Apple US$99/yr + App ID + Services ID + key, secret JWT every <=6 months (server can self-sign from the .p8, removing the chore), enrolment can take days [limited source]; Facebook Live mode + privacy-policy + deletion URL + possible Business Verification (#13, #14) | Rewrite as "each configured provider". CI cannot hit real providers: use a mock OIDC/OAuth server in tests plus a manual release checklist per provider. Also AC-22 (30 min) is only credible with one provider or with provider accounts pre-existing |
| AC-20 >10 failed sign-ins/min/IP rejected | 10/min/IP | With social-only login there are no passwords to guess; "failed sign-in" must be defined: bad state/PKCE/code, invalid tokens, device user-code guesses, share-link password failures. Per-IP counting is wrong behind NAT/CGNAT and a reverse proxy unless X-Forwarded-For is trusted only from the known proxy. RFC 8628 says rate-limit user-code attempts (8 chars base-20 ~34.5 bit) (#2) | Add per-user_code (e.g. 5 tries then burn code), per-account and per-link limits; 429 with Retry-After; implement in API or proxy once |
| AC-21 revoke -> sync stops <= 5 s | 5 s | Easy with opaque server-checked tokens (per-request DB/cached lookup, cache TTL <= 2 s, Postgres NOTIFY to drop live connections). Hard with long-lived JWTs without denylist. Long uploads/SSE/WebSocket must be actively cut | Test on the sync stream, not just the REST call. Revoking a provider account at Google/Apple does not revoke our session unless we poll/act on provider events (Apple server-to-server notifications) |

## Recommendation

**Auth architecture:** Built-in OAuth/OIDC client in the Rust API (Option 1): `openidconnect` for Google and Apple, `oauth2` + provider REST calls for Facebook and GitHub. Identity key = (provider, sub); never auto-link by email. Server issues its own opaque, hashed-at-rest session/refresh tokens (rotation with reuse detection), HttpOnly SameSite cookie for web; PKCE S256 everywhere; Apple client-secret JWT minted by the server from the operator's .p8 (removes the 6-month rotation chore). Operators get signup control (invite / allowed domains / admin approval), because open social login on a self-hosted server otherwise lets any GitHub user register.

**Clients:** Web = server-side code flow. Mobile = system browser (ASWebAuthenticationSession/Custom Tabs via flutter_web_auth_2) to our `/api/auth/...`, returning to the app via claimed-https or custom scheme with a one-time code redeemed with PKCE; no native provider SDKs (their client IDs belong to the app publisher, not the operator). Desktop = server-issued RFC 8628 device grant (user code shown by daemon/tray, approval page after normal social login, device name shown, user-code rate limits); loopback redirect as optional convenience.

**Vault:** One Rust core crate (native for daemon, WASM for web, flutter_rust_bridge for mobile). Per-file random key, chunked XChaCha20-Poly1305 STREAM construction with authenticated final chunk, fixed chunk size. Argon2id m=64 MiB, t=3, p=1 (params in header), passphrase -> KEK -> Vault Key; separate recovery key wraps Vault Key; File Keys wrapped by Vault Key; HKDF key separation; names/metadata encrypted and authenticated with AAD binding to vault/object IDs; vault is single-owner, sharing only through per-file fragment-key links. Fall back to libsodium secretstream only if an independently implementable, widely reviewed format matters more than a single Rust core.

**UI toolkit:** Headless Rust daemon with tray via `tray-icon`/`ksni`, plus a separate on-demand settings app in Tauri 2 + Svelte (rfd native dialogs), local JSON-RPC socket, config file reloaded by daemon. Switch to Slint if measured cold start of the Tauri settings window exceeds 1 s, or if webview dependencies (WebKitGTK) prove painful to package, and only after deciding the project license (Slint GPLv3 vs royalty-free).

**Deciding facts:** (1) Dex lacks Facebook/Apple and Keycloak costs ~1 GB+ RAM, so a built-in client wins at AC-22 scale. (2) Apple has no device flow and Google/Facebook device flows are separate special client types, so our own RFC 8628 endpoint is the only uniform desktop login. (3) Web Crypto lacks XChaCha20/Argon2 and ETH papers show divergent implementations break, so one Rust core (WASM/FFI) beats three ports.

**What would change it:** Operators demand enterprise SSO/LDAP -> add generic OIDC upstream (still our API) or document Authentik. Provider-specific bugs dominate maintenance -> adopt Authentik/Zitadel as optional broker. Browser vendors ship ChaCha20-Poly1305/Argon2 in Web Crypto -> drop WASM crypto in web. Measured Tauri memory/cold start fails AC-15/16 -> Slint. Need vault sharing between users -> redesign with authenticated public keys (high-risk area per #32/#33), out of v1.

## Open questions
1. Who publishes the iOS/Android and desktop binaries, and under which signing identities? A shared App Store build must satisfy Guideline 4.8 even if an operator configures only Google, and needs a demo server for App Review [limited source].
2. Google: does an app requesting only openid/email/profile for 500 users avoid verification and the 100-user cap? Verify with a real project before promising AC-19.
3. Facebook: is Business Verification or Advanced Access for public_profile required for non-role users in 2026? Docs and posts conflict (#13, #14).
4. Apple: confirm client-secret max lifetime and absence of device flow on Apple's own pages (not retrievable here); confirm `sub` stability if an operator changes Apple team.
5. Measure: Argon2id 64 MiB timing in Chrome/Firefox/Safari WASM and on a low-end Android/iPhone; XChaCha20 WASM throughput for 100 MiB; Tauri settings cold start on Windows/WebKitGTK; daemon RSS at 100k files with tray on each OS.
6. Does AC-15's 100 MB include the on-demand settings process?
7. Project license (affects Slint option); GPL/AGPL implications for libraries.
8. Is "encrypted at rest" on the server per-user keys, per-install key, or disk-level (LUKS)? Defines what a stolen disk or backup reveals.
9. Linux support matrix: GNOME without extension has no tray, so daemon must be fully usable without tray (CLI status, notifications, .desktop settings launcher).

## Missing security requirements (not in security_approach.md)

- **Signup control:** open social login on a self-hosted server lets any GitHub/Google/Facebook user create an account; need invite/allowlist/domain/admin-approval and an admin role model.
- **Account linking and takeover:** link by (provider, sub), require re-auth to link, treat provider email as unverified unless `email_verified`; Apple private relay; lockout when a user loses their provider account (admin-assisted re-binding; vault recovery key is independent).
- **OAuth hygiene (RFC 9700):** state/nonce/PKCE, exact redirect URIs, `iss` check, one-time auth codes, no open redirect after login; Apple form_post exception to CSRF/SameSite rules.
- **Session/token theft:** HttpOnly+Secure+SameSite cookies, short access lifetime, refresh rotation with reuse detection (revoke the family), device tokens stored in OS keychain (Keychain/Credential Manager/Secret Service), not plain files; logout everywhere; list/revoke devices.
- **CSRF and web headers:** Origin checks, CSP, HSTS, `X-Content-Type-Options: nosniff`, frame-ancestors, referrer policy (share-link tokens must not leak via Referer).
- **Multi-tenant isolation:** every query scoped by owner; Postgres row-level security as defence in depth; opaque storage IDs (no user-controlled paths on ext4); per-user quotas; tests for IDOR across users.
- **Path traversal:** server never builds paths from client names; sync clients reject `..`, absolute paths, symlinks leaving the sync root, Windows reserved names, case-collisions, NTFS ADS colons (#54).
- **Upload abuse:** size and concurrency caps, quotas, per-user rate limits, chunk-size validation, resumable-session expiry and cleanup, decompression/pixel bombs in thumbnails and transcodes, content-type sniffing off, user content served from a separate origin or with `Content-Disposition: attachment`+CSP sandbox (SVG/HTML XSS).
- **Rate limiting beyond sign-in:** share-link access, share password attempts (hash link passwords with Argon2id, rate-limit), device user-code guesses, API per-token limits; trusted-proxy handling for client IP.
- **Share-link enumeration:** >=128-bit random tokens, no sequential IDs, uniform 404 for missing/expired/revoked, expiry and revocation enforced server-side without CDN caching beyond 5 s, download counters, optional per-link audit.
- **Malicious media parsing:** run thumbnail/transcode workers sandboxed (no network, seccomp/container, CPU/mem/time limits, read-only inputs); pin and patch libwebp/libheif/ffmpeg (CVE-2023-4863, #55); ffmpeg protocol whitelist so crafted playlists cannot read local files or reach internal hosts [limited source].
- **SSRF:** provider discovery/JWKS fetches only to configured issuers, redirects off (openidconnect advice, #24), IP-pinning against rebinding, block link-local/metadata ranges; any future URL preview or webhook goes through the same fetcher (#54).
- **Vault-specific:** document leakage (sizes, counts, tree, timestamps); downgrade protection for format/KDF params; passphrase strength check client-side; recovery-key confirmation at creation; client-side KDF cost cap so a malicious server cannot force huge Argon2 parameters (DoS) or weak ones (downgrade); web-delivered JS trust caveat and CSP/SRI for the vault page.
- **Server-side secrets and at-rest keys:** storage of provider secrets, Apple .p8, at-rest data key and where it lives (env/secret store/KMS), key rotation procedure, encrypted backups.
- **Audit logging:** sign-ins (success/failure, provider), device approve/revoke, session revoke, share create/revoke/access, vault create/unlock failures, admin actions, account deletion; append-only, no secrets/PII beyond IDs, retention policy.
- **GDPR/privacy:** account deletion cascading to blobs, thumbnails, transcodes, shares, search/index rows, and backup retention window; Apple token revocation and in-app deletion (#10); Facebook data-deletion callback (#14); data export; privacy policy URL required by Google/Facebook/Apple.
- **Supply chain and updates:** signed desktop/mobile builds and updater (#44), `cargo audit`/`cargo deny`, reproducible CI on GitHub Actions with pinned actions, SBOM, Docker image signing.
- **Availability:** DoS limits on expensive endpoints (Argon2 for link passwords, transcodes), per-user worker fair share, graceful behaviour when providers or the internet are down.
