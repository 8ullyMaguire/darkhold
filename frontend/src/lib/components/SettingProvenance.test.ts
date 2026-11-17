import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

vi.mock('../api', () => ({ apiFetch: vi.fn(async () => undefined) }));

import { apiFetch } from '../api';
import SettingProvenance from './SettingProvenance.svelte';

describe('SettingProvenance', () => {
  it('names an onboarding default as the instance, explains it, and offers no reset', () => {
    render(SettingProvenance, {
      props: {
        label: 'Who can see your reading history',
        view: {
          value: 'readers',
          source: 'instance',
          reset: null,
          note: 'Set when you registered; change it to make it yours.',
        },
      },
    });
    expect(screen.getByText(/who can see your reading history/i)).toBeInTheDocument();
    expect(screen.getByText('readers')).toBeInTheDocument();
    expect(screen.getByText('instance')).toBeInTheDocument();
    expect(screen.getByText(/set when you registered/i)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /reset/i })).toBeNull();
  });

  it('links its label to the exact control that owns the value', () => {
    render(SettingProvenance, {
      props: {
        label: 'Who can see your reading history',
        controlId: 'privacy-history_visibility',
        view: {
          value: 'readers',
          source: 'instance',
          reset: null,
        },
      },
    });
    const link = screen.getByRole('link', { name: /who can see your reading history/i });
    expect(link).toHaveAttribute('href', '#privacy-history_visibility');
  });

  it('offers a reset that calls the server path and tells the parent to reload', async () => {
    const onchange = vi.fn();
    render(SettingProvenance, {
      props: {
        label: 'Who can see your reading history',
        view: {
          value: 'private',
          source: 'account',
          reset: { method: 'DELETE', path: '/api/v1/settings/privacy/history_visibility', becomes: 'the instance default' },
        },
        onchange,
      },
    });
    expect(screen.getByText('account')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: /reset/i }));
    await waitFor(() => expect(apiFetch).toHaveBeenCalledWith(
      '/api/v1/settings/privacy/history_visibility',
      expect.objectContaining({ method: 'DELETE' }),
    ));
    expect(onchange).toHaveBeenCalledOnce();
  });
});
