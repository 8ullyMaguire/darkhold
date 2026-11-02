/**
 * The nav an instance actually serves (M65-06, spec §6.3).
 *
 * ## The bug this exists for
 *
 * The header's destinations were a hardcoded list. An instance that disables a module still
 * rendered the link to it, so the reader followed a destination that answers 404 — and nothing
 * anywhere treated that as a failure, because the nav never asked what the instance runs.
 *
 * ## The rule, and the two ways to get it wrong
 *
 * A nav entry may name the **module** that serves it. If the catalogue says that module is off
 * (or does not list it at all — a build without the surface is the same 404), the entry is not
 * rendered. An entry with no module is core navigation and always renders.
 *
 * Two failure modes decide the details:
 *
 * * **Fail open when the catalogue is unavailable.** If `/api/v1/modules` fails, the nav renders
 *   unfiltered. A hidden destination is a broken site for every reader because one request
 *   timed out; a link that 404s is a broken page for readers of a disabled module. The first is
 *   worse and affects more people.
 * * **A module id the catalogue does not list hides the entry**, rather than showing it. That is
 *   the same case as a disabled module — nothing serves it — and it is what makes the mapping
 *   safe to add before the module exists.
 */

import { apiFetch } from './api';

/** One navigation entry a module declares (§6.3's slot contracts). */
export interface ModuleNavEntry {
  label: string;
  href: string;
  slot: string;
}

/** One row of `/api/v1/modules`. Only the fields the nav reads. */
export interface ModuleRow {
  id: string;
  enabled: boolean;
  /** `enabled` narrowed by the caller's own override — the field a reader's choice lives in. */
  enabled_for_caller: boolean;
  nav: ModuleNavEntry[];
}

/** The part of the catalogue payload the nav uses. */
export interface ModuleCatalogue {
  core: string[];
  modules: ModuleRow[];
}

/** A nav destination, optionally owned by a module. */
export interface NavLink {
  href: string;
  label: string;
  module?: string;
}

/** `/api/v1/modules`, the payload the nav filters against. */
export function fetchModules(signal?: AbortSignal): Promise<ModuleCatalogue> {
  return apiFetch<ModuleCatalogue>('/modules', { signal });
}

/**
 * `items` without the entries whose module this instance does not serve.
 *
 * Order is preserved and the input array is never mutated: the same constant is iterated by the
 * desktop menus and the mobile drawer, so a filter that reordered or mutated would make the two
 * layouts disagree.
 */
export function visibleNav<T extends NavLink>(
  items: readonly T[],
  catalogue: ModuleCatalogue | null,
): T[] {
  // A payload that is not the catalogue fails open exactly like a missing one. The shell renders
  // whatever `/api/v1/modules` answered with, and an early version of this function assumed
  // `modules` was an array — one unexpected response and the whole page stopped rendering, which
  // is a worse failure than the 404 this exists to prevent.
  if (!catalogue || !Array.isArray(catalogue.modules)) return [...items];
  return items.filter((item) => {
    if (!item.module) return true;
    const row = catalogue.modules.find((module) => module.id === item.module);
    return row ? row.enabled_for_caller : false;
  });
}

/**
 * The nav entries the enabled modules declare, de-duplicated by `href`.
 *
 * Served by `/api/v1/modules` today with `nav: []` on every row — no module has declared an entry
 * yet — so this returns `[]` in practice. It exists because the payload's `nav` field is the slot
 * contract §6.3 promised: when a module declares a surface, the header renders it without the
 * frontend being changed in the same commit.
 */
export function moduleNavEntries(catalogue: ModuleCatalogue | null): ModuleNavEntry[] {
  if (!catalogue || !Array.isArray(catalogue.modules)) return [];
  const seen = new Set<string>();
  const out: ModuleNavEntry[] = [];
  for (const module of catalogue.modules) {
    if (!module.enabled_for_caller) continue;
    for (const entry of module.nav ?? []) {
      if (seen.has(entry.href)) continue;
      seen.add(entry.href);
      out.push(entry);
    }
  }
  return out;
}
