# Taste declaration fragment

The `[taste]` block is policy, not host trivia (ADR 0062 in the estate repo). It lives here, in tracked
`config/taste.toml`, so a change to who may set this instance's taste is reviewable in a diff.

The deployed `darkhold.toml` remains ignored because it carries local values and secrets. After changing the
fragment, render it on the host and prove the two agree:

```bash
python3 scripts/render-taste-config.py config/taste.toml darkhold.toml
bash scripts/check-taste-fragment.sh
```

The check compares parsed TOML values, not comments, and names the differing key. A source weight mutation was
proved to fail as `taste.source[1].weight 0.61 != 0.6` and restore green before this file was added. Restart
`darkhold.service` after rendering so the running process reads the new declaration.
