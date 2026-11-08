import { fireEvent, render } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { describe, expect, it, vi } from 'vitest';

/**
 * The six primitives the estate had nowhere (`ui/docs/census.md`, rows 5, 8, 18, 21, 22 and the card
 * frame's action row). They live in `@polaris/ui` and this file is the proof that a product actually
 * renders them: each assertion is a part of the contract a page relies on, not a snapshot of markup.
 *
 * Written here rather than in `estate/ui` because that package has no test runner — `verify.mjs` only
 * compiles. A component that compiles and has never mounted is a component whose a11y wiring is a guess.
 */
import { Avatar, Breadcrumb, CardActions, Checkbox, Kbd } from '@polaris/ui';

import TooltipHost from './tooltip-host.fixture.svelte';

const text = (value: string) =>
  createRawSnippet(() => ({ render: () => `<span>${value}</span>` }));

describe('Checkbox', () => {
  it('names the control with its label and describes it with its hint and error', () => {
    const { getByLabelText, getByText } = render(Checkbox, {
      props: { label: 'Hide crossovers', hint: 'Only in this search', error: 'Pick one' },
    });

    const input = getByLabelText('Hide crossovers') as HTMLInputElement;
    expect(input.type).toBe('checkbox');

    // Both descriptions reach the input, or a screen reader never explains the error it is showing.
    const describedBy = input.getAttribute('aria-describedby') ?? '';
    const ids = describedBy.split(/\s+/).filter(Boolean);
    expect(ids).toHaveLength(2);
    expect(ids.map((id) => document.getElementById(id)?.textContent)).toEqual([
      'Only in this search',
      'Pick one',
    ]);
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(getByText('Pick one').getAttribute('role')).toBe('alert');
  });

  it('reports the new value, and a radio is the same anatomy', async () => {
    const onchange = vi.fn();
    const { getByLabelText } = render(Checkbox, {
      props: { type: 'radio', name: 'sort', value: 'kudos', label: 'Most kudos', onchange },
    });

    const input = getByLabelText('Most kudos') as HTMLInputElement;
    expect(input.type).toBe('radio');
    await fireEvent.click(input);
    expect(onchange).toHaveBeenCalledWith(true);
  });

  it('sets indeterminate on the node, because the attribute does nothing', () => {
    const { getByLabelText } = render(Checkbox, {
      props: { label: 'Select all', indeterminate: true },
    });
    expect((getByLabelText('Select all') as HTMLInputElement).indeterminate).toBe(true);
  });

  it('can hide the label without losing the name', () => {
    const { getByLabelText } = render(Checkbox, {
      props: { label: 'Select The Long Road', labelHidden: true },
    });
    expect(getByLabelText('Select The Long Road')).toBeTruthy();
  });
});

describe('Avatar', () => {
  it('names the person when it is the only thing showing them', () => {
    const { getByRole } = render(Avatar, {
      props: { src: '/a.png', name: 'Ada Lovelace' },
    });
    expect(getByRole('img').getAttribute('alt')).toBe('Ada Lovelace');
  });

  it('is silent when the name is already written beside it', () => {
    const { container } = render(Avatar, {
      props: { src: '/a.png', name: 'Ada Lovelace', decorative: true },
    });
    expect(container.querySelector('img')?.getAttribute('alt')).toBe('');
  });

  it('falls back to initials — and to the name — when the image fails', async () => {
    const { container, getByText } = render(Avatar, { props: { src: '/gone.png', name: 'ada lovelace' } });
    await fireEvent.error(container.querySelector('img')!);
    expect(container.querySelector('img')).toBeNull();
    expect(container.textContent).toContain('AL');
    expect(getByText('ada lovelace')).toBeTruthy();
  });
});

describe('Breadcrumb', () => {
  it('is a labelled nav whose last crumb is the current page and a link never', () => {
    const { getByRole, getAllByRole } = render(Breadcrumb, {
      props: {
        items: [
          { label: 'Help', href: '/docs' },
          { label: 'Importing', href: '/docs/importing' },
        ],
      },
    });

    const nav = getByRole('navigation', { name: 'Breadcrumb' });
    expect(nav.tagName).toBe('NAV');

    // The last item keeps its href in the data and must still not be a link: a crumb pointing at the
    // page you are on is a reload button dressed as navigation.
    expect(getAllByRole('link')).toHaveLength(1);
    const current = nav.querySelector('[aria-current="page"]');
    expect(current?.textContent).toBe('Importing');
  });
});

describe('Tooltip', () => {
  // The fixture writes the two-line snippet a consumer writes. `createRawSnippet` cannot stand in for it:
  // its arguments are captured when the snippet is created, so it would never re-render with the id and
  // the test would be measuring the harness, not the component (which is what the first version did).
  it('shows on focus, describes the focusable child, and leaves on Escape', async () => {
    const { queryByRole, getByRole } = render(TooltipHost, {
      props: { text: 'Votes reset at midnight' },
    });

    expect(queryByRole('tooltip')).toBeNull();

    const trigger = getByRole('button');
    await fireEvent.focusIn(trigger);

    const tip = getByRole('tooltip');
    expect(tip.getAttribute('role')).toBe('tooltip');
    expect(tip.textContent).toBe('Votes reset at midnight');
    // The description has to sit on the element that takes focus; a wrapper never is.
    expect(trigger.getAttribute('aria-describedby')).toBe(tip.id);

    await fireEvent.keyDown(trigger, { key: 'Escape' });
    expect(queryByRole('tooltip')).toBeNull();
    // Svelte omits an attribute whose value is undefined, so the description is gone, not empty.
    expect(trigger.getAttribute('aria-describedby')).toBeNull();
  });
});

describe('Kbd and CardActions', () => {
  it('renders a real kbd element with its key', () => {
    const { getByText } = render(Kbd, { props: { children: text('Ctrl') } });
    expect(getByText('Ctrl').closest('kbd')?.tagName).toBe('KBD');
  });

  it('renders the action row, and the divider is opt-in', () => {
    const { container } = render(CardActions, { props: { children: text('Download') } });
    const row = container.querySelector('.actions')!;
    expect(row.textContent).toContain('Download');
    expect(row.classList.contains('divider')).toBe(false);

    const divided = render(CardActions, { props: { divider: true, children: text('Save') } });
    expect(divided.container.querySelector('.actions')!.classList.contains('divider')).toBe(true);
  });
});
