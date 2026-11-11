# follow / syndicate / notify — adoption

Measured shape: **three systems**, the worst in the family. Four follow tables (`subscriptions`, `saved_view_subscriptions`, `shelf_follows`,
`wip_watches`) and six notification tables (`notifications`, `notification_routes`, `outbox_events`, `push_subscriptions`, `search_alerts`,
`taste_notification_queue`) for one concept.

Subject types already exist: shelf, collection, tag, author, series, saved view.

Merge: the four follow tables into `follow`; the six notification tables into `follow_delivery` channels, keeping `outbox_events` as the **transport**
(it delivers, it does not hold the preference). Delete `saved_view_subscriptions`, `shelf_follows`, `wip_watches`, `search_alerts`,
`taste_notification_queue`, `notification_routes`.

Why darkhold first: it is deployed, it is the largest, and every other Rust member will copy whatever this one does.

The rule (ADR 0049, estate): **follow, syndicate and notify are one object with three verbs, not three systems.**

    follow(id, actor, subject_type, subject_id, created_at, position)
    follow_delivery(follow_id, channel, target, enabled, last_delivered_at, watermark, cursor)

    channels = rss | atom | activitypub | email | webpush | digest | webhook

Rules that apply here: the follow row is the only subscription model (a new subject is a *subject type*, never a table); every follow is syndicated from an
existing `<subject>/feed.xml` that is the same query as the page; notification is a channel on the follow, not a separate setting; the watermark is an **id**
not a timestamp (imports arrive with old `posted_at`); one page shows follow + feed URL + channels; a private feed URL is an opaque revocable token held by
`follow_delivery`.

Gate (run it before claiming the member conforms):

    # no parallel notification system outside the follow object
    grep -rIl "create table .*notification" migrations/ | wc -l          # 0
    # every followable subject answers a feed, and the ids equal the page's
    curl -s <subject>/feed.xml | grep -c "<entry>"                        # > 0
    # switching a channel off keeps the follow; deleting the follow removes every channel
