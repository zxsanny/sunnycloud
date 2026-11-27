---
title: `docker compose start` can answer 0 and leave the container exited; check its start time
applies_when: e2e-ctl `process-kill` start, a decoder swap back (`real`), or any harness step that runs `compose start` after a kill
source: 2026-11-27, batch 133 (AZ-4387), H2 worker suite: IT-09-1, IT-09-5 and IT-09-11 teardowns
---

After `docker compose kill -s KILL worker` and `docker compose start worker` (both exit 0, Compose 2.40.3), the worker
was still `exited` with its old `State.StartedAt`, and stayed so until the next golden reset started it: the case's
teardown waited 90 s for `/readyz` and failed, and the next case found "service worker has no container". It happened
3 times in about 120 restarts across three suite runs, and not once in 22 deliberate repeats of the same sequence; the
cause was not found (the container log showed a normal run up to the kill; Docker's event buffer had rolled over).

e2e-ctl now checks every `start` (`ctl_worker.py` `_start`): the container must show a new start time within 10 s;
if not, it logs `e2e-ctl: compose start left <service> ... (attempt 1)` to `e2e-results/e2e-ctl.log` and starts once
more, and a second miss is the typed error `start_failed`. A container that did start and then exited is not
restarted: that is the service's own outcome. A container that already runs is left alone, since `compose start` does
nothing to it and its start time stays (a lane swapped to FD twice is swapped back twice). When the log line shows up, record the time and what else ran on the
Docker daemon then (other projects share it). The `lanes` teardown of the worker suite puts the worker's container
state and log tail in its failure message (`suites/worker/conftest.py` `worker_state`).
