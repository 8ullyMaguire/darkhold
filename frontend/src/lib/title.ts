/**
 * What a tab says.
 *
 * One place decides it, because the alternative was measured and it was worse: `frontend/index.html`
 * carried the shell's title and **nothing in the SPA ever assigned `document.title`**, so nine
 * different addresses (`/browse`, `/tags`, `/discover`, `/community`, …) all wore
 * "Darkhold — the dark side of fanfiction". A reader with eight tabs open could not tell them apart,
 * and a search engine saw nine pages whose only difference was the URL.
 *
 * The map is keyed by `RouteId`, which is a closed union, so a new route cannot ship without a title:
 * the type checker says so. That is the whole point of putting it here rather than in each page —
 * a page that forgets is a page that silently rejoins the duplicates, and nothing would notice until
 * somebody opened eight tabs again.
 */
import type { RouteId, RouteMatch } from './router';

/** What the front page says, and what the shell says before the client runs (see `index.html`). */
export const TAGLINE = 'Darkhold — the dark side of fanfiction';

const SUFFIX = ' — Darkhold';

/**
 * The name of each page. `home` is empty on purpose: the front page is the site, so it wears the
 * tagline instead of a page name. Every other entry must be distinct from every other — a shared
 * name is the defect this file exists to prevent, and `title.test.ts` asserts it.
 */
export const ROUTE_NAMES: Record<RouteId, string> = {
  home: '',
  register: 'Create an account',
  'sign-in': 'Sign in',
  'password-reset': 'Reset a password',
  account: 'Your account',
  settings: 'Settings',
  'author-media': 'Your media',
  pseuds: 'Your pseuds',
  'pseud-profile': '', // the handle, read from the match
  write: 'Write',
  'work-editor': 'Edit a work',
  'work-read': 'Read',
  history: 'Reading history',
  import: 'Import',
  library: 'Your library',
  exports: 'Exports',
  jobs: 'Jobs',
  search: 'Search',
  media: 'Media',
  'media-search': 'Media search',
  'admin-jobs': 'Admin: jobs',
  'admin-economy-flows': 'Admin: economy flows',
  'admin-media-health': 'Admin: media health',
  'admin-mirror': 'Admin: mirror',
  discover: 'Discover',
  concierge: 'Concierge',
  'blind-date': 'Blind date',
  'surprise-me': 'Surprise me',
  draw: 'The draw',
  arena: 'Arena',
  community: 'Community',
  'forum-category': 'Forum category',
  'forum-search': 'Forum search',
  'forum-topic': 'Forum topic',
  notifications: 'Notifications',
  docs: 'Docs',
  'doc-page': '', // the slug, read from the match
  quiz: 'Quiz',
  vanguard: 'Vanguard',
  roadmap: 'Roadmap',
  'roadmap-card': 'Roadmap card',
  directory: 'Directory',
  analytics: 'Analytics',
  planned: 'Coming soon',
  'not-found': 'Page not found',
};

/** `a-work-slug` -> `A work slug`: a slug is an address, not a sentence. */
function slugTitle(slug: string): string {
  const words = slug.replace(/[-_]+/g, ' ').trim();
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : '';
}

/**
 * The title for a matched route.
 *
 * Parameterised routes read the parameter out of the match rather than fetching anything: the tab is
 * set on the first paint, before the page's own request finishes, and a title that arrives late is a
 * title the reader never sees.
 */
export function titleFor(match: RouteMatch): string {
  if (match.id === 'home') return TAGLINE;

  if (match.id === 'pseud-profile') {
    const handle = match.params?.handle?.trim();
    return handle ? `@${handle}${SUFFIX}` : `A profile${SUFFIX}`;
  }

  if (match.id === 'doc-page') {
    const name = slugTitle(match.params?.slug ?? '');
    return name ? `${name}${SUFFIX}` : `Docs${SUFFIX}`;
  }

  if (match.id === 'planned') {
    const name = match.planned?.title?.trim();
    return name ? `${name}${SUFFIX}` : `Coming soon${SUFFIX}`;
  }

  const name = ROUTE_NAMES[match.id];
  return name ? `${name}${SUFFIX}` : `${ROUTE_NAMES['not-found']}${SUFFIX}`;
}
