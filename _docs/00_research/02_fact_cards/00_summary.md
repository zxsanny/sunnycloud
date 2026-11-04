# Fact Cards (split form)

Fact cards live in each category report under `## Fact cards` (statement, source number, confidence). Per-mode API verification blocks (MVE) are in the files below.

| Category | Fact cards | API verification (MVE) |
|----------|-----------|------------------------|
| Storage and DB | [storage_db](../raw/storage_db.md) | [mve_server_media](mve_server_media.md) (sqlx, job queue) |
| Sync and transfer | [sync_protocol](../raw/sync_protocol.md) | [mve_server_media](mve_server_media.md) (axum, hyper, blake3) |
| Media and mobile | [media_pipeline](../raw/media_pipeline.md) | [mve_server_media](mve_server_media.md) (libvips, ffmpeg, sandbox), [mve_crypto_auth_mobile](mve_crypto_auth_mobile.md) |
| Security, crypto, auth | [security_clients](../raw/security_clients.md) | [mve_crypto_auth_mobile](mve_crypto_auth_mobile.md) |
| Desktop client | [sync_protocol](../raw/sync_protocol.md), [security_clients](../raw/security_clients.md) | [mve_desktop](mve_desktop.md) |

Confidence marks: High = official documentation or own measurement; Med = engineering blog or secondary; Low = estimate or [limited source].
