import { writable } from 'svelte/store';

export type Page = 'chat' | 'dashboard' | 'pea' | 'workflows' | 'watcher' | 'settings';

export const currentPage = writable<Page>('chat');
export const authenticated = writable(false);

// Toast notifications
export interface ToastItem {
  id: number;
  message: string;
  variant: 'success' | 'error' | 'info';
}
export const toasts = writable<ToastItem[]>([]);

let toastId = 0;
export function showToast(message: string, variant: 'success' | 'error' | 'info' = 'info') {
  const id = ++toastId;
  toasts.update(t => [...t, { id, message, variant }]);
  setTimeout(() => {
    toasts.update(t => t.filter(item => item.id !== id));
  }, 3000);
}
