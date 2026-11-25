// Progressive enhancement only. The reader is fully readable without this file.
(() => {
  const main = document.querySelector('.reader-surface__main');
  const body = document.querySelector('.reader-surface__body');
  if (!main || !body) return;

  const rail = document.querySelector('.reader-surface__rail');
  const paint = () => {
    const scrollable = Math.max(1, document.documentElement.scrollHeight - innerHeight);
    const value = Math.min(100, Math.max(0, Math.round((scrollY / scrollable) * 100)));
    main.style.setProperty('--reader-progress', String(value));
    if (rail) rail.setAttribute('aria-valuenow', String(value));
  };

  const saved = Number(main.dataset.restorePermille || 0);
  if (saved > 1 && saved <= 1000 && !location.hash) {
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const top = body.getBoundingClientRect().top + scrollY;
      const distance = Math.max(0, body.scrollHeight - innerHeight);
      scrollTo(0, top + (distance * saved) / 1000);
      paint();
    }));
  }
  addEventListener('scroll', paint, { passive: true });
  addEventListener('resize', paint, { passive: true });
  paint();
})();
