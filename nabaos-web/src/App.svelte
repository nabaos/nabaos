<script lang="ts">
  import { onMount } from 'svelte';
  import { checkAuth, isLoggedIn, login, logout } from './lib/api';
  import { currentPage, authenticated, toasts, showToast } from './lib/stores';
  import type { Page } from './lib/stores';
  import { ThemeToggle, Toast } from './lib/components';
  import Dashboard from './routes/Dashboard.svelte';
  import Chat from './routes/Chat.svelte';
  import Workflows from './routes/Workflows.svelte';
  import Settings from './routes/Settings.svelte';
  import Pea from './routes/Pea.svelte';
  import Watcher from './routes/Watcher.svelte';

  let isAuth = $state(false);
  let password = $state('');
  let loginError = $state('');
  let loading = $state(true);
  let page = $state<Page>('chat');
  let toastList = $state<{ id: number; message: string; variant: 'success' | 'error' | 'info' }[]>([]);

  const pages: { id: Page; label: string }[] = [
    { id: 'chat', label: 'Chat' },
    { id: 'dashboard', label: 'Dashboard' },
    { id: 'pea', label: 'PEA' },
    { id: 'workflows', label: 'Workflows' },
    { id: 'watcher', label: 'Watcher' },
    { id: 'settings', label: 'Settings' },
  ];

  const mobilePages: { id: Page; label: string }[] = [
    { id: 'chat', label: 'Chat' },
    { id: 'dashboard', label: 'Dashboard' },
    { id: 'pea', label: 'PEA' },
    { id: 'workflows', label: 'Workflows' },
    { id: 'watcher', label: 'Watcher' },
    { id: 'settings', label: 'Settings' },
  ];

  currentPage.subscribe((v) => (page = v));
  toasts.subscribe((v) => (toastList = v));

  onMount(async () => {
    try {
      const status = await checkAuth();
      if (!status.auth_required || (status.auth_required && isLoggedIn() && status.authenticated)) {
        isAuth = true;
        authenticated.set(true);
      }
    } catch {
      if (isLoggedIn()) {
        isAuth = true;
        authenticated.set(true);
      }
    }
    loading = false;
  });

  async function handleLogin() {
    loginError = '';
    try {
      const ok = await login(password);
      if (ok) {
        isAuth = true;
        authenticated.set(true);
      } else {
        loginError = 'Login failed';
      }
    } catch (e: any) {
      loginError = e.message || 'Login failed';
    }
  }

  async function handleLogout() {
    await logout();
    isAuth = false;
    authenticated.set(false);
    password = '';
  }

  function navigate(p: Page) {
    currentPage.set(p);
  }

  function dismissToast(id: number) {
    toasts.update(t => t.filter(item => item.id !== id));
  }
</script>

{#if loading}
  <div class="login-page">
    <p class="loading">Loading...</p>
  </div>
{:else if !isAuth}
  <div class="login-page">
    <div class="login-box">
      <h1>NabaOS</h1>
      <p>Secure agent runtime — enter password to continue</p>
      <form onsubmit={(e) => { e.preventDefault(); handleLogin(); }}>
        <input type="password" placeholder="Password" bind:value={password} />
        {#if loginError}
          <p class="error-text">{loginError}</p>
        {/if}
        <button class="btn btn-primary" type="submit">Sign In</button>
      </form>
    </div>
  </div>
{:else}
  <div class="app-layout">
    <aside class="sidebar">
      <div class="sidebar-logo">NabaOS</div>
      <nav>
        {#each pages as p}
          <button class={page === p.id ? 'active' : ''} onclick={() => navigate(p.id)}>
            {#if p.id === 'chat'}
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/></svg>
            {:else if p.id === 'dashboard'}
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>
            {:else if p.id === 'pea'}
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="11" width="18" height="10" rx="2"/><circle cx="12" cy="5" r="4"/><line x1="8" y1="16" x2="8" y2="16.01"/><line x1="16" y1="16" x2="16" y2="16.01"/></svg>
            {:else if p.id === 'workflows'}
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>
            {:else if p.id === 'watcher'}
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
            {:else if p.id === 'settings'}
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
            {/if}
            <span class="nav-label">{p.label}</span>
          </button>
        {/each}
      </nav>
      <div class="sidebar-bottom">
        <ThemeToggle />
        <button class="btn btn-danger btn-sm" style="width:100%; margin-top: 0.5rem;" onclick={handleLogout}>
          Logout
        </button>
      </div>
    </aside>
    <main class="main-content">
      {#key page}
        <div class="page-container fade-in">
          {#if page === 'chat'}
            <Chat />
          {:else if page === 'dashboard'}
            <Dashboard />
          {:else if page === 'pea'}
            <Pea />
          {:else if page === 'workflows'}
            <Workflows />
          {:else if page === 'watcher'}
            <Watcher />
          {:else if page === 'settings'}
            <Settings />
          {/if}
        </div>
      {/key}
    </main>
  </div>

  <nav class="mobile-nav">
    {#each mobilePages as p}
      <button class={page === p.id ? 'active' : ''} onclick={() => navigate(p.id)}>
        {#if p.id === 'chat'}
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/></svg>
        {:else if p.id === 'dashboard'}
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>
        {:else if p.id === 'pea'}
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="11" width="18" height="10" rx="2"/><circle cx="12" cy="5" r="4"/><line x1="8" y1="16" x2="8" y2="16.01"/><line x1="16" y1="16" x2="16" y2="16.01"/></svg>
        {:else if p.id === 'workflows'}
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>
        {:else if p.id === 'watcher'}
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
        {:else if p.id === 'settings'}
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
        {/if}
        <span>{p.label}</span>
      </button>
    {/each}
  </nav>
{/if}

{#if toastList.length > 0}
  <div class="toast-container">
    {#each toastList as t (t.id)}
      <Toast message={t.message} variant={t.variant} onclose={() => dismissToast(t.id)} />
    {/each}
  </div>
{/if}

<style>
  .sidebar nav button {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .sidebar nav button svg {
    flex-shrink: 0;
    opacity: 0.6;
  }

  .sidebar nav button.active svg {
    opacity: 1;
  }

  .sidebar nav button:hover svg {
    opacity: 0.85;
  }

  .mobile-nav button {
    flex-direction: column;
    font-size: 0.65rem;
    gap: 2px;
  }

  .mobile-nav button svg {
    width: 20px;
    height: 20px;
  }

  .page-container {
    animation: fadeIn 0.2s ease;
  }
</style>
