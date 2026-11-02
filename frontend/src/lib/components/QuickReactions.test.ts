import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import QuickReactions from './QuickReactions.svelte';
import { session } from '../session.svelte';

const fetchQuickReactions = vi.fn();
const addQuickReaction = vi.fn();
const removeQuickReaction = vi.fn();

vi.mock('../api', () => ({
  fetchQuickReactions: (...args: unknown[]) => fetchQuickReactions(...args),
  addQuickReaction: (...args: unknown[]) => addQuickReaction(...args),
  removeQuickReaction: (...args: unknown[]) => removeQuickReaction(...args),
}));

const VOCABULARY: [string, string][] = [
  ['made_me_cry', 'made me cry'],
  ['banter', 'the banter!'],
  ['need_more', 'I need more of this'],
  ['this_scene', 'this scene!'],
  ['comforting', 'comforting'],
  ['worldbuilding', 'worldbuilding!'],
];

function view(overrides: Partial<Parameters<typeof fetchQuickReactions>[0]> = {}) {
  return {
    items: [],
    mine: [],
    enabled: true,
    vocabulary: VOCABULARY,
    ...overrides,
  };
}

beforeEach(() => {
  session.status = 'signed-in';
  session.me = { pseuds: [], active_pseud_id: null } as any;
  fetchQuickReactions.mockReset();
  fetchQuickReactions.mockResolvedValue(view());
  addQuickReaction.mockReset();
  addQuickReaction.mockResolvedValue({ label: 'banter', reacted: true });
  removeQuickReaction.mockReset();
  removeQuickReaction.mockResolvedValue({ label: 'banter', removed: true });
});

describe('QuickReactions', () => {
  it('renders every label from the server vocabulary', async () => {
    render(QuickReactions, { workId: 'w1' });

    expect(await screen.findByText('made me cry')).toBeInTheDocument();
    expect(screen.getByText('the banter!')).toBeInTheDocument();
    expect(screen.getByText('worldbuilding!')).toBeInTheDocument();
  });

  it('renders nothing when the author has feedback off', async () => {
    fetchQuickReactions.mockResolvedValue(view({ enabled: false }));
    render(QuickReactions, { workId: 'w1' });

    await waitFor(() => expect(fetchQuickReactions).toHaveBeenCalled());
    expect(screen.queryByLabelText('Quick reactions')).not.toBeInTheDocument();
  });

  it('shows a count only for labels somebody used', async () => {
    fetchQuickReactions.mockResolvedValue(
      view({ items: [{ label: 'banter', count: 3 }] }),
    );
    render(QuickReactions, { workId: 'w1' });

    expect(await screen.findByText('3')).toBeInTheDocument();
    expect(screen.getAllByRole('button')).toHaveLength(6);
  });

  it('adds a label that is not held and removes one that is', async () => {
    fetchQuickReactions.mockResolvedValue(view({ mine: ['comforting'] }));
    render(QuickReactions, { workId: 'w1' });
    await screen.findByText('made me cry');

    await fireEvent.click(screen.getByText('the banter!'));
    expect(addQuickReaction).toHaveBeenCalledWith('w1', 'banter');

    await fireEvent.click(screen.getByText('comforting'));
    expect(removeQuickReaction).toHaveBeenCalledWith('w1', 'comforting');
  });

  it('re-reads the tally instead of incrementing locally', async () => {
    fetchQuickReactions.mockResolvedValue(view());
    render(QuickReactions, { workId: 'w1' });
    await screen.findByText('made me cry');
    const before = fetchQuickReactions.mock.calls.length;

    await fireEvent.click(screen.getByText('the banter!'));

    // The server owns the count: a local increment on a reaction that was already there would
    // double it, and the double-tap case is exactly the one a local patch gets wrong.
    await waitFor(() =>
      expect(fetchQuickReactions.mock.calls.length).toBeGreaterThan(before),
    );
  });

  it('is read-only for a signed-out reader', async () => {
    session.status = 'anonymous';
    session.me = null;
    render(QuickReactions, { workId: 'w1' });
    await screen.findByText('made me cry');

    const button = screen.getByText('made me cry').closest('button');
    expect(button).toBeDisabled();
    await fireEvent.click(button as HTMLElement);
    expect(addQuickReaction).not.toHaveBeenCalled();
  });
});
