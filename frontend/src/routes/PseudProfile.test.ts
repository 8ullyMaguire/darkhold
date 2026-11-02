import { render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import PseudProfile from './PseudProfile.svelte';

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>();
  return {
    ...actual,
    fetchPublicPseud: vi.fn(),
  };
});

import { fetchPublicPseud } from '../lib/api';

const PROFILE = {
  id: 'pseud-1',
  handle: 'alice',
  display_name: 'Alice',
  bio: 'Writer of stories',
  created_at: '2026-01-01T00:00:00Z',
  badges: [],
  debut: false,
};

beforeEach(() => {
  vi.clearAllMocks();
  (fetchPublicPseud as any).mockResolvedValue(PROFILE);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('PseudProfile page', () => {
  it('renders the pseud handle and display name', async () => {
    render(PseudProfile, { props: { handle: 'alice' } });
    await waitFor(() => expect(screen.getByText('Alice')).toBeInTheDocument());
  });

  it('fetches the public pseud on mount', async () => {
    render(PseudProfile, { props: { handle: 'alice' } });
    await waitFor(() => expect(fetchPublicPseud).toHaveBeenCalledWith('alice'));
  });

  it('renders the bio', async () => {
    render(PseudProfile, { props: { handle: 'alice' } });
    await waitFor(() => expect(screen.getByText('Writer of stories')).toBeInTheDocument());
  });

  it('renders the handle', async () => {
    render(PseudProfile, { props: { handle: 'alice' } });
    await waitFor(() => expect(screen.getByText('Alice')).toBeInTheDocument());
    expect(screen.getByText('@alice')).toBeInTheDocument();
  });
});

describe('PseudProfile badges', () => {
  it('shows the badge a milestone awarded', async () => {
    (fetchPublicPseud as any).mockResolvedValue({
      ...PROFILE,
      badges: [
        { badge: 'first_words', name: 'First Words', credits: 20, awarded_at: '2026-11-02T00:00:00Z' },
      ],
      debut: true,
    });

    render(PseudProfile, { handle: 'alice' });

    expect(await screen.findByText('First Words')).toBeInTheDocument();
    expect(screen.getByText('New author')).toBeInTheDocument();
  });

  it('draws neither when nothing was earned', async () => {
    render(PseudProfile, { handle: 'alice' });

    expect(await screen.findByText('Alice')).toBeInTheDocument();
    // A profile with no badges is a profile, not an empty award case: nothing is rendered rather
    // than an "no badges yet" line nobody asked for.
    expect(screen.queryByLabelText('Badges')).not.toBeInTheDocument();
    expect(screen.queryByText('New author')).not.toBeInTheDocument();
  });
});
