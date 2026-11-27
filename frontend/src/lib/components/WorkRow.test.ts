import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import WorkRow from './WorkRow.svelte';

const base = {
  title: 'A Quiet Archive',
  author: 'archivist',
  words: 42300,
  score: 7,
  href: '/works/abc',
  provenance: 'hosted',
  status: 'live',
} as const;

describe('WorkRow', () => {
  it('renders one title link, a byline, metrics, and both facts in order', () => {
    const { container } = render(WorkRow, { props: base });
    const links = [...container.querySelectorAll('a[href="/works/abc"]')];
    expect(links).toHaveLength(1);
    expect(links[0].textContent).toContain('A Quiet Archive');
    expect(screen.getByText(/by archivist/i)).toBeTruthy();
    expect(screen.getByText(/42,300 words/)).toBeTruthy();
    expect(screen.getByText(/7 points/)).toBeTruthy();
    const text = container.textContent ?? '';
    expect(text.indexOf('by archivist')).toBeLessThan(text.indexOf('42,300 words'));
    expect(screen.getByText('Hosted here')).toBeTruthy();
    expect(screen.getByText('Live')).toBeTruthy();
  });

  it('names provenance and status without turning the badge into a second link', () => {
    const { container } = render(WorkRow, {
      props: { ...base, provenance: 'referenced', status: 'held' },
    });
    expect(screen.getByText('Points elsewhere')).toBeTruthy();
    expect(screen.getByText('Held')).toBeTruthy();
    expect(container.querySelectorAll('a')).toHaveLength(1);
    expect(screen.getByText('Points elsewhere').getAttribute('title')).toMatch(/no text here/i);
  });

  it('uses a single, honest word for unknown provenance', () => {
    render(WorkRow, { props: { ...base, provenance: 'known', status: 'private' } });
    expect(screen.getByText('Catalogued only')).toBeTruthy();
    expect(screen.getByText('Private')).toBeTruthy();
  });

  it('says unmeasured when availability is not supplied', () => {
    render(WorkRow, { props: base });
    expect(screen.getByText('unmeasured')).toBeTruthy();
  });

  it('prints an explicit availability instead of a blank span', () => {
    render(WorkRow, { props: { ...base, availability: 'Readable here' } });
    expect(screen.getByText('Readable here')).toBeTruthy();
  });
});
