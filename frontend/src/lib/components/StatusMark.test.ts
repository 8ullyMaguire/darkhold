import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import StatusMark from './StatusMark.svelte';

describe('StatusMark', () => {
  it('names the state in text, not only in colour', () => {
    render(StatusMark, { props: { level: 'red', label: 'Mirror behind' } });
    expect(screen.getByText('red')).toBeInTheDocument();
    expect(screen.getByText('Mirror behind')).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveAttribute('aria-label', 'red: Mirror behind');
  });

  it('offers a fix link beside a red state when one is supplied', () => {
    render(StatusMark, { props: { level: 'red', label: 'Queue stalled', fixHref: '/admin/jobs' } });
    expect(screen.getByRole('link', { name: /fix/i })).toHaveAttribute('href', '/admin/jobs');
  });

  it('does not print a fix link for green', () => {
    render(StatusMark, { props: { level: 'green', label: 'Healthy', fixHref: '/admin/jobs' } });
    expect(screen.queryByRole('link', { name: /fix/i })).toBeNull();
  });
});
