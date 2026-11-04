#!/usr/bin/env python3
"""arena_live.py — one real arena vote through darkhold's own HTTP surface, and the service must see it.

The unit tests prove the client builds the right headers; the service's own probe proves the service accepts
them. Neither proves the deployed binary uses either. This does, without a browser:

  1. seed a session row for an existing account (the same row the sign-in flow writes: a token and a CSRF
     token, both stored as SHA-256 hashes — `crypto::hash_token`);
  2. read two real published work ids from the database;
  3. ask the service how many ballots the subject has cast;
  4. POST `/api/v1/arena/vote` with the session and CSRF cookies, exactly as the page does;
  5. ask the service again — a ballot must have arrived, with the reader's own subject;
  6. delete the seeded session.

Nothing secret is printed. The database URL is read from the service's environment, never echoed.

    python3 scripts/arena_live.py [--base https://darkhold.polarisocial.xyz]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import secrets
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid

UNIT = "darkhold.service"


def env_from_unit(name: str) -> str:
    out = subprocess.run(
        ["systemctl", "--user", "show", UNIT, "-p", "Environment"],
        capture_output=True, text=True, check=True,
    ).stdout
    for pair in out.split():
        if pair.startswith(f"{name}="):
            return pair.split("=", 1)[1]
    raise SystemExit(f"arena_live: {name} is not in {UNIT}")


def psql(dsn: str, sql: str) -> list[list[str]]:
    out = subprocess.run(
        ["psql", dsn, "-tAF", "\t", "-c", sql],
        capture_output=True, text=True, check=True,
    ).stdout
    return [line.split("\t") for line in out.strip().splitlines() if line]


def hash_token(value: str) -> str:
    return hashlib.sha256(value.encode()).hexdigest()


def http(method: str, url: str, body: dict | None, headers: dict) -> tuple[int, str]:
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(url, data=data, method=method)
    request.add_header("User-Agent", "estate-arena-live/1.0")
    if data is not None:
        request.add_header("Content-Type", "application/json")
    for key, value in headers.items():
        request.add_header(key, value)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.status, response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", default="http://127.0.0.1:8081")
    parser.add_argument("--taste", default="http://127.0.0.1:8077")
    parser.add_argument("--config", default=None)
    args = parser.parse_args()

    # The database is configured in the instance's own config file, which the unit names in DARKHOLD_CONFIG.
    config_path = args.config or env_from_unit("DARKHOLD_CONFIG")
    dsn = None
    for line in open(config_path):
        stripped = line.strip()
        if stripped.startswith("url") and "=" in stripped and dsn is None:
            dsn = stripped.split("=", 1)[1].strip().strip('"')
    if not dsn:
        raise SystemExit("arena_live: no database url in the instance config")

    accounts = psql(dsn, "SELECT CAST(id AS TEXT) FROM accounts ORDER BY created_at LIMIT 1")
    if not accounts:
        raise SystemExit("arena_live: no account to act as")
    account_id = accounts[0][0]

    works = psql(
        dsn,
        "SELECT CAST(id AS TEXT) FROM works "
        "WHERE lifecycle = 'published' AND visibility = 'public' "
        "ORDER BY random() LIMIT 4",
    )
    if len(works) < 4:
        raise SystemExit(f"arena_live: only {len(works)} published works — nothing to compare")
    card_ids = [row[0] for row in works]

    session_id = str(uuid.uuid4())
    session_token = secrets.token_urlsafe(32)
    csrf_token = secrets.token_urlsafe(32)
    now = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    expires = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() + 600))
    psql(
        dsn,
        "INSERT INTO sessions (id, token_hash, account_id, csrf_token_hash, user_agent, "
        "created_at, last_seen_at, expires_at) VALUES "
        f"('{session_id}', '{hash_token(session_token)}', '{account_id}', '{hash_token(csrf_token)}', "
        f"'estate-arena-live', '{now}', '{now}', '{expires}')",
    )
    print(f"seeded a session for account {account_id[:8]}… over 4 works ({len(card_ids)} cards)")

    before = 0
    status, body = http("GET", f"{args.taste}/api/v1/taste/ratings?site=darkhold&kind=work&limit=200", None, {})
    rated_before = json.loads(body)["data"] if status == 200 else []
    print(f"the service holds {len(rated_before)} rated works before the vote")

    cookies = f"darkhold_session={session_token}; darkhold_csrf={csrf_token}"
    vote_status, body = http(
        "POST",
        f"{args.base}/api/v1/arena/vote",
        {
            "best_work_id": card_ids[0],
            "worst_work_id": card_ids[-1],
            "reason_tags": [],
            "card_ids": card_ids,
        },
        {"Cookie": cookies, "x-csrf-token": csrf_token},
    )
    print(f"POST /api/v1/arena/vote → {vote_status} {body[:120]}")

    time.sleep(2)
    status, body = http("GET", f"{args.taste}/api/v1/taste/ratings?site=darkhold&kind=work&limit=200", None, {})
    rated_after = json.loads(body)["data"] if status == 200 else []
    ids_after = {row["item_id"]: row for row in rated_after}
    ids_before = {row["item_id"] for row in rated_before}
    appeared = 0
    moved = 0
    for item_id, row in ids_after.items():
        work = item_id.removeprefix("work:")
        if work not in card_ids:
            continue
        if item_id not in ids_before:
            appeared += 1
        elif row["matches"] > 0:
            moved += 1
    print(f"the service now holds {len(rated_after)} rated works, {appeared} from these cards")

    psql(dsn, f"DELETE FROM sessions WHERE id = '{session_id}'")
    print("the seeded session is deleted")

    failures = []
    if vote_status != 200:
        failures.append(f"the vote did not reach the route ({vote_status})")
    if appeared == 0:
        failures.append("the service saw no rating for the cards that were voted on")
    print()
    print(f"CHECK arena-live: {'ok' if not failures else '; '.join(failures)}")
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
