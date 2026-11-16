#!/usr/bin/env python3
"""Tests for the backlog join (§1 check 2). Run with `python3 scripts/backlog_test.py`.

The join is only useful if it can fail: a card that disappears from both the requirement map and the deferral
file must stay visible as unlinked, and a stale/duplicate deferral must be an error instead of a silent count.
"""

from __future__ import annotations

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
import backlog  # noqa: E402


def test_every_card_is_linked_or_deferred() -> None:
    data = backlog.build()
    sources = data["sources"]["concord"]
    cards = sources["work_cards"]
    assert (
        sources["linked_to_a_requirement"] + len(data["deferred_concord"]) + sources["unlinked"] == cards
    ), "the three buckets must partition the work cards"
    assert sources["unlinked"] == 0, (
        "a Concord card is unaccounted for: " + ", ".join(c["id"] for c in data["unlinked_concord"])
    )
    assert len(data["deferred_concord"]) == 50, "the 2026-11-16 scope decision covers 50 cards"
    assert all(card["decision"] and card["reason"] for card in data["deferred_concord"])


def test_deferral_file_refuses_stale_ids_and_duplicates(tmp: pathlib.Path) -> None:
    concord = [{"id": "1", "title": "a", "status": "unknown"}]
    original = backlog.DEFERRED
    try:
        backlog.DEFERRED = tmp / "stale.tsv"
        backlog.DEFERRED.write_text("card\tdecision\treason\n999\tdeferred\tgone\n", encoding="utf-8")
        try:
            backlog.read_deferred(concord)
        except ValueError as error:
            assert "not in the Concord snapshot" in str(error)
        else:
            raise AssertionError("a card absent from the snapshot was accepted")

        backlog.DEFERRED = tmp / "duplicate.tsv"
        backlog.DEFERRED.write_text(
            "card\tdecision\treason\n1\tdeferred\tfirst\n1\tdeferred\tsecond\n", encoding="utf-8"
        )
        try:
            backlog.read_deferred(concord)
        except ValueError as error:
            assert "deferred twice" in str(error)
        else:
            raise AssertionError("a duplicate deferral was accepted")

        backlog.DEFERRED = tmp / "blank.tsv"
        backlog.DEFERRED.write_text("card\tdecision\treason\n1\t\t\n", encoding="utf-8")
        try:
            backlog.read_deferred(concord)
        except ValueError as error:
            assert "needs a decision and a reason" in str(error)
        else:
            raise AssertionError("a blank reason was accepted")
    finally:
        backlog.DEFERRED = original


if __name__ == "__main__":
    import tempfile

    test_every_card_is_linked_or_deferred()
    with tempfile.TemporaryDirectory(prefix="darkhold-backlog-test-") as folder:
        test_deferral_file_refuses_stale_ids_and_duplicates(pathlib.Path(folder))
    print("backlog_test: 3/3 passed")
