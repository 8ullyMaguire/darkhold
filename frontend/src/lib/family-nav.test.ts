import { render, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { FamilyNav } from '@polaris/ui';

/**
 * `more from this maker` (M72-07), as a behaviour: which sites appear, and — the half that matters more —
 * when the component renders nothing at all. A footer that throws, or that shows an empty heading, is worse
 * than a footer that is not there: every product serves it on every page.
 */
const document = {
  estate: 'polaris',
  groups: [
    {
      key: 'social',
      label: 'Social',
      sites: [
        { name: 'ascend', display: 'Ascend', url: 'https://ascend.example', tagline: 'Draw a character' },
        { name: 'gravity', display: 'Gravity', url: 'https://gravity.example', tagline: 'A federated social' },
      ],
    },
    {
      key: 'content',
      label: 'Content',
      sites: [
        { name: 'darkhold', display: 'Darkhold', url: 'https://darkhold.example', tagline: 'An archive' },
        { name: 'kindred', display: 'Kindred', url: 'https://kindred.example', tagline: 'Read something new' },
        { name: 'radio', display: 'Radio', url: 'https://radio.example', tagline: 'Shortwave, in a tab' },
      ],
    },
  ],
};

const respond = (body: unknown, ok = true) =>
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({ ok, json: async () => body })),
  );

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('FamilyNav', () => {
  it("shows this product's group, without itself", async () => {
    respond(document);
    const { findByRole, queryByText } = render(FamilyNav, { props: { self: 'darkhold' } });

    const heading = await findByRole('heading', { name: /more content from the same maker/i });
    expect(heading).toBeTruthy();

    // The siblings, with their taglines — and not the product the reader is already on.
    expect(await findByRole('link', { name: /Kindred/ })).toBeTruthy();
    expect(await findByRole('link', { name: /Radio/ })).toBeTruthy();
    expect(queryByText('Darkhold')).toBeNull();
    // A different group's sites are not siblings, however live they are.
    expect(queryByText('Ascend')).toBeNull();
  });

  it('renders nothing when the document is not served', async () => {
    respond({}, false);
    const { container } = render(FamilyNav, { props: { self: 'darkhold' } });

    // Let the rejection settle, then assert the absence.
    await waitFor(() => expect(container.querySelector('.family')).toBeNull());
    expect(container.textContent?.trim()).toBe('');
  });

  it('renders nothing when this product is the only one in its group', async () => {
    respond({
      groups: [{ key: 'tools', label: 'Tools', sites: [{ name: 'solo', url: 'https://solo.example' }] }],
    });
    const { container } = render(FamilyNav, { props: { self: 'solo' } });

    await waitFor(() => expect(container.querySelector('.family')).toBeNull());
  });
});
