// Svelte 5 shared reactive state using $state runes
// This file uses .svelte.ts extension to enable runes in module context

export type Page = 'chat' | 'dashboard' | 'pea' | 'outputs' | 'workflows' | 'watcher' | 'settings';

// ── App state ────────────────────────────────────────────────────────
export let appState = $state({
  page: 'chat' as Page,
  authenticated: false,
});

export function navigateTo(p: Page) {
  appState.page = p;
}

// ── Toast notifications ──────────────────────────────────────────────
export interface ToastItem {
  id: number;
  message: string;
  variant: 'success' | 'error' | 'info';
}

export let toastState = $state<{ items: ToastItem[] }>({ items: [] });

let toastId = 0;
export function showToast(message: string, variant: 'success' | 'error' | 'info' = 'info') {
  const id = ++toastId;
  toastState.items = [...toastState.items, { id, message, variant }];
  setTimeout(() => {
    toastState.items = toastState.items.filter(item => item.id !== id);
  }, 3000);
}

export function dismissToast(id: number) {
  toastState.items = toastState.items.filter(item => item.id !== id);
}
