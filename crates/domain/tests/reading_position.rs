//! M70-02 — the two frames convert exactly, and a position that cannot be located is refused, not clamped.

use darkhold_domain::reading_position::{
    canonical, from_whole, nearest, switch, to_whole, Chapters, Position,
};

const LENGTHS: [usize; 3] = [100, 250, 40];

fn chapters() -> Chapters<'static> {
    Chapters(&LENGTHS)
}

#[test]
fn every_position_round_trips_through_the_whole_work_frame() {
    for (chapter, length) in LENGTHS.iter().enumerate() {
        for offset in [0, 1, length / 2, *length] {
            let position = Position { chapter, offset };
            let whole = to_whole(chapters(), position).expect("locatable");
            // A boundary has two spellings — (0, 100) and (1, 0) — so the round trip is compared in canonical form.
            assert_eq!(
                from_whole(chapters(), whole),
                canonical(chapters(), position),
                "{position:?}"
            );
            // The place itself survives: same whole offset, and canonical form is idempotent.
            let back = switch(chapters(), position).expect("round trip");
            assert_eq!(to_whole(chapters(), back), Some(whole), "{position:?}");
            assert_eq!(canonical(chapters(), back), Some(back));
        }
    }
}

#[test]
fn the_whole_work_frame_is_cumulative_and_the_cursor_after_the_last_unit_is_a_place() {
    assert_eq!(
        to_whole(
            chapters(),
            Position {
                chapter: 0,
                offset: 0
            }
        ),
        Some(0)
    );
    assert_eq!(
        to_whole(
            chapters(),
            Position {
                chapter: 1,
                offset: 10
            }
        ),
        Some(110)
    );
    assert_eq!(
        to_whole(
            chapters(),
            Position {
                chapter: 2,
                offset: 40
            }
        ),
        Some(390)
    );
    assert_eq!(chapters().total(), 390);
    // 390 is where the cursor sits after the last character; it belongs to the end of chapter 3, not outside.
    assert_eq!(
        from_whole(chapters(), 390),
        Some(Position {
            chapter: 2,
            offset: 40
        })
    );
    // 391 does not.
    assert_eq!(from_whole(chapters(), 391), None);
}

#[test]
fn a_boundary_is_one_place_with_two_spellings_and_the_earlier_chapter_wins() {
    let end_of_first = Position {
        chapter: 0,
        offset: 100,
    };
    let start_of_second = Position {
        chapter: 1,
        offset: 0,
    };
    assert_eq!(
        to_whole(chapters(), end_of_first),
        to_whole(chapters(), start_of_second)
    );
    assert_eq!(canonical(chapters(), start_of_second), Some(end_of_first));
    assert_eq!(
        canonical(chapters(), end_of_first),
        Some(end_of_first),
        "already canonical"
    );
}

#[test]
fn a_chapter_that_no_longer_exists_is_refused_rather_than_moved() {
    // The author deleted chapter 3: the stored position names a chapter past the end.
    assert_eq!(
        to_whole(
            chapters(),
            Position {
                chapter: 3,
                offset: 0
            }
        ),
        None
    );
    // And an offset past the end of an existing chapter is not silently the end of it.
    assert_eq!(
        to_whole(
            chapters(),
            Position {
                chapter: 0,
                offset: 101
            }
        ),
        None
    );
    // Moving is a separate decision, available but never automatic.
    assert_eq!(
        nearest(
            chapters(),
            Position {
                chapter: 9,
                offset: 7
            }
        ),
        Some(Position {
            chapter: 2,
            offset: 7
        })
    );
    assert_eq!(
        nearest(
            chapters(),
            Position {
                chapter: 0,
                offset: 900
            }
        ),
        Some(Position {
            chapter: 0,
            offset: 100
        })
    );
}

#[test]
fn a_work_with_no_chapters_has_no_positions() {
    let empty = Chapters(&[]);
    assert!(empty.is_empty());
    assert_eq!(from_whole(empty, 0), None);
    assert_eq!(
        nearest(
            empty,
            Position {
                chapter: 0,
                offset: 0
            }
        ),
        None
    );
    assert_eq!(
        to_whole(
            empty,
            Position {
                chapter: 0,
                offset: 0
            }
        ),
        None
    );
}
