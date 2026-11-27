"""The e2e-ctl actions the worker suite needs (harness H2 of 09 tests.md), mixed into `ctl_actions.Actions`: a decoder's
processes, the timing settings a service was started with, the worker token rotation of an api restart, the swap of a
decoder for the scripted fake decoder FD, and the input faults of IT-09-7 and IT-09-6 (i): a flipped bit in a stored
blob, a job row whose hash no longer matches its input, and the worker recreated with a variant of its settings."""
import json
import secrets
import sys
import time
import uuid
from pathlib import Path

from ctl_errors import ActionError

DECODERS = frozenset({"decoder-interactive", "decoder-general"})
# `process-kill` restarts of the api: `next` puts a fresh token in `worker_api_token_next` (the api then accepts both);
# `drop-old` makes the next token (or a fresh one) the only one, so a worker still holding the old token gets 403.
TOKEN_STAGES = frozenset({"next", "drop-old"})
WORKER_TOKEN_NEXT_SECRET = "worker_api_token_next"
WORKER_TOKEN_SECRET = "worker_api_token"
# The settings `inspect` may read from a container's environment: the timings a harness profile sets, never a secret.
TIMING_SETTINGS = ("SUNNYCLOUD_JOB_LEASE", "SUNNYCLOUD_WORKER_CLAIM_POLL_INTERVAL", "SUNNYCLOUD_WORKER_STATUS_INTERVAL",
                   "SUNNYCLOUD_DECODER_SILENCE_TIMEOUT", "SUNNYCLOUD_DECODER_CANCEL_GRACE", "SUNNYCLOUD_PHOTO_JOB_TIMEOUT",
                   "SUNNYCLOUD_WORKER_SHUTDOWN_GRACE")
SETTINGS_SERVICES = frozenset({"api", "worker"})
# `scratch-fill`: the volume of each lane, and the owner of what a supervisor that died leaves behind (its uid and the
# decoder group): the sweep at its next start must be able to remove it.
SCRATCH_VOLUMES = {"interactive": "scratch_interactive", "general": "scratch_general"}
LEFTOVER_OWNER = "10002:10003"
MAX_LEFTOVER_TREES = 10
LEFTOVER_SCRIPT = ('set -eu; i=0; while [ "$i" -lt "$1" ]; do d="/s/leftover-$i"; mkdir -p "$d/in" "$d/out"; '
                   'printf x > "$d/in/input"; printf y > "$d/out/result.webp"; i=$((i + 1)); done; printf z > /s/stray.tmp')
# `inspect {"container"}`: the services whose container contents may be listed, and the media tools looked for.
CONTAINER_SERVICES = frozenset({"worker"})
MEDIA_TOOLS = frozenset({"ffmpeg", "ffprobe", "vips", "vipsheader", "vipsthumbnail"})
# The fake decoder (FD) service of each decoder lane: compose.h2.yml, profile `fd`.
FAKE_DECODERS = {"decoder-interactive": "fd-interactive", "decoder-general": "fd-general"}
DECODER_HEALTHY_TIMEOUT_SECONDS = 60
# `bit-flip` (environment.md, Bit flip at rest): a blob's file in the blob volume is `blobs/<id[0:2]>/<id[2:4]>/<id>`
# (04 `path.rs`; the api mounts the volume as its blob root), and chunk 0's first ciphertext byte follows the 64-byte
# header (04 `header.rs`). The reader authenticates chunk 0 before it sends a body byte, so any read of that blob fails
# at once.
BLOB_VOLUME = "blobs"
BLOB_HEADER_LEN = 64
FLIP_SCRIPT = ('set -eu; f="$1"; o="$2"; [ -f "$f" ] || { echo "no blob file $f" >&2; exit 3; }; '
               'byte() { dd if="$f" bs=1 skip="$o" count=1 status=none | od -An -tu1 | tr -d " "; }; '
               'b=$(byte); [ -n "$b" ]; flipped=$(printf %03o $((b ^ 1))); '
               r'printf "\\$flipped" | dd of="$f" bs=1 seek="$o" count=1 conv=notrunc status=none; '
               'printf "%s %s" "$b" "$(byte)"')
# `job-row-edit` (environment.md, Job row edit): the one write it makes, on a queued job only. The id is parsed as a
# UUID before it reaches the statement; no free-form SQL.
JOB_HASH_EDIT = ("UPDATE jobs SET content_hash = set_byte(content_hash, 0, get_byte(content_hash, 0) # 1) "
                 "WHERE id = '{job}' AND status = 'queued' RETURNING id")
# `worker-variant` (environment.md, Worker variant): the override files the worker may be recreated with, each merged
# over the stack's compose files for that one `up`; `default` recreates it as the stack defines it.
WORKER_VARIANTS_DIR = Path(__file__).resolve().parents[2] / "h2-variants"
WORKER_VARIANTS = frozenset({"default", "photo-limit-1mib", "small-scratch-general"})
WORKER_HEALTHY_TIMEOUT_SECONDS = 60
# `stop_timed`: Docker's SIGKILL after SIGTERM comes this late, well past the worker's shutdown grace (H2: 3 s), so a
# drain within the grace shows as exit code 0 and a hung one as 137.
STOP_TIMEOUT_SECONDS = 10
# `_start`: how long a started container gets to show a new start time, checked every `START_POLL_SECONDS`.
START_POLL_SECONDS = 0.2
START_CHECKS = 50


class WorkerSuiteActions:
    """Uses `_checked`, `_compose`, `_container`, `_wait_healthy`, `project` and `secrets_dir` of the class it is mixed
    into."""

    def decoder_processes(self, service):
        """The processes of a decoder container, listed by `docker top` on the host (nothing runs inside the container,
        whose runner watches its own process table): the check that no ffmpeg or vips is left after a job."""
        if service not in DECODERS:
            raise ActionError(400, "bad_request", f"processes is one of {sorted(DECODERS)}")
        listing = self._checked(["docker", "top", self._container(service), "-o", "pid,comm,args"])
        processes = []
        for row in listing.splitlines()[1:]:
            fields = row.split(None, 2)
            if len(fields) < 2 or not fields[0].isdigit():
                raise ActionError(500, "top_failed", f"unexpected docker top row: {row[:200]}")
            processes.append({"pid": int(fields[0]), "comm": fields[1], "args": fields[2] if len(fields) > 2 else ""})
        return {"processes": processes}

    def timing_settings(self, service):
        """The `TIMING_SETTINGS` in the environment the container was created with (`null` when unset). The binary
        refuses to start on an unknown `SUNNYCLOUD_*` name or a value it cannot bind, so a healthy container runs
        with these values."""
        if service not in SETTINGS_SERVICES:
            raise ActionError(400, "bad_request", f"settings is one of {sorted(SETTINGS_SERVICES)}")
        env = json.loads(self._checked(["docker", "inspect", "-f", "{{json .Config.Env}}", self._container(service)]) or "[]")
        values = dict(entry.split("=", 1) for entry in env if "=" in entry)
        return {"settings": {name: values.get(name) for name in TIMING_SETTINGS}}

    def _rotate_worker_token(self, stage):
        """Rewrites the secret files in place: each is bind-mounted into the api as it is, so a new file (a rename)
        would not reach the container. The worker read its token at its start and keeps it until it restarts."""
        current, following = self.secrets_dir / WORKER_TOKEN_SECRET, self.secrets_dir / WORKER_TOKEN_NEXT_SECRET
        if not following.is_file():
            raise ActionError(409, "no_next_token_secret", f"the stack was started without {WORKER_TOKEN_NEXT_SECRET}")
        if stage == "next":
            following.write_text(secrets.token_hex(24), encoding="utf-8")
            return
        promoted = following.read_text(encoding="utf-8").strip() or secrets.token_hex(24)
        current.write_text(promoted, encoding="utf-8")
        following.write_text("", encoding="utf-8")

    def swap_decoder(self, service, op):
        """`fake`: stop the real decoder (SIGTERM) and start a new FD container on its socket and scratch volumes; FD
        reads its script when it starts and appends to its record. `real`: stop FD and start the decoder again, waiting
        until it is healthy. The supervisor sees the runner go and reconnects to whatever listens next."""
        fake = FAKE_DECODERS[service]
        listed = self._checked(self._compose("config", "--services")).split()
        if fake not in listed:
            raise ActionError(409, "no_fake_decoder", f"{fake} is not in this stack: start it with tests/run.sh --profile h2")
        if op == "fake":
            self._checked(self._compose("stop", service))
            self._checked(self._compose("up", "-d", "--no-deps", "--force-recreate", fake))
            return
        self._checked(self._compose("stop", fake))
        self._start(service)
        self._wait_healthy(service, DECODER_HEALTHY_TIMEOUT_SECONDS)

    def stop_timed(self, service):
        """SIGTERM through `compose stop` (SIGKILL after `STOP_TIMEOUT_SECONDS`): how long the stop took and the exit
        code, so a case can tell a drain within the shutdown grace from a kill (IT-09-17)."""
        started = time.monotonic()
        self._checked(self._compose("stop", "-t", str(STOP_TIMEOUT_SECONDS), service))
        seconds = time.monotonic() - started
        code = self._checked(["docker", "inspect", "-f", "{{.State.ExitCode}}",
                              self._container(service, stopped_too=True)]).strip()
        if not code.lstrip("-").isdigit():
            raise ActionError(500, "inspect_failed", f"unexpected exit code: {code[:100]}")
        return {"seconds": round(seconds, 3), "exit_code": int(code)}

    def _start(self, service):
        """`compose start`, then the container must show a new start time. Compose has answered 0 for a worker it did not
        start (3 of about 120 restarts on 2026-11-27, cause not found: the container stayed `exited` with its old start
        time), so the start is checked and tried once more, and the miss is logged. A container that did start and then
        exited is the service's own outcome and is returned as it is; one that already runs needs no start (a lane
        swapped to FD twice is swapped back twice)."""
        before = self._start_state(service)
        if before[0] == "running":
            return
        for attempt in (1, 2):
            self._checked(self._compose("start", service))
            for _ in range(START_CHECKS):
                now = self._start_state(service)
                if now[1] != before[1]:
                    return
                self._sleep(START_POLL_SECONDS)
            print(f"e2e-ctl: compose start left {service} {now[0]} with its old start time {now[1]} (attempt {attempt})",
                  file=sys.stderr, flush=True)
        raise ActionError(500, "start_failed", f"{service} is {now[0]} and was not started by two starts")

    def _start_state(self, service):
        """`(status, start time)` of the service's container, running or not."""
        out = self._checked(["docker", "inspect", "-f", "{{.State.Status}} {{.State.StartedAt}}",
                             self._container(service, stopped_too=True)])
        status, _, started = out.strip().partition(" ")
        return status, started

    def scratch_fill(self, params):
        """`{"lane", "trees"}`: `trees` job trees (`leftover-N/in/input`, `out/result.webp`) and a stray file written into
        the lane's scratch volume by a throwaway container, owned as a supervisor that died leaves them (IT-09-1)."""
        lane, trees = params.get("lane"), params.get("trees", 2)
        if lane not in SCRATCH_VOLUMES or type(trees) is not int or not 1 <= trees <= MAX_LEFTOVER_TREES:
            raise ActionError(400, "bad_request", f"lane is one of {sorted(SCRATCH_VOLUMES)}, trees 1 to {MAX_LEFTOVER_TREES}")
        self._checked(["docker", "run", "--rm", "--network", "none", "--user", LEFTOVER_OWNER,
                       "-v", f"{self.project}_{SCRATCH_VOLUMES[lane]}:/s", "--entrypoint", "sh", self._helper_image(),
                       "-c", LEFTOVER_SCRIPT, "sh", str(trees)])
        return {"lane": lane, "trees": trees, "stray": 1}

    def container_facts(self, service):
        """What the service's container holds (IT-09-1, ST-09-2): its environment variable names (never values), the
        secrets mounted under /run/secrets, every mount destination, and the paths of the media tools in its file
        system (listed from `docker export`, so a tool anywhere in the image is found)."""
        if service not in CONTAINER_SERVICES:
            raise ActionError(400, "bad_request", f"container is one of {sorted(CONTAINER_SERVICES)}")
        container = self._container(service)
        env = json.loads(self._checked(["docker", "inspect", "-f", "{{json .Config.Env}}", container]) or "[]")
        mounts = json.loads(self._checked(["docker", "inspect", "-f", "{{json .Mounts}}", container]) or "[]")
        destinations = sorted(m.get("Destination", "") for m in mounts)
        listing = self._checked(["bash", "-c", 'set -o pipefail; docker export "$1" | tar -t', "bash", container])
        tools = sorted(path for path in listing.split("\n") if path.rstrip("/").rsplit("/", 1)[-1] in MEDIA_TOOLS)
        return {"env_names": sorted(entry.split("=", 1)[0] for entry in env),
                "secrets": sorted(d.rsplit("/", 1)[1] for d in destinations if d.startswith("/run/secrets/")),
                "mounts": destinations, "media_tools": tools}

    def bit_flip(self, params):
        """`{"blob_id"}`: one bit of the first ciphertext byte of the blob's chunk 0 flipped in the blob volume, by a
        throwaway container while the api keeps running; the byte's value before and after."""
        blob = _uuid(params.get("blob_id"), "blob_id")
        path = f"/b/blobs/{blob[0:2]}/{blob[2:4]}/{blob}"
        out = self._checked(["docker", "run", "--rm", "--network", "none", "--user", "0:0",
                             "-v", f"{self.project}_{BLOB_VOLUME}:/b", "--entrypoint", "sh", self._helper_image(),
                             "-c", FLIP_SCRIPT, "sh", path, str(BLOB_HEADER_LEN)])
        values = out.split()
        if len(values) != 2 or not all(v.isdigit() for v in values):
            raise ActionError(500, "flip_failed", f"unexpected flip output: {out.strip()[:200]}")
        return {"blob_id": blob, "offset": BLOB_HEADER_LEN, "before": int(values[0]), "after": int(values[1])}

    def job_row_edit(self, params):
        """`{"job"}`: one bit of a queued job's `content_hash` flipped, so the input the api serves for the job no longer
        has the job's hash; `edited` is 0 when that job is not queued."""
        job = _uuid(params.get("job"), "job")
        out = self._checked(self._compose("exec", "-T", "db", "psql", "-U", "postgres", "-d", "sunnycloud", "-At",
                                          "-c", JOB_HASH_EDIT.format(job=job)))
        return {"job": job, "edited": sum(1 for line in out.splitlines() if line.strip() == job)}

    def worker_variant(self, params):
        """`{"variant"}`: the worker recreated from the stack's compose files plus `tests/h2-variants/<variant>.yml`
        (none for `default`), then healthy. A later `process-kill` restart keeps the variant, so the case that set one
        sets `default` again."""
        variant = params.get("variant")
        if variant not in WORKER_VARIANTS:
            raise ActionError(400, "bad_request", f"variant is one of {sorted(WORKER_VARIANTS)}")
        extra = () if variant == "default" else (str(WORKER_VARIANTS_DIR / f"{variant}.yml"),)
        self._checked(self._compose("up", "-d", "--no-deps", "--no-build", "--force-recreate", "worker",
                                    extra_files=extra))
        self._wait_healthy("worker", WORKER_HEALTHY_TIMEOUT_SECONDS)
        return {"variant": variant}

    def _helper_image(self):
        """The database image, the one throwaway helper containers run (it has a shell; nothing is pulled)."""
        return self._checked(["docker", "inspect", "-f", "{{.Config.Image}}", self._container("db")]).strip()


def _uuid(value, name):
    """`value` in the canonical spelling of a UUID, the only text an id parameter may put into a command."""
    try:
        return str(uuid.UUID(value))
    except (TypeError, ValueError, AttributeError) as error:
        raise ActionError(400, "bad_request", f"{name} is a UUID") from error
