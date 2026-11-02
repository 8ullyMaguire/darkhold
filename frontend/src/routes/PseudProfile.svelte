<script lang="ts">
  import { ApiError, fetchPublicPseud, type PublicPseud } from '../lib/api';
  import { handleLinkClick } from '../lib/router';
  import { formatTimestamp } from '../lib/time';
  import EmptyState from '../lib/components/EmptyState.svelte';
  import ErrorSummary from '../lib/components/ErrorSummary.svelte';
  import Skeleton from '../lib/components/Skeleton.svelte';

  interface Props {
    /** The handle from the path, without the leading @. */
    handle: string;
  }

  let { handle }: Props = $props();

  let pseud = $state<PublicPseud | null>(null);
  let error = $state<unknown>(null);
  let loading = $state(true);
  let missing = $state(false);

  async function load() {
    loading = true;
    error = null;
    missing = false;
    try {
      pseud = await fetchPublicPseud(handle);
    } catch (failure) {
      // A hidden pseud is a 404, not a 403: the site does not confirm that a
      // profile exists but is withheld. The page says the same thing.
      if (failure instanceof ApiError && failure.code === 'NOT_FOUND') missing = true;
      else error = failure;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    // Re-read when the handle changes, which happens on client-side navigation
    // between two profiles.
    void handle;
    void load();
  });
</script>

{#if loading}
  <Skeleton lines={4} label={`Loading @${handle}`} />
{:else if missing}
  <EmptyState
    title={`No pseud called @${handle}`}
    description="A profile that exists but is hidden is reported the same way, so this page never confirms who is here."
  >
    {#snippet action()}
      <a href="/" onclick={(event) => handleLinkClick(event, '/')}>Back to the front page</a>
    {/snippet}
  </EmptyState>
{:else if error}
  <ErrorSummary {error} onretry={load} />
{:else if pseud}
  <article class="profile">
    <header>
      <h1>
        {pseud.display_name}
        {#if pseud.debut}
          <span class="debut">New author</span>
        {/if}
      </h1>
      <p class="handle">@{pseud.handle}</p>
    </header>

    {#if pseud.badges.length > 0}
      <ul class="badges" aria-label="Badges">
        {#each pseud.badges as badge (badge.badge)}
          <li class="badge">
            <span class="badge-name">{badge.name}</span>
          </li>
        {/each}
      </ul>
    {/if}

    {#if pseud.bio}
      <p class="bio">{pseud.bio}</p>
    {:else}
      <p class="bio empty">This pseud has not written a biography.</p>
    {/if}

    <dl class="facts">
      <dt>On Darkhold since</dt>
      <dd>{formatTimestamp(pseud.created_at)}</dd>
    </dl>

    <p class="note">
      Works, bookmarks and followers appear here once the milestones that hold them are
      built. Nothing on this page links this pseud to any other.
    </p>
  </article>
{/if}

<style>
  .profile {
    max-width: 52rem;
  }

  h1 {
    font-family: var(--font-heading);
    margin: 0;
  }

  .debut {
    margin-left: var(--space-2);
    font-family: var(--font-interface);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-accent);
    border: 1px solid var(--color-accent);
    border-radius: 999px;
    padding: 0 var(--space-2);
    vertical-align: middle;
  }

  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    list-style: none;
    padding: 0;
    margin: 0 0 var(--space-4);
  }

  .badge {
    border: 1px solid var(--color-border);
    border-radius: 999px;
    padding: 0 var(--space-3);
    font-size: var(--text-sm);
  }

  .handle {
    color: var(--color-muted);
    margin: var(--space-1) 0 var(--space-5);
  }

  .bio {
    white-space: pre-wrap;
    max-width: 60ch;
    font-size: var(--text-lg);
  }

  .bio.empty {
    color: var(--color-muted);
    font-size: var(--text-base);
  }

  .facts {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-1) var(--space-4);
    font-size: var(--text-sm);
    margin: var(--space-5) 0;
    padding-top: var(--space-4);
    border-top: var(--border-width) solid var(--color-border);
  }

  .facts dt {
    color: var(--color-muted);
  }

  .facts dd {
    margin: 0;
  }

  .note {
    color: var(--color-muted);
    font-size: var(--text-sm);
    max-width: 60ch;
  }
</style>
