/**
 * PWA lifecycle (M7-02, redesigned 2026-11-16).
 *
 * The service worker owns caching. This module owns the human conversation: offer install only
 * when the browser offers it, offer an update only when a NEW worker is actually waiting, and
 * never reload silently — a reload can discard a half-written draft.
 */
export interface PwaEvents {
  onInstallAvailable?: (install: () => void) => void;
  onUpdateAvailable?: (reload: () => void) => void;
}

export interface PwaOptions {
  /** Test seam; production reloads the window. */
  reload?: () => void;
}

export function registerPwa(events: PwaEvents = {}, options: PwaOptions = {}): void {
  if (typeof window === 'undefined' || !('serviceWorker' in navigator)) return;
  const reload = options.reload ?? (() => window.location.reload());
  let reloadWhenControlled = false;

  window.addEventListener('beforeinstallprompt', (event) => {
    event.preventDefault();
    const installEvent = event as Event & { prompt?: () => Promise<void> };
    events.onInstallAvailable?.(() => { void installEvent.prompt?.(); });
  });

  navigator.serviceWorker.addEventListener('controllerchange', () => {
    if (reloadWhenControlled) reload();
  });

  void navigator.serviceWorker
    .register('/service-worker.js')
    .then((registration) => {
      registration.addEventListener('updatefound', () => {
        const next = registration.installing;
        if (!next) return;
        next.addEventListener('statechange', () => {
          // A controller is what makes this an UPDATE. Without one this is the first install,
          // and reloading there would take the reader away from a page they just opened.
          if (next.state === 'installed' && navigator.serviceWorker.controller) {
            events.onUpdateAvailable?.(() => {
              reloadWhenControlled = true;
              next.postMessage({ type: 'skip-waiting' });
            });
          }
        });
      });
    })
    .catch(() => undefined);
}
