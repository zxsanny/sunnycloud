# 07a_quota — Quota ledger

## 1. High-Level Overview

**Purpose**: keep the per-account storage ledger (limit, used, reserved) that uploads, commits and deletes update atomically, and expose it for admin and self-service views.

**Why a separate component**: the ledger is a low-level module called by 02_tree and 03_transfer inside their transactions. Admin and deletion orchestration (07_admin) sits above 01, 02, 03, 04, 06 and 08. Merged they formed a cycle (02 -> 07 -> 02). 07a depends on `contracts` and helpers only and requires no component port.

**Architectural pattern**: single-row atomic counter per account; stateless.

**Scope in**: `quota` table, reserve/release/adjust/recompute, effective limit (per-user or installation default), the `quota` row lifecycle hook (create at account creation, delete at account erase).
**Scope out**: session cap and free-space floor (03/04 refuse before calling the ledger), derived storage (never counted), billing, organizations, user-facing trash, admin user listing and role changes (07), HTTP (11).

## 2. Ports (declared in `contracts`)

**Provides**
| Port | Methods | Consumers |
|---|---|---|
| `QuotaLedger` | `reserve`, `release_reservation`, `adjust_used` (all take `&mut DbTx`) | 02 (inside the commit transaction), 03 (session start, expiry, abort) |
| `QuotaAdmin` | `view`, `views`, `set_limit`, `recompute(account, live_bytes, reserved_bytes, repair)` | 07, 11 (own quota view), 12 (doctor drift check) |
| `AccountLifecycleHook` | `initialize(tx, account)` inserts the `quota` row; `erase_batch(account, limit) -> EraseProgress{remaining}` deletes it (`remaining = 0`) | 01 (initialize), 07 (erase_batch; registered late by 12, run last) |

**Requires**: none. The caller of `recompute` (12 doctor, 07) takes `live_bytes` from 02's `LiveUsageSource` and `reserved_bytes` from 03's `ReservationSource` and passes them in, so the crate graph has no edge from 07a to 02 or 03.

Helpers: db (`DbTx`, system scope), clock, error. No other component is imported.

## 3. Internal Interfaces

### QuotaLedger (called inside caller transactions)
| Method | Input | Output | Errors |
|---|---|---|---|
| `reserve` | `&mut DbTx`, account, bytes | `Reserved` | `QuotaExceeded{limit, used, reserved}` |
| `release_reservation` | tx, account, bytes | `()` | none (clamped at zero, logged if it would go negative) |
| `adjust_used` | tx, account, delta (signed) | `()` | `InvariantViolation` if the result would be negative |

`reserve` is the single statement `UPDATE quota SET reserved_bytes = reserved_bytes + $n WHERE account_id = $1 AND used_bytes + reserved_bytes + $n <= COALESCE(limit_bytes, $default)`; zero rows means refused, so 100 concurrent 50 GiB sessions cannot exceed the limit. The UPDATE takes its own row lock on the `quota` row and holds it to the end of the caller's transaction; a caller that needs serialisation (03's open-session cap check) runs its check after `reserve` in the same transaction and rolls back on refusal. There is no lock-order contract with the other components.

**Commit**: the commit transaction belongs to 02_tree. Inside it 02 calls `release_reservation(reserved_bytes)` for the session's reserved bytes and `adjust_used(delta)`, then the sync_feed port; 07a never starts a transaction. If the entry later fails inside 02's savepoint, the release rolls back with it; sessions are released exactly once by whoever deletes the session row (03). Counting rule (confirmed): only current revisions of live files; replaced revisions in retention, tombstoned data, thumbnails and transcodes never count.

### QuotaAdmin
| Method | Input | Output | Errors |
|---|---|---|---|
| `view` | account | `QuotaView{limit_bytes?, effective_limit, used, reserved}` | `NotFound` |
| `views` | `[account]` | map; an unknown id is omitted | none |
| `set_limit` | account, `Option<i64>` (None = installation default) | `()` | `NotFound`, `InvalidLimit` (valid: 0 <= limit <= 2^53 bytes; 0 = no new data) |
| `recompute` | account (system scope), `live_bytes`, `reserved_bytes`, `repair: bool` | `Drift{stored, computed}`, repaired when asked | none |

Lowering a limit below current use is allowed: it blocks new reservations and deletes nothing. `recompute` compares the stored `used_bytes` with the `live_bytes` and `reserved_bytes` it is given; it is used only by doctor and tests, never on a request path.

## 4. Data Access Patterns
**Owned**: `quota` (`account_id` PK, `limit_bytes`, `used_bytes`, `reserved_bytes`, CHECK >= 0). No other tables.
| Query | Frequency | Hot | Index |
|---|---|---|---|
| Reserve (conditional UPDATE) | per upload session start | Yes | PK |
| Adjust used | per commit/delete batch | Yes | PK |
| Views for admin list | Low | No | PK batch |
No cache. One row per account. **Rollback**: none; `recompute` repairs drift.

## 5. Implementation Details
**State**: stateless. Reservation lifetime is owned by 03's session and staged rows; expiry and abort call `release_reservation`. At account erase the `quota` row is deleted and open reservations vanish with it.
**Races**: reserve is atomic; used/reserved never negative by CHECK; row-level updates are commutative, and a deadlock between transactions is resolved by the database and retried by the caller (02 retries once). After 1,000 concurrent commits/deletes `used_bytes` equals the recomputed sum and never exceeds the limit (AC-74).
**Libraries**: sqlx 0.9 only. **Errors**: typed enums above.

## 6. Configuration keys
`default_quota_bytes` (operator-set). Durations: none.

## 7. Security notes
Every query is scoped by `account_id`; `set_limit` is reachable only through 07 (`Scope::Admin`) or the operator CLI. A `QuotaExceeded` maps to problem code `quota_exceeded` in 11, with nothing reserved. No PII is stored.

## 8. Dependency graph
Implement after helpers and `contracts`; blocks 02 and 03 integration tests. Parallel: 05, 06.

## 9. ACs, tests, acceptance
**ACs [release]**: AC-39 (quota refusal side; session cap/floor in 03/04) [v1.0]; AC-27 (ledger row erased) [v1.0]. **Interaction AC folded in**: AC-74 [v1.0] (1,000 concurrent commits and deletes keep usage equal to the sum of current live revisions and never above the quota); AC-68 partly (reserved quota released at session expiry).
**Tests**: FT-P-19, FT-P-65, FT-N-40; NFT-RES-05, NFT-RES-06, NFT-RES-LIM-01 (quota side). Unit: reserve race with 100 tasks, clamp on release, negative adjust refused, effective-limit fallback.
**Acceptance (observable)**: 100 sessions declaring 50 GiB beyond the quota are refused with nothing reserved; thumbnails and transcodes do not change `used_bytes`; `recompute` reports drift injected by a test.

## 10. Logging
WARN quota drift found by `recompute`, release that would go negative; INFO limit changes (IDs only, no names). Structured JSON.

## 11. Risks
**Defaults chosen at Step 5 gap pass**: `InvalidLimit` bounds are 0 <= limit <= 2^53 bytes (0 = no new data); `views` with an unknown id omits it.
Cycle risk if the ledger ever imports tree or admin types (enforced by the crate graph: 07a depends on `contracts` and helpers only). Conflict copies consume quota (old content stays counted) and can fill a nearly full account; documented, not blocked.
