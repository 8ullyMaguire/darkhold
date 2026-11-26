# docs/plans/milestone-06-xenforo-recon.md — plan card

Source: `docs/plans/milestone-06-xenforo-recon.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Landed.** `crates/scrapers/src/sites/xenforo.rs` is written and tested: three
`crates/scrapers/tests/fixtures/xenforo/`. The module documentation in
`xenforo.rs` and the `## xenforo` section of `tests/fixtures/README.md` are the

## Copy-pasteable code / commands

User-agent: *
Content-Signal: search=yes,ai-train=no,use=reference
Allow: /

User-agent: Amazonbot
Disallow: /
… ninety more lines, every one of them a named crawler …

<div class="threadmarkListingHeader"> …  <dt>Created</dt><dd><time datetime="2013-06-24T23:27:50-0400">…
                                          <dt>Status</dt><dd>Ongoing</dd>
                                          <dt>Watchers</dt><dd>209</dd>
                                          <dt>Threadmarks</dt><dd>42</dd> …

<div class="structItem structItem--threadmark js-inlineModContainer "
     data-likes="59" data-content-author="master arminas" data-content-date="1372130903">
  <div class="structItem-cell structItem-cell--main">
    <div class="structItem-title threadmark_depth0">
      <ul class="listInline listInline--bullet">
        <li><a href="/threads/by-the-horns-story-only-thread.262832/#post-11149727">Prologue; September 27, 2596</a></li>
      </ul>
    </div>
  </div>
  <div class="structItem-cell structItem-cell--meta" title="Likes: 59">
    <dl class="pairs pairs--justified"><dt>Words</dt><dd>1.7k</dd></dl>
  </div>
  <div class="structItem-cell structItem-cell--latest">
    <a href="/threads/by-the-horns-story-only-thread.262832/#post-11149727"><time … datetime="2013-06-24T23:28:23-0400">…</time></a>
  </div>
</div>

## Verification command and expected output

test over all three hosts. `v0.07-imports` is unblocked by it.
`Watchers`. The first pass read the header for the first three and missed the
is not — so a file that allowed everything to everybody could not pass.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
