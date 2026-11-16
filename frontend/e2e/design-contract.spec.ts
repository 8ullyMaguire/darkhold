import { test, expect } from '@playwright/test';

const widths = [320, 390, 768, 1440];

for (const width of widths) {
  test(`no horizontal scroll and a visible skip link at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/search');
    const overflow = await page.evaluate(() =>
      document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow, `document overflows by ${overflow}px`).toBeLessThanOrEqual(0);
    await page.keyboard.press('Tab');
    const text = await page.evaluate(() => document.activeElement?.textContent?.trim());
    expect(text).toMatch(/skip to content/i);
    await expect.poll(() => page.evaluate(() =>
      document.activeElement?.getBoundingClientRect().top ?? -1)).toBeGreaterThanOrEqual(0);
  });
}

test('main navigation exposes one row on desktop and a mobile bar', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/search');
  const desktop = page.locator('header nav.desktop');
  await expect(desktop).toBeVisible();
  const rows = await desktop.evaluate((el) => new Set(
    [...el.querySelectorAll('a')].map((a) => Math.round(a.getBoundingClientRect().top))).size);
  expect(rows).toBe(1);
  await page.setViewportSize({ width: 390, height: 900 });
  await expect(page.locator('header nav.desktop')).toBeHidden();
  await expect(page.getByRole('button', { name: /more/i }).first()).toBeVisible();
});

test('forced colors keeps a visible focus ring', async ({ page }) => {
  await page.emulateMedia({ forcedColors: 'active' });
  await page.setViewportSize({ width: 390, height: 900 });
  await page.goto('/search');
  await page.keyboard.press('Tab');
  const ring = await page.evaluate(() =>
    getComputedStyle(document.activeElement as HTMLElement).outlineStyle);
  expect(ring).not.toBe('none');
});

test('interactive targets are at least 24 CSS px', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 900 });
  await page.goto('/search');
  const small = await page.evaluate(() =>
    [...document.querySelectorAll('header a, header button, nav a, nav button, footer a, footer button, .skip-link')]
      .filter((el) => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; })
      .filter((el) => { const r = el.getBoundingClientRect(); return r.width < 24 || r.height < 24; })
      .map((el) => `${el.tagName}:${(el.textContent || '').trim().slice(0, 24)}`));
  expect(small, small.join(', ')).toEqual([]);
});
