<script lang="ts">
  import { ApiError } from '../lib/api';
  import { handleLinkClick, navigate } from '../lib/router';
  import { session } from '../lib/session.svelte.ts';
import { Button } from '@polaris/ui';
  import ErrorSummary from '../lib/components/ErrorSummary.svelte';
  import TextField from '../lib/components/TextField.svelte';

  interface Props {
    /** Where to go after signing in. */
    next?: string;
  }

  let { next = '/' }: Props = $props();

  let email = $state('');
  let password = $state('');
  let error = $state<unknown>(null);
  let submitting = $state(false);
  let fieldErrors = $state<Record<string, string>>({});

  // W2: one account for the estate. The button is a plain navigation, not a fetch: the provider has to be
  // a top-level document, because its session cookie is first-party to id.polarisocial.xyz and a
  // cross-site fetch would send nothing.
  const polarisHref = `/api/v1/auth/oidc/start?next=${encodeURIComponent(next)}`;

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    submitting = true;
    error = null;
    fieldErrors = {};

    try {
      await session.signInWith(email.trim(), password);
      password = '';
      navigate(next);
    } catch (failure) {
      error = failure;
      if (failure instanceof ApiError) fieldErrors = failure.fieldErrors;
    } finally {
      submitting = false;
    }
  }
</script>

<section class="page">
  <h1>Sign in</h1>

  {#if error}
    <ErrorSummary {error} />
  {/if}

  <p class="polaris">
    <a class="polaris-button" href={polarisHref}>Continue with Polaris</a>
    <span class="note">One account for every Polaris site. If you are already signed in there, this takes
      you straight back.</span>
  </p>

  <form onsubmit={submit} novalidate>
    <TextField
      id="sign-in-email"
      label="Email address"
      type="email"
      autocomplete="email"
      required
      value={email}
      error={fieldErrors.email}
      oninput={(event) => (email = event.currentTarget.value)}
    />

    <TextField
      id="sign-in-password"
      label="Password"
      type="password"
      autocomplete="current-password"
      required
      value={password}
      error={fieldErrors.password}
      oninput={(event) => (password = event.currentTarget.value)}
    />

    <p class="note">
      An unknown address and a wrong password produce the same answer, so this form cannot
      be used to find out which addresses have accounts.
    </p>

    <div class="actions">
      <Button type="submit" loading={submitting}>Sign in</Button>
      <a href="/register" onclick={(event) => handleLinkClick(event, '/register')}>
        Create an account
      </a>
      <a href="/password-reset" onclick={(event) => handleLinkClick(event, '/password-reset')}>
        Forgot your password?
      </a>
    </div>
  </form>
</section>

<style>
  .page {
    max-width: 30rem;
  }

  h1 {
    font-family: var(--font-heading);
    margin-bottom: var(--space-5);
  }

  .polaris {
    display: grid;
    gap: var(--space-2);
    margin: 0 0 var(--space-5);
  }

  .polaris-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    font-weight: 600;
    background: var(--color-surface-raised);
  }

  .note {
    color: var(--color-muted);
    font-size: var(--text-sm);
    max-width: 52ch;
    margin: 0 0 var(--space-4);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
</style>
