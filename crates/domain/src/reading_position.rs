//! M70-02 — reading a multi-chapter work as one document, without losing the reader's place.
//!
//! Whole-work mode is not concatenation. A reader at 40% of a 300k-word work who switches to per-chapter view has
//! to land in the chapter they were in, at the paragraph they were reading — so the position has to be expressible
//! in both frames and convert exactly. A byte offset into a merged document is the tempting shortcut and it is
//! wrong: notes, chapter headings and front matter differ between the two renderings, so the same byte is a
//! different place in each.
//!
//! # The frame
//!
//! A [`Position`] is `(chapter, offset)` where `chapter` indexes the work's ordered chapters and `offset` counts
//! units *inside* that chapter. Units, not bytes: callers pass the length of each chapter in whatever unit they
//! render in (characters, words, even paragraphs) as long as it is the same unit on both sides. The conversion
//! below is arithmetic on those lengths and nothing else, which is what makes it testable without a database, a
//! renderer or a browser.
//!
//! # Why conversion can fail, and why that is the honest answer
//!
//! A stored position can name a chapter that no longer exists — the author deleted chapter 7, or a draft was
//! published in a different order. Clamping silently to the end would drop the reader somewhere they never were;
//! returning `None` lets the caller choose (usually: top of the nearest surviving chapter, with a notice), and the
//! difference between "we moved you" and "you are where you were" is exactly what a reader notices.

/// A place in a work, in one of the two frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Index into the work's ordered chapters.
    pub chapter: usize,
    /// Units into that chapter, counted from its start.
    pub offset: usize,
}

/// The chapter lengths of a work, in render units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chapters<'a>(pub &'a [usize]);

impl Chapters<'_> {
    /// The number of chapters.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the work has no chapters at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Total length of the work in the same unit.
    #[must_use]
    pub fn total(&self) -> usize {
        self.0.iter().sum()
    }
}

/// Convert `(chapter, offset)` into an offset into the whole work.
///
/// Returns `None` when the chapter does not exist or the offset is past the end of it — a position that cannot be
/// located is not silently the end of the chapter.
#[must_use]
pub fn to_whole(chapters: Chapters<'_>, position: Position) -> Option<usize> {
    if position.chapter >= chapters.len() {
        return None;
    }
    if position.offset > chapters.0[position.chapter] {
        return None;
    }
    let before: usize = chapters.0[..position.chapter].iter().sum();
    Some(before + position.offset)
}

/// Convert an offset into the whole work back into a chapter and an offset inside it.
///
/// `offset == total` is a real position — the cursor after the last unit — and maps to the end of the last
/// chapter. Anything beyond that is not a place in this work.
#[must_use]
pub fn from_whole(chapters: Chapters<'_>, offset: usize) -> Option<Position> {
    if chapters.is_empty() {
        return None;
    }
    let mut remaining = offset;
    for (index, length) in chapters.0.iter().enumerate() {
        if remaining <= *length {
            return Some(Position {
                chapter: index,
                offset: remaining,
            });
        }
        remaining -= *length;
    }
    None
}

/// Switch frames, refusing rather than clamping. The named operation, so a caller cannot accidentally use
/// `to_whole` for a conversion it meant to be total.
#[must_use]
pub fn switch(chapters: Chapters<'_>, position: Position) -> Option<Position> {
    from_whole(chapters, to_whole(chapters, position)?)
}

/// Put a position in canonical form.
///
/// A chapter boundary is one place with two spellings: `(0, 100)` and `(1, 0)` in a work whose first chapter has
/// 100 units. Storing either is fine, but comparing them as tuples reports two different places for one place, so
/// the canonical spelling is the **end of the earlier chapter** — every boundary reads as `(n, len(n))` and only
/// the final position of the work carries an offset equal to its chapter's length.
#[must_use]
pub fn canonical(chapters: Chapters<'_>, position: Position) -> Option<Position> {
    from_whole(chapters, to_whole(chapters, position)?)
}

/// The nearest surviving place, for a caller that has decided a notice is better than nothing.
///
/// Clamps the chapter into range and the offset into that chapter. This is deliberately a *separate* function from
/// the exact conversion: a reader who is moved should be able to tell that they were moved.
#[must_use]
pub fn nearest(chapters: Chapters<'_>, position: Position) -> Option<Position> {
    if chapters.is_empty() {
        return None;
    }
    let chapter = position.chapter.min(chapters.len() - 1);
    Some(Position {
        chapter,
        offset: position.offset.min(chapters.0[chapter]),
    })
}
