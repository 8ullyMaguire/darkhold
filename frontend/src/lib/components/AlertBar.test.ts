import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import AlertBar from './AlertBar.svelte';

const ALERTS = [
  { label: 'import job job-1 failed', href: '#job-job-1' },
  { label: 'mirror behind', href: '/admin/mirror' },
];

describe('AlertBar', () => {
  it('names every red state with a fix link', () => {
    render(AlertBar, { props: { alerts: ALERTS } });
    expect(screen.getByRole('alert')).toHaveTextContent('Needs attention');
    const links = screen.getAllByRole('link', { name: /fix/i });
    expect(links).toHaveLength(2);
    expect(links[0]).toHaveAttribute('href', '#job-job-1');
    expect(links[1]).toHaveAttribute('href', '/admin/mirror');
    expect(screen.getByText('import job job-1 failed')).toBeInTheDocument();
    expect(screen.getByText('mirror behind')).toBeInTheDocument();
  });

  it('is silent when there is nothing red', () => {
    render(AlertBar, { props: { alerts: [] } });
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryByRole('link', { name: /fix/i })).toBeNull();
  });
});
