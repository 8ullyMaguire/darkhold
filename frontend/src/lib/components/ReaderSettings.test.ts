import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const accountView = {
  font_scale: 1.2,
  line_height: 1.6,
  measure: 66,
  reader_theme: 'sepia',
  distraction_free: false,
  version: 1,
  provenance: {
    distraction_free: {
      value: 'off',
      source: 'account',
      reset: { method: 'DELETE', path: '/api/v1/settings/typography', becomes: 'instance default' },
    },
    font_scale: {
      value: '1.20',
      source: 'account',
      reset: { method: 'DELETE', path: '/api/v1/settings/typography', becomes: 'instance default' },
    },
    line_height: {
      value: '1.60',
      source: 'account',
      reset: { method: 'DELETE', path: '/api/v1/settings/typography', becomes: 'instance default' },
    },
    measure: {
      value: '66',
      source: 'account',
      reset: { method: 'DELETE', path: '/api/v1/settings/typography', becomes: 'instance default' },
    },
    reader_theme: {
      value: 'sepia',
      source: 'account',
      reset: { method: 'DELETE', path: '/api/v1/settings/typography', becomes: 'instance default' },
    },
  },
};

const instanceView = {
  ...accountView,
  font_scale: 1.0,
  version: 0,
  provenance: Object.fromEntries(
    Object.keys(accountView.provenance).map((key) => [
      key,
      { value: accountView.provenance[key as keyof typeof accountView.provenance].value, source: 'instance', reset: null },
    ]),
  ),
};

vi.mock('../api', () => ({
  fetchTypography: vi.fn(),
  patchTypography: vi.fn(async () => undefined),
  apiFetch: vi.fn(async () => undefined),
}));

import { apiFetch, fetchTypography } from '../api';
import ReaderSettings from './ReaderSettings.svelte';

describe('ReaderSettings provenance', () => {
  it('names the level of each typography value and resets through the bodyless route', async () => {
    vi.mocked(fetchTypography)
      .mockResolvedValueOnce(accountView as never)
      .mockResolvedValue(instanceView as never);
    vi.mocked(apiFetch).mockResolvedValue(undefined as never);

    render(ReaderSettings, { props: { signedIn: true } });

    await waitFor(() => expect(screen.getAllByText('account').length).toBeGreaterThan(0));
    expect(screen.getAllByText('account')).toHaveLength(5);
    const links = screen.getAllByRole('link', { name: /text size/i });
    expect(links.length).toBeGreaterThan(0);
    expect(links[0]).toHaveAttribute('href', '#typography-font_scale');

    await fireEvent.click(screen.getAllByRole('button', { name: /reset to instance default/i })[0]);
    await waitFor(() =>
      expect(apiFetch).toHaveBeenCalledWith(
        '/api/v1/settings/typography',
        expect.objectContaining({ method: 'DELETE' }),
      ),
    );
    await waitFor(() => expect(screen.queryByRole('button', { name: /reset/i })).toBeNull());
    expect(screen.getAllByText('instance').length).toBeGreaterThan(0);
  });
});
