import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/svelte';
import ResumePrompt from './ResumePrompt.svelte';
import type { ProgressResolution, ResumeView } from '../api';

function resume(overrides: Partial<ResumeView>): ResumeView {
  return {
    state: 'current',
    percent: 42,
    display: '42%',
    sentence: 'You can pick up where you left off.',
    needs_a_choice: false,
    ...overrides,
  };
}

const position = { revision: 'r1', anchor: null, position_permille: 420, device: 'laptop' };
const other = { revision: 'r2', anchor: null, position_permille: 900, device: 'phone' };

describe('M71-09 resume honesty', () => {
  it('prints the server sentence and display for a current place', () => {
    const { getByText, container } = render(ResumePrompt, {
      props: {
        workId: 'w1',
        firstUnitId: 'u1',
        resolution: { kind: 'use_stored', position } as ProgressResolution,
        resume: resume({}),
      },
    });
    expect(getByText('You can pick up where you left off.')).toBeTruthy();
    expect(getByText('42%')).toBeTruthy();
    expect(container.textContent).not.toContain('0%');
  });

  it('renders an em dash and no fabricated number when the place is unmeasured', () => {
    const { getByText, container } = render(ResumePrompt, {
      props: {
        workId: 'w1',
        firstUnitId: 'u1',
        resolution: { kind: 'no_position' } as ProgressResolution,
        resume: resume({
          state: 'unmeasured',
          percent: null,
          display: '—',
          sentence: 'There is no saved place in this text.',
        }),
      },
    });
    expect(getByText('There is no saved place in this text.')).toBeTruthy();
    expect(getByText('—')).toBeTruthy();
    expect(container.textContent).not.toMatch(/\d+%/);
  });

  it('offers both devices as choices when they disagree, without inventing a percentage', () => {
    const { getByText, container, getAllByRole } = render(ResumePrompt, {
      props: {
        workId: 'w1',
        firstUnitId: 'u1',
        resolution: { kind: 'ask_the_reader', mine: position, other } as ProgressResolution,
        resume: resume({
          state: 'conflict',
          percent: null,
          display: '—',
          sentence: 'Two devices have different places in this text. Choose the one to continue from.',
          needs_a_choice: true,
        }),
      },
    });
    expect(
      getByText('Two devices have different places in this text. Choose the one to continue from.'),
    ).toBeTruthy();
    expect(getByText('laptop')).toBeTruthy();
    expect(getByText('phone')).toBeTruthy();
    expect(getAllByRole('link')).toHaveLength(2);
    expect(container.textContent).not.toMatch(/\d+%/);
  });

  it('offers the stale place and a top-of-text alternative, saying why', () => {
    const { getByText, getAllByRole } = render(ResumePrompt, {
      props: {
        workId: 'w1',
        firstUnitId: 'u1',
        resolution: { kind: 'use_stored', position } as ProgressResolution,
        resume: resume({
          state: 'stale',
          percent: null,
          display: '—',
          sentence:
            'Your place was saved on an earlier version of this text. You can go back to it, or start from the top.',
          needs_a_choice: true,
        }),
      },
    });
    expect(
      getByText(
        'Your place was saved on an earlier version of this text. You can go back to it, or start from the top.',
      ),
    ).toBeTruthy();
    expect(getByText('laptop')).toBeTruthy();
    expect(getByText('Start from the top')).toBeTruthy();
    expect(getAllByRole('link')).toHaveLength(2);
  });
});
