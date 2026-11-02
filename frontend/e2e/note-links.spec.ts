/**
 * M45-38 / §32.7.4a — a note's links, in a real browser.
 *
 * The unit tests prove the component decides correctly given a report. They cannot prove the report
 * exists: that the note is written through the editor, that the save extracts its links, and that the
 * work page then reads them back. That chain is what this journey drives, and it is why it types into
 * the editor rather than calling the API — a shortcut here would test the endpoint twice.
 *
 * The links are loopback on purpose. The fetch guard refuses a private address unconditionally, so
 * "check these links now" produces a **deterministic** dead link with no network involved — which is
 * also the honest behaviour to assert: an author pointing a note at their own machine gets told the
 * link is dead, not "degraded, try tomorrow".
 */

import { expect, test, type Page } from '@playwright/test';

const PASSPHRASE = 'note-links-passphrase-1';

function uniqueHandle(base: string): string {
  return `${base}${Date.now().toString(36).slice(-6)}`;
}

async function signUp(page: Page, base: string): Promise<void> {
  const handle = uniqueHandle(base);
  await page.goto('/register');
  await page.fill('#register-email', `${handle.toLowerCase()}@note-links.test`);
  await page.fill('#register-password', PASSPHRASE);
  await page.fill('#register-handle', handle);
  await page.fill('#register-display-name', handle);
  await page.selectOption('#register-age-band', 'adult');
  await page.click('button[type=submit]');

  const landed = await page
    .waitForURL(/\/account/, { timeout: 8000 })
    .then(() => true)
    .catch(() => false);
  if (landed) return;

  await page.goto('/sign-in');
  await page.fill('#sign-in-email', `${handle.toLowerCase()}@note-links.test`);
  await page.fill('#sign-in-password', PASSPHRASE);
  await page.click('button[type=submit]');
  await expect(page).toHaveURL(/\/(account|home|discover)/, { timeout: 8000 });
}

/** Publish a work whose single unit carries the given author's note. Returns the work id. */
async function publishWithNote(page: Page, title: string, note: string): Promise<string> {
  await page.goto('/write');
  await page.fill('input[id^=field-]', title);
  await page.click('button:text-is("Start a draft")');
  await expect(page).toHaveURL(/\/write\//);
  await page.click('button:text-is("Add unit")');
  await page.locator('.tiptap.ProseMirror').click();
  await page.keyboard.type(`A unit for ${title}, long enough to be a unit.`);

  // The note is written in the editor, by the author, exactly as a person would.
  await page.getByLabel('Revision note (optional)').fill(note);

  const saved = page.waitForResponse(
    (r) => r.url().includes('/api/v1/chapters/') && r.request().method() !== 'GET',
  );
  // The save carries the note; a second save without one would supersede it.
  await page.click('button:text-is("Save now")');
  await saved;
  await page.click('button:text-is("Publish")');
  await expect(page.getByText('Republish')).toBeVisible();
  return page.url().split('/').pop()!;
}

test('a note link is reported dead before a reader clicks it', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));

  await signUp(page, 'NoteLinker');
  const workId = await publishWithNote(
    page,
    'Notes That Rot',
    'Soundtrack: http://127.0.0.1:9/one and http://127.0.0.1:9/two for the vibes.',
  );

  await page.goto(`/works/${workId}`);

  // The report exists, and it is honest about not having checked yet — the extraction happened at
  // save time, and nothing has been fetched since.
  const section = page.getByTestId('note-links');
  await expect(section).toBeVisible();
  await expect(section.getByTestId('note-link')).toHaveCount(2);
  await expect(section).toContainText('not yet checked');

  // The warning is absent before a check: nothing has failed yet.
  await expect(page.getByTestId('note-links-warning')).toHaveCount(0);

  // The author's button runs the check inline.
  await page.getByRole('button', { name: 'check these links now' }).click();
  await expect(section).toContainText(/Checked 2/, { timeout: 20_000 });
  await expect(section).toContainText('dead');

  // …and the reader now hears about it, in the plural, before clicking.
  const warning = page.getByTestId('note-links-warning');
  await expect(warning).toBeVisible();
  await expect(warning).toContainText('2 links have stopped answering');

  // A refusal is a verdict, so it comes with a reason rather than a shrug.
  await expect(section).toContainText(/127\.0\.0\.1|non-public|loopback|refus/i);

  expect(errors).toEqual([]);
});

test('a work whose notes hold no links grows no such section', async ({ page }) => {
  await signUp(page, 'NoLinks');
  const workId = await publishWithNote(page, 'Plain Notes', 'Thanks to my beta readers.');

  await page.goto(`/works/${workId}`);
  await expect(page.getByRole('heading', { name: 'Plain Notes' })).toBeVisible();
  await expect(page.getByTestId('note-links')).toHaveCount(0);
});
