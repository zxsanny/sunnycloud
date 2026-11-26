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
# The fake decoder (FD) service of each decoder lane: compose.h2.yml, profile `fd`.
FAKE_DECODERS = {"decoder-interactive": "fd-interactive", "decoder-general": "fd-general"}
DECODER_HEALTHY_TIMEOUT_SECONDS = 60


class WorkerSuiteActions:
    """Uses `_checked`, `_compose`, `_container`, `_wait_healthy` and `secrets_dir` of the class it is mixed into."""

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
