"""The e2e-ctl actions the worker suite needs (harness H2 of 09 tests.md), mixed into `ctl_actions.Actions`: a decoder's
processes, the timing settings a service was started with, the worker token rotation of an api restart, and the swap of a
decoder for the scripted fake decoder FD."""
import json
import secrets

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
        self._checked(self._compose("start", service))
        self._wait_healthy(service, DECODER_HEALTHY_TIMEOUT_SECONDS)

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

    def _helper_image(self):
        """The database image, the one throwaway helper containers run (it has a shell; nothing is pulled)."""
        return self._checked(["docker", "inspect", "-f", "{{.Config.Image}}", self._container("db")]).strip()

