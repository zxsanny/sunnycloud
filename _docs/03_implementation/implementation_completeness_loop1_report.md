# Product Implementation Completeness Gate, loop 1

**Date**: 2026-11-24. Method: four read-only audits of the 122 specs still in `todo/` (spec Outcome and ACs against code, tests, batch reports and leftovers); the parent spot-checked the cited files of every DONE row. 123 specs were in `todo/` at the start; AZ-3795 was archived in batch 113.

## Result

| Class | Count | Tasks |
|-------|-------|-------|
| DONE, archived now | 4 | AZ-3855, AZ-3857, AZ-3946, AZ-4024 |
| PARTIAL, Linux-buildable remainder | 20 | AZ-3935, 3936, 3937, 3938, 3939, 3940, 3945, 3947, 3950, 3956, 3671, 3654, 3858, 3865, 3996, 4007, 3928, 3668, 3813, 3772 |
| PARTIAL or NOT STARTED, need the E-31 compose test stack (api, worker, decoder, Caddy, mock IdP in the stack) | 15 | AZ-3661, 3662, 3664, 3665, 3815, 3835, 3836, 3847, 3848, 3878, 3879, 3971, 4021, 4023 (edge checks), 3772 (AT legs) |
| NOT STARTED, Linux-buildable, not MVP (roadmap section 7 items 5) | about 45 | desktop core (AZ-3669, 3670, 3894-3912, 3915, 3917, 3918), Android core (AZ-3672, 3673, 3992-3995, 3998, 3999, 4001, 4019), vault core (AZ-4008-4017, 4025-4027, 3954, 3948, 3675) |
| BLOCKED_EXTERNAL | 28 | see below |

No spec has a stub, passthrough or fake standing in for a promised capability. Every open item is missing work or a missing prerequisite, not scaffolding. PASS for what is built; the gate does not pass as "all product tasks complete".

## BLOCKED_EXTERNAL (needs the owner or a device)

- Windows or macOS machine, screen readers, reference hosts: AZ-3881, 3882, 3883, 3884, 3886, 3893, 3919, 3920, 3921.
- Mac with Xcode, iPhone: AZ-3674, 3975-3984 (iOS app; roadmap section 7 item 3).
- Android emulator with KVM and Appium, OEM phones: AZ-3987, 3988, 4003, 4005.
- Reference hosts REF-SRV, REF-LOAD, REF-CPU4, a 1 Gbps link, root for `tc`: AZ-3799, 3951, 3972, 3952, 3866, 3849, 3850.
- NVENC GPU: AZ-3862. iPhone and Mac Safari: AZ-3932.
- External vault review, no reviewer yet: AZ-4018.
- Owner-supplied phone files (DS-03/05/06/08/35): AZ-3865 NOT RUN list, AZ-3849, AZ-3850.

## Found while auditing (to fix in place)

- AZ-3945 and AZ-3946 specs are stale: polling replaced by pushed frames (roadmap D5), "Revoked" status dropped by owner decision 2026-11-22. Amend the specs.
- AZ-3654: `serve` never issues the setup token (AC-5); `admin setup-token` works.
- AZ-3858: resolved in batch 126. The 50 ms / 100 ms probe bounds are asserted on the decoder container (IT-10-4, medians 24-28 ms); the host-process overhead was the guard's whole-`/proc` reads, not load.
- AZ-4024: tested on an in-process mock; token revocation at account deletion is not built.
- AZ-3937: the account page lacks the email "Verified" mark and last sign-in per provider (server fields missing).

## Next

The single largest unlock is the E-31 compose test stack: 15 tasks above wait on it, and the leftovers of AZ-3835, 3990, 4000 and 3941 name it. Then the Linux-buildable web partials, then Android, desktop and vault cores.
