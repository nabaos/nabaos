<script lang="ts">
  let theme = $state(localStorage.getItem('nabaos-theme') || 'system');

  function getSystemTheme(): 'light' | 'dark' {
    return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
  }

  function applyTheme(t: string) {
    const resolved = t === 'system' ? getSystemTheme() : t;
    document.documentElement.setAttribute('data-theme', resolved);
  }

  function toggle() {
    const order = ['dark', 'light', 'system'];
    const idx = order.indexOf(theme);
    theme = order[(idx + 1) % order.length];
    localStorage.setItem('nabaos-theme', theme);
    applyTheme(theme);
  }

  $effect(() => { applyTheme(theme); });

  $effect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: light)');
    const handler = () => { if (theme === 'system') applyTheme('system'); };
    mq.addEventListener('change', handler);
    return () => mq.removeEventListener('change', handler);
  });
</script>

<button class="theme-toggle" onclick={toggle} title="Theme: {theme}">
  {#if theme === 'dark'}
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>
  {:else if theme === 'light'}
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>
  {:else}
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="20" height="14" rx="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>
  {/if}
  <span class="theme-label">{theme}</span>
</button>

<style>
  .theme-toggle { display: flex; align-items: center; gap: 8px; background: none; border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 6px 10px; color: var(--text-dim); cursor: pointer; font-size: 0.78rem; text-transform: capitalize; transition: all var(--transition-fast); width: 100%; }
  .theme-toggle:hover { background: var(--bg-hover); color: var(--text); }
  .theme-label { flex: 1; text-align: left; }
</style>
