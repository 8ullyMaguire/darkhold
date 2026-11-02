import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import ReadingGoalRing from './ReadingGoalRing.svelte';

const WINDOW = {
  window_start: '2026-11-02T00:00:00Z',
  window_end: '2026-11-03T00:00:00Z',
};

describe('ReadingGoalRing', () => {
  it('renders the count and the target', async () => {
    render(ReadingGoalRing, {
      load: async () => ({
        goal: { period: 'daily', metric: 'chapters', target: 3, scope: 'pseud' },
        progress: { current: 2, remaining: 1, met: false, ...WINDOW },
      }),
    });

    expect(await screen.findByText('2')).toBeInTheDocument();
    expect(screen.getByText(/\/ 3/)).toBeInTheDocument();
    expect(screen.getByText('chapters')).toBeInTheDocument();
    expect(screen.getByText('today')).toBeInTheDocument();
  });

  it('says "this week" for a weekly goal', async () => {
    render(ReadingGoalRing, {
      load: async () => ({
        goal: { period: 'weekly', metric: 'words', target: 20000, scope: 'pseud' },
        progress: { current: 5000, remaining: 15000, met: false, ...WINDOW },
      }),
    });

    expect(await screen.findByText('this week')).toBeInTheDocument();
    // §9.6: an estimate is labelled as one.
    expect(screen.getByText(/approximate/)).toBeInTheDocument();
  });

  it('is neutral when the target is reached', async () => {
    render(ReadingGoalRing, {
      load: async () => ({
        goal: { period: 'daily', metric: 'chapters', target: 2, scope: 'pseud' },
        progress: { current: 4, remaining: 0, met: true, ...WINDOW },
      }),
    });

    // A met goal does not become a celebration and an unmet one does not become a warning: the same
    // two numbers, and no word about failing either way.
    expect(await screen.findByText('4')).toBeInTheDocument();
    const text = document.body.textContent ?? '';
    for (const forbidden of ['missed', 'failed', 'streak', 'behind', 'try again']) {
      expect(text.toLowerCase()).not.toContain(forbidden);
    }
  });

  it('offers the setting and draws nothing when no goal is set', async () => {
    render(ReadingGoalRing, { load: async () => ({ goal: null }) });

    expect(await screen.findByText('Set a reading goal')).toBeInTheDocument();
    expect(screen.queryByLabelText('Reading goal')).not.toBeInTheDocument();
  });

  it('draws nothing at all when the read fails', async () => {
    render(ReadingGoalRing, {
      load: async () => {
        throw new Error('offline');
      },
    });

    // The library page keeps working; a ring nobody could load is not a page-level error.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByLabelText('Reading goal')).not.toBeInTheDocument();
    expect(screen.queryByText('Set a reading goal')).not.toBeInTheDocument();
  });
});
