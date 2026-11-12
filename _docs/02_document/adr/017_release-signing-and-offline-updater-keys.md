# ADR-017: Release signing and offline updater keys

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

Release signing is where one stolen secret becomes code execution on every client: a stolen updater key replaces the desktop binary on all installs, and a second CI system that also signs is a second trust root (R17, scored High with a critical consequence). The project has a primary CI on GitHub Actions and a fallback of Forgejo (git.azaion.com) with Woodpecker (ci.azaion.com) named in the restrictions. Woodpecker cannot obtain a GitHub OIDC identity, so keyless signing from it is impossible, and giving it a long-lived key would create the second root.

Two artefact families need different treatment. Server images are pulled by operators who can run `cosign verify`. Desktop agents update themselves, with the update source fixed at build time and never the sync server (AC-49). The decoder image is the exception to digest pinning: it is rebuilt at least every 7 days so ffmpeg, libheif, libvips and libwebp security fixes reach operators without a new release (AC-63), so the release compose references its moving `<semver>` tag.

The owner answered at Step 4 (S4-3): the manifest is signed offline, never by an automatic CI secret; the app pins two Ed25519 public keys (active and recovery); the manifest carries `version`, per-OS entries with SHA-256, a monotonic `serial` and an `expires` time; the exact layout is an ADR.

- Acceptance criteria addressed: AC-49 (the updater rejects unsigned, other-key and lower-version builds; the source is fixed at build time), AC-63 (release images signed with an SBOM, decoder rebuilt at least every 7 days)
- Restrictions addressed: RESTRICT-30 (GitHub Actions pinned by commit SHA, image digests pinned, fallback Forgejo and Woodpecker), RESTRICT-06 (Slint settings window, Tauri 2 only as the fallback), RESTRICT-29 (decoder image ships GPL ffmpeg; source offer per build)
- Risks addressed: R17 (signing trust: updater key theft, second CI as signing root, mutable decoder tag), R19 (Windows MSI unsigned or signing rejected)
- Research source: `_docs/00_research/raw/b_security.md` findings F29 and F30 (auto-update and supply chain)

## Decision

We will sign releases only from the GitHub release workflow and never from the fallback CI. Forgejo and Woodpecker run the PR gate (build, test, image build) and nothing else: no signing, no release, no `stage`/`main` tag pushes. If GitHub is unavailable, a release waits.

Server images (api, worker, decoder) are built from the tagged commit, pushed by digest to GHCR and signed with keyless `cosign` (GitHub OIDC, Sigstore) on the digest only, with an SBOM attestation and provenance. Before signing, the workflow starts the pushed `api@digest` and `worker@digest` and asserts that both test-clock routes return 404. The release compose (`sunnycloud-deploy-<ver>.tar.gz`) pins caddy, db, api, migrate and worker by digest taken from `images.lock`; the decoder alone keeps its moving `:<semver>` tag, which the weekly rebuild moves after its own cosign signing and scan, and operators verify with the printed `cosign verify` command. The release workflow uploads a draft GitHub Release and an unsigned manifest draft; the environment `release` requires the owner's approval; the owner then signs the manifest offline with the active updater key and attaches it before publishing. The workflow holds no updater key.

The desktop daemon compiles in the update source (GitHub Releases) and two Ed25519 public keys, active and recovery, and ignores any environment or settings override. It accepts a manifest only when the signature verifies against either key, `expires` has not passed, `serial` is greater than the last accepted one, `version` is strictly greater than the installed one and the downloaded payload matches the signed per-OS SHA-256. The recovery key lives in cold storage and signs only a manifest that rotates trust, together with a new build that pins the new pair. Windows MSI code signing is an open owner decision (D-1): SignPath Foundation is the proposed route once the AGPL licence file exists; until then the MSI is unsigned with a documented SmartScreen warning.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| One updater key held as a CI secret and used automatically | A GitHub or Woodpecker compromise would sign malware for all desktops: the exposure is 100% of installs with no human step. Offline signing adds one owner action per release and removes the secret from every CI system |
| Tauri updater for the daemon and settings | The daemon is a Rust process with its own tray; Tauri is only the fallback to Slint (RESTRICT-06), and its updater would couple signing to a UI toolkit that may not ship. The dedicated verifier is about the five checks above and covers deb, which the Tauri updater does not |
| No auto-update (manual downloads only) | Leaves AC-49 untestable and stale daemons in the field: a protocol fix could not reach clients, and `upgrade_required` (HTTP 426, AC-64) would strand desktop users on a hand-installed binary |
| Fallback CI also signs (own key or second OIDC trust) | Creates the second signing root R17 names; Woodpecker has no GitHub identity, so it would need a long-lived key. One root means one verification identity regexp for operators |
| Pin the decoder by digest like the other images | Weekly security rebuilds would reach no operator until the next release, which breaks the 7-day freshness in AC-63; the moving tag is cosign-verified and the immutable dated tag stays as the record |

## Consequences

### Positive

- A compromise of any CI system cannot produce an accepted update manifest: it holds no updater private key. The updater test (14 § 14) requires 8 of 8 bad builds to be rejected (unsigned, unpinned key, expired, replayed serial, lower or same version, altered payload, truncated), one case per check.
- Operators verify images with one identity pattern (`release.yml` at a `v*` tag), and the digest-pinned compose makes an install reproducible.
- `expires` and `serial` bound a leaked or replayed manifest: an old manifest dies at its expiry and cannot be replayed once a higher serial was accepted.

### Negative

- Every desktop release needs the owner to sign by hand with an offline key; a missed or late signing delays the update, and the manifest's `expires` forces a periodic re-sign.
- Losing both updater keys means a one-time manual reinstall for every desktop user; the key backup rule is two offline copies of each, the same as the master key.
- The decoder's moving tag is the one mutable reference in a release; an attacker who compromised the GitHub workflow could move it, so its trust is exactly the workflow's.
- If GitHub is down, no release can be signed; the fallback CI cannot cover for it.
- Windows signing is unresolved, so v1.0 may ship an unsigned MSI with SmartScreen warnings (R19).

### Neutral / Open

- Open: Windows signing route and timing (SignPath Foundation proposed; alternatives are a purchased OV or EV certificate, Azure Trusted Signing, or unsigned).
- Decided 2026-11-11 (AZ-3887): the offline signing tool, see "Offline signing tool" in Notes.
- Does not change image pinning for caddy and postgres (digests, RESTRICT-30) or the Android signing path (Play App Signing, upload key as an Environment secret).

## Evidence

- `_docs/02_document/canonical-names.md` § Owner decisions at Step 4 (2026-11-04), S4-3; § Technical defaults applied at Step 4 (decoder isolation line, signing only from the GitHub workflow)
- `_docs/02_document/architecture.md` § 5. Integration Points (External Integrations, GitHub Releases); § 7. Security Architecture (Data protection, Updates)
- `_docs/02_document/deployment/ci_cd_pipeline.md` § Principles (Platform, Pinning, Least privilege); § Image build, signing, SBOM and scanning (AC-63) (Fallback, Weekly rebuild); § Release workflow (`release.yml`, tag `v*` on `main`); § Release artefacts (updater keys); § Owner decisions this document depends on (D-1, D-3)
- `_docs/02_document/deployment/containerization.md` § Image tagging
- `_docs/02_document/components/14_desktop_daemon/description.md` § 5. Implementation Details (Updater); § 14. Component acceptance; § 15. Risks (open item: manifest layout)
- `_docs/02_document/risk_mitigations.md` § Risk Register (R17, R19)
- `_docs/00_problem/acceptance_criteria.md` AC-49, AC-63

## Notes

The layout below is the **confirmed layout (owner, 2026-11-11, AZ-3887)**; it was first proposed with the ADR set: the owner said the layout is an ADR, and this section is where it is recorded. It is the minimum that satisfies S4-3 and the daemon's check order. The manifest is UTF-8 JSON, `update-manifest.json`, with a detached Ed25519 signature file `update-manifest.json.sig` over the exact manifest bytes (no canonicalisation step, so the signer and verifier see the same bytes):

```
{ "format": 1,
  "version": "1.0.1",
  "serial": 7,
  "expires": "2027-01-15T00:00:00Z",
  "platforms": {
    "linux-x86_64":   { "asset": "sunnycloud-desktop_1.0.1_amd64.deb", "sha256": "<hex>", "size": 18234112 },
    "windows-x86_64": { "asset": "sunnycloud-desktop-1.0.1.msi",       "sha256": "<hex>", "size": 21503488 } } }
.sig file: { "key": "active" | "recovery", "sig": "<base64 Ed25519 signature>" }
```

`asset` is a file name resolved against the compiled-in GitHub Releases source of the matching tag, never a full URL; the download follows at most a few redirects and only to a compiled-in list of GitHub hosts (github.com and its release-asset hosts; owner decision 2026-11-12), any other host is refused, and the signature and the signed size and SHA-256 still decide acceptance. The release public keys (active, recovery) do not exist yet: the owner creates both offline before the first desktop release and they are pinned in the daemon's trust file (release blocker); until then a release build refuses every manifest. The manifest is a file name, never a full URL, so the manifest cannot redirect the download. The `key` field is a hint; the verifier may try both pinned keys. The daemon stores the highest accepted `serial` in its local `meta`. `format` lets a later layout change be refused by old daemons instead of misread. `size` allows a truncation check before hashing.

Re-evaluate Windows signing when the AGPL `LICENSE` file exists (SignPath requires an OSI licence). Revisit the decoder tag policy if the owner prefers digest pinning at the price of a manual operator step for weekly rebuilds (the "decoder image tag versus digest pinning" owner choice in `risk_mitigations.md` § Summary).

### Where each updater check reads its input

| Check (order in the daemon) | Input |
|---|---|
| Signature | `update-manifest.json.sig` field `sig`, over the exact manifest bytes, against either pinned key (`key` is only a hint) |
| Layout | `format` (an unknown value is refused, never misread) |
| Expiry | `expires` |
| Replay | `serial`, compared with the highest accepted serial stored in the daemon's local `meta` |
| Version | `version`, strictly greater than the installed one |
| Platform entry | `platforms.<os>-<arch>`; no entry for this machine means no update |
| Source | `asset` is a file name resolved against the compiled-in GitHub Releases source of the matching tag, never a URL |
| Size, then digest | `size` before hashing (truncation), then `sha256` of the downloaded payload |

### Offline signing tool (decided 2026-11-11, AZ-3887; agent decision, the owner may overturn)

- **Form and place**: a second small binary (`update-sign`) of the updater's crate in the repository, so it shares the manifest type with the verifier; built by E-24/c18 with the updater. Not a script, so the bytes that are signed are the bytes the verifier reads.
- **Inputs**: `--manifest <file>`, `--key <file>` (a regular file holding the 32-byte Ed25519 seed, readable by its owner only), `--slot active|recovery`.
- **Output**: `<manifest>.sig`, the JSON `{"key": "<slot>", "sig": "<base64>"}`, written next to the manifest. Before writing, the tool verifies the signature against the pinned public key of that slot, so a wrong key file is caught at signing time.
- **Key handling**: the private key is read only from the file the operator provides at signing time. The tool has no option and no code path that reads key material from an environment variable, an argument or standard input, refuses a key file that other users can read, refuses to run when `CI` or `GITHUB_ACTIONS` is set (a guard against a signing run on CI, not a security boundary: the workflow holds no updater key at all), and never prints the key.
- **Who builds it**: E-24/c18 (shares the manifest type); E-31/c14 uses it with the test keys to make the fixtures.

### Release runbook step

1. The release workflow uploads the draft GitHub Release and an unsigned `update-manifest.json` (proposed `expires`: the release date plus 90 days; the owner may edit the manifest before signing).
2. The owner approves the `release` environment, downloads the manifest to the offline machine holding the active key.
3. The owner runs `update-sign --manifest update-manifest.json --key <active key file> --slot active`.
4. The owner attaches `update-manifest.json.sig` to the draft and publishes. The recovery key signs only a manifest that rotates trust, together with a new build that pins the new pair.

### Prototype evidence (throwaway, test key pair only, no real updater key)

OpenSSL 3.5.5 Ed25519 raw signing of `update-manifest.json` exactly as written (no canonicalisation step): the exact bytes verify; flipping one byte of the manifest fails; flipping one byte of the signature fails; a whitespace-only change that leaves the JSON meaning intact also fails, which is the bytes-as-signed rule.

### DS-31 fixtures: how the ten cases are made (E-31/c14, test keys A active, R recovery, B unpinned)

| # | Case | Made by |
|---|---|---|
| 1 | good, active key | manifest with `version` above the installed one, `serial` above the last accepted, `expires` in the future; signed with A (`"key": "active"`) |
| 2 | good, recovery key | the same manifest signed with R (`"key": "recovery"`) |
| 3 | unsigned | the case 1 manifest published with no `.sig` file |
| 4 | unpinned key | the case 1 manifest signed with B |
| 5 | expired | the case 1 manifest with `expires` in the past, signed with A |
| 6 | replayed serial | case 1 after it was accepted: a manifest with the same `serial` and a higher `version`, signed with A |
| 7 | same version | `version` equal to the installed one, higher `serial`, signed with A |
| 8 | lower version | `version` below the installed one, higher `serial`, signed with A |
| 9 | altered payload | the case 1 manifest and signature, with one byte of the asset changed after signing (`sha256` mismatch) |
| 10 | truncated payload | the case 1 manifest and signature, with the asset cut short (`size` mismatch, caught before hashing) |
