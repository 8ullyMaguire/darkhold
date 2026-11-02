# 37. Multi-Platform Companion Bot

**Status:** reserved
**Milestone:** —
**Source:** §37 of `docs/spec.md`

> **2026-09-20 addition.** Darkhold's companion bot is a thin client of
> the public API (§23.1) that lets readers search, browse, download, and
> interact with works from Discord, Telegram, Matrix, IRC, the Fediverse
> (Mastodon, Bluesky), and the terminal. The bot never touches the
> database; it speaks the same REST API as every other client.

## 37.0 Architecture

The bot is a **separate workspace** (`darkhold-bot`) with a platform-neutral
core driving thin adapters:

```
darkhold-bot/
├── crates/
│   ├── bot-core/          # typed API client, PlatformMessage IR, dispatch, intent, store, cache
│   ├── darkhold-bot-discord/     # Poise + Serenity adapter
│   ├── darkhold-bot-telegram/    # Teloxide adapter
│   ├── darkhold-bot-matrix/      # matrix-sdk adapter
│   ├── darkhold-bot-irc/         # irc crate adapter
│   ├── darkhold-bot-fediverse/   # Mastodon + Bluesky (raw XRPC)
│   └── darkhold-bot-cli/         # REPL + TUI (ratatui) + download tool
```

**Platform-neutral core.** `bot-core` exports:
- `DarkholdClient` — typed HTTP client for every public API endpoint
- `PlatformMessage` — intermediate representation with `RichItem` + `ActionRow`
- `do_*` functions — one implementation of each command (search, recs, download, ...)
- `intent` — free-form mention classification (optional LLM, heuristic fallback)
- `store` — token store (`/link` flow) keyed by platform user id
- `cache` — Redis pagination + response cache
- `ratelimit` — cross-platform rate limiter

**Adapters.** Each adapter renders `PlatformMessage` natively and wires
platform events to the shared `do_*` functions. No adapter contains
business logic.

**Why a separate workspace.** The bot brings its own dependencies (Poise,
Teloxide, matrix-sdk, irc, ratatui) that Darkhold's server does not need.
Deploying it independently means a bot restart does not restart the server.

## 37.1 Shared Infrastructure

**API client.** `DarkholdClient` wraps `reqwest::Client` and exposes one
method per public endpoint (§23.1): `search`, `recommendations`, `work`,
`work_download`, `work_bookmark`, `work_kudos`, `forum_categories`,
`forum_topics`, `forum_topic`, `forum_post_create`, `fandoms`, `fandom`,
`tags`, `random`, `trending`, `similar`, `also_bookmarked`, `blind_date`,
`comments`, `reading_status`, `user`, `user_works`, `user_bookmarks`,
`notifications`, `me`, `link_token`, `link`, `unlink`. Every method returns
a typed model; errors are `BotError`.

**Token store.** OAuth-style link flow: a reader runs `/link`, the bot
stores a pending code, the reader authorizes on the Darkhold site, the bot
exchanges the code for a token. Tokens are stored in Redis keyed by
`bot:{platform}:{user_id}`. The store also holds per-user preferences
(filter chips, recs tuners, track state).

**Pagination cache.** Search and list responses are cached in Redis with a
short TTL. A pagination session pointer (`bot:page:{user_id}`) tracks the
current page so `next`/`prev` work without re-fetching.

**Rate limiting.** Cross-platform rate limiter using Redis buckets. A user
who spams commands across Discord and Telegram shares one bucket. Limits
are configurable per-platform.

**Intent classification.** When a user @mentions the bot with free text
(not a slash command), `intent::classify` parses it. With
`LOHAVEN_BOT_LLM_ENABLED=0` (default), a heuristic parser handles URLs,
questions, and keywords. With the flag on, a local LLM (Ollama) classifies
against a fixed `Intent` enum — prompt injection can only pick from the
closed action set.

## 37.2 Commands

Every adapter exposes the same command surface. The canonical command list:

| Command | Description | Auth |
|---------|-------------|------|
| `/search <query>` | Search works with optional filters | no |
| `/ask <question>` | Natural-language archive question | no |
| `/recs` | Personalized recommendations | yes |
| `/fresh` | Recently active works | no |
| `/gems` | Hidden-gem recommendations | no |
| `/roll` | Random work from recs | yes |
| `/download <url>` | Download links (EPUB/PDF/MOBI) | no |
| `/metadata <url>` | Metadata card for a URL | no |
| `/bookmark <url>` | Bookmark a work | yes |
| `/kudos <url>` | Leave kudos | yes |
| `/work <id>` | Work detail card | no |
| `/fandoms` | List all fandoms | no |
| `/fandom <slug>` | Fandom detail | no |
| `/forum cats` | Forum categories | no |
| `/forum topics <slug>` | Topics in a category | no |
| `/forum show <id>` | Thread detail | no |
| `/forum reply <id> <text>` | Reply to a topic | yes |
| `/forum follow <id>` | Subscribe to a topic | yes |
| `/forum mark-read <id>` | Mark topic read | yes |
| `/forum search <query>` | Forum full-text search | no |
| `/blind-date` | Random work, hidden metadata | no |
| `/trending` | Trending works | no |
| `/similar <id>` | Similar works | no |
| `/also-bookmarked <id>` | Readers also bookmarked | no |
| `/comments <url>` | Work comments | no |
| `/random` | Random work | no |
| `/help [topic]` | Help text | no |
| `/link` | Link Darkhold account | no |
| `/unlink` | Unlink account | yes |
| `/me` | Linked account info | yes |

## 37.3 Discord Adapter

Built on Poise + Serenity. Slash commands with autocomplete for fandoms and
tags. Embeds for search results, metadata, and recommendations. Buttons
for pagination (`next`/`prev`) and actions (`download`, `bookmark`,
`kudos`). Free-form @mentions trigger intent classification.

**Guild configuration.** Server admins set default fandom filters, NSFW
channel restrictions, and command permissions via `/guild config`.

**OAuth.** `/link` generates a one-time code; the user authorizes on the
Darkhold site; the bot stores the token.

## 37.4 Telegram Adapter

Built on Teloxide. Slash commands with the same surface as Discord.
Inline mode: type `@darkholdbot <query>` in any chat for instant results.
Callback buttons for pagination and actions. Channel integration: admins
point a channel at a fandom or author feed for auto-posts.

**Instant View.** Works shared via the bot use Telegram's Instant View
renderer when available.

## 37.5 Matrix Adapter

Built on matrix-sdk. Logs into a homeserver, joins configured rooms, listens
for room messages. Renders `PlatformMessage` as HTML `m.room.message`.
Supports both slash commands and free-form @mentions.

## 37.6 IRC Adapter

Built on the `irc` crate. Connects to an IRC server, joins channels,
responds to `!command` prefix and free-form text. Renders as plain text
with numbered actions (IRC has no buttons).

## 37.7 Fediverse Adapter

**Mastodon.** Polls `/api/v1/notifications` for mentions. Replies to
mentions with rendered `PlatformMessage`. Posts new works to a configured
Mastodon account (one per work, with tags and quote card image). Supports
Misskey, Akkoma, Pleroma, GoToSocial via the same Mastodon-compatible API.

**Bluesky.** Polls `app.bsky.notification.listNotifications` via raw XRPC
(no atrium dependency). Replies are posted with
`com.atproto.repo.createRecord` on `app.bsky.feed.post`, threading into
the original post. Files are not supported from a bot app-password;
`PlatformMessage::File` degrades to the download link.

**Piefed/Lemmy.** Poll-based community monitor using the Lemmy API. Posts
new works matching configured filters to a community.

## 37.8 CLI/TUI Adapter

**REPL.** `darkhold-bot repl` runs a read-evaluate-print loop. Line grammar:
- `quit`/`exit` — end session
- `!cmd args` — command dispatch
- `page N`/`next`/`prev`/`first`/`last` — paginate last list
- bare fanfic URL — metadata card
- `1`..`9` — pick numbered action from last reply
- any other text — intent classification

**TUI.** `darkhold-bot tui` runs a three-pane terminal UI (ratatui):
left pane with Search/Forum tabs, right pane with metadata or threads,
bottom status bar + input line. Keybindings: `↑/↓` move cursor, `Enter`
open, `Tab` switch pane, `h/l` switch tab, `/` focus input, `q` quit,
`r` refresh, `d` download, `b` bookmark, `f` follow, `R` reply.

**Download tool.** `darkhold-bot download <url>` fetches a work in the
specified format and writes it to disk. Pipe-friendly: `darkhold-bot
search --json | jq '.results[0].url' | darkhold-bot download --format epub`.

## 37.9 Webhooks

The bot can receive webhooks from Darkhold (§36.14) and forward them to
configured platform channels. A new-work event in a fandom can be posted
to a Discord channel, a Telegram group, a Matrix room, or an IRC channel.

## 37.10 Configuration

Environment variables (all prefixed `LOHAVEN_BOT_`):

| Variable | Default | Description |
|----------|---------|-------------|
| `LOHAVEN_BOT_API_URL` | `https://darkhold.polarisocial.xyz` | Darkhold instance URL |
| `LOHAVEN_BOT_REDIS_URL` | `redis://127.0.0.1:6379` | Redis connection |
| `LOHAVEN_BOT_DISCORD_TOKEN` | — | Discord bot token |
| `LOHAVEN_BOT_TELEGRAM_TOKEN` | — | Telegram bot token |
| `LOHAVEN_BOT_MATRIX_HOMESERVER` | — | Matrix homeserver URL |
| `LOHAVEN_BOT_MATRIX_USER` | — | Matrix user ID |
| `LOHAVEN_BOT_MATRIX_PASSWORD` | — | Matrix password |
| `LOHAVEN_BOT_IRC_SERVER` | — | IRC server host |
| `LOHAVEN_BOT_IRC_NICK` | `darkhold-bot` | IRC nick |
| `LOHAVEN_BOT_MASTODON_INSTANCE` | — | Mastodon instance |
| `LOHAVEN_BOT_MASTODON_TOKEN` | — | Mastodon access token |
| `LOHAVEN_BOT_BSKY_HANDLE` | — | Bluesky handle |
| `LOHAVEN_BOT_BSKY_PASSWORD` | — | Bluesky app password |
| `LOHAVEN_BOT_LLM_ENABLED` | `0` | Enable Ollama intent classification |
| `LOHAVEN_BOT_LLM_URL` | `http://localhost:11434` | Ollama URL |
| `LOHAVEN_BOT_LLM_MODEL` | `llama3.2` | Ollama model |
| `LOHAVEN_BOT_RATE_LIMIT_PER_MINUTE` | `10` | Commands per user per minute |
| `LOHAVEN_BOT_CACHE_TTL` | `300` | Response cache TTL (seconds) |

## 37.11 Acceptance

- A user can search Darkhold from Discord, Telegram, Matrix, IRC, and the terminal.
- A user can link their Darkhold account via `/link` and use authed commands.
- A user who @mentions the bot with a fanfic URL gets a metadata card.
- A user who @mentions the bot with a natural-language question gets an answer.
- Pagination (`next`/`prev`) works across all adapters.
- A Mastodon mention of the bot triggers a reply with search results.
- A Bluesky mention of the bot triggers a reply with search results.
- The TUI runs in a terminal and renders search results, metadata, and forum threads.
- The CLI download tool fetches a work in the specified format.
- A webhook from Darkhold is forwarded to a configured Discord channel.
- A user who sends 11 commands in a minute is rate-limited on the 11th.
- The bot shares one rate limit bucket across Discord and Telegram for the same user.

## 37.12 What this section deliberately does not do

- **No server coupling.** The bot is a separate workspace with its own
  dependencies. It talks to Darkhold's public API, not its database.
- **No AI-generated content.** Intent classification is optional and
  heuristic by default. The LLM can only pick from a closed action set.
- **No paywalled features.** Every command available in the bot is also
  available in the web interface. The bot is a convenience, not a premium
  tier.
- **No platform lock-in.** A reader who uses the bot on Discord can switch
  to Telegram without losing their linked account or preferences.


---
