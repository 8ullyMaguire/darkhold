# Darkhold private taste body archive (TA2)

The daily body wrapper is `scripts/darkhold-archive-bodies.sh`. It is installed as a user service/timer pair:

- `~/.config/systemd/user/darkhold-archive-bodies.service`
- `~/.config/systemd/user/darkhold-archive-bodies.timer`

The timer fires at 04:15 local time with `Persistent=true`. It runs the same script the operator runs by hand, so there is no second implementation to drift.

The script writes two artefacts under `~/.local/state/darkhold/`:

- `archive-rank.latest` — the ranked candidate report from that run;
- `archive-bodies.latest` — UTC timestamp written after the body enqueue finishes.

Freshness is judged from `archive-bodies.latest`, not from `systemctl is-active`. A stopped timer and a failed step both leave an old artefact.

Manual run:

```bash
systemctl --user start darkhold-archive-bodies.service
journalctl --user -u darkhold-archive-bodies.service -n 20 --no-pager
stat -c '%y' ~/.local/state/darkhold/archive-bodies.latest
```

The body command keeps the existing safety posture: only works with a positive private taste match and no hard exclusion are enqueued; each import carries `body_policy: store`; discovery itself remains metadata-only; no auth, cookies, private works or access-control bypass.
