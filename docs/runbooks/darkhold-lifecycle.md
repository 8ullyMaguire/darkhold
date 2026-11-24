# Lifecycle sweep runbook (M45-76)

The timer must be installed **disabled**. It is deliberately not enabled by the
deploy: the first automated write needs a human to read a dry run and one small
manual batch first.

## Files

- `deploy/darkhold-lifecycle.service` — user unit, mirrored from
  `darkhold-archive-bodies.service`. It calls the `dh` CLI at
  `/home/alvaro/.cargo-target/darkhold/release/dh`, the path the deploy host
  actually has. The task file does not expand `~`, so the path is absolute.
- `deploy/darkhold-lifecycle.timer` — daily at 04:45, persistent.

The environment file is `/personal/secrets/estate/darkhold.env`, the same private
file the archive unit reads. Its contents are never committed or printed.

## Install without enabling

On thinkcentre:

```bash
cp deploy/darkhold-lifecycle.service deploy/darkhold-lifecycle.timer ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user is-enabled darkhold-lifecycle.timer   # expect: disabled
systemctl --user list-timers darkhold-lifecycle.timer  # expect: no scheduled job
```

## Before enabling

1. Rebuild and deploy the release binaries (H3). The current deployed binary
   predates the sweep route, the CLI and the provenance columns.
2. Run a bounded dry run with an operator session supplied through the normal
   `DARKHOLD_COOKIE` path (never pasted into a command in chat):

   ```bash
   /home/alvaro/.cargo-target/darkhold/release/dh lifecycle sweep --limit 10
   ```

   Read `scanned`, `abandoned`, `revived`, `concluded`, `raced` and `unmeasured`.
   The plan's pre-deploy estimate was 9,722 eligible and 30,570 unmeasured;
   that is a prediction, not evidence, until this command prints the real report.
3. Run one bounded write by hand with `--apply --limit 10` and read the marks.
4. Only then enable: `systemctl --user enable --now darkhold-lifecycle.timer`.

## Blockers recorded, not waited on

- H3 rebuild/deploy: needs a quiet deploy host, no build or tagger workers, and
  swap use below 2 GB.
- H1 swap cleanup: the temporary `/swapfile-build*` entries are still enabled.
- Operator session: needed for the dry run; no credential is stored in this repo.
