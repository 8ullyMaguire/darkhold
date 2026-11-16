import { beforeEach, describe, expect, it, vi } from 'vitest';

class FakeWorker extends EventTarget {
  state = 'installing';
  posted: unknown[] = [];
  postMessage(message: unknown) {
    this.posted.push(message);
    this.state = 'activated';
    this.dispatchEvent(new Event('statechange'));
  }
  setState(state: string) {
    this.state = state;
    this.dispatchEvent(new Event('statechange'));
  }
}

class FakeRegistration extends EventTarget {
  installing: FakeWorker | null = null;
  emitUpdate(worker: FakeWorker) {
    this.installing = worker;
    this.dispatchEvent(new Event('updatefound'));
  }
}

class FakeContainer extends EventTarget {
  controller: unknown = null;
  registration = new FakeRegistration();
  register = vi.fn(async () => this.registration);
}

describe('PWA lifecycle', () => {
  let container: FakeContainer;

  beforeEach(() => {
    container = new FakeContainer();
    Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: container });
    vi.resetModules();
  });

  it('offers install once, only when the browser fires beforeinstallprompt', async () => {
    const { registerPwa } = await import('./pwa');
    const onInstallAvailable = vi.fn();
    registerPwa({ onInstallAvailable });
    expect(onInstallAvailable).not.toHaveBeenCalled();
    const event = new Event('beforeinstallprompt', { cancelable: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(onInstallAvailable).toHaveBeenCalledTimes(1);
  });

  it('offers an update only when a controller already exists', async () => {
    const { registerPwa } = await import('./pwa');
    const onUpdateAvailable = vi.fn();
    registerPwa({ onUpdateAvailable });
    await Promise.resolve();
    const worker = new FakeWorker();
    container.registration.emitUpdate(worker);
    worker.setState('installed');
    expect(onUpdateAvailable).not.toHaveBeenCalled();
    container.controller = {};
    const second = new FakeWorker();
    container.registration.emitUpdate(second);
    second.setState('installed');
    expect(onUpdateAvailable).toHaveBeenCalledTimes(1);
  });

  it('reloads only after the reader accepts an update, never on first install', async () => {
    const { registerPwa } = await import('./pwa');
    const reload = vi.fn();
    let accept: (() => void) | undefined;
    registerPwa({ onUpdateAvailable: (fn) => { accept = fn; } }, { reload });
    await Promise.resolve();
    container.dispatchEvent(new Event('controllerchange'));
    expect(reload).not.toHaveBeenCalled();
    container.controller = {};
    const worker = new FakeWorker();
    container.registration.emitUpdate(worker);
    worker.setState('installed');
    accept?.();
    expect(worker.posted).toEqual([{ type: 'skip-waiting' }]);
    container.dispatchEvent(new Event('controllerchange'));
    expect(reload).toHaveBeenCalledTimes(1);
  });
});
