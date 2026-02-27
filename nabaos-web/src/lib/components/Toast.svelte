<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    message: string;
    variant?: 'success' | 'error' | 'info';
    duration?: number;
    onclose: () => void;
  }
  let { message, variant = 'info', duration = 3000, onclose }: Props = $props();

  onMount(() => {
    const timer = setTimeout(onclose, duration);
    return () => clearTimeout(timer);
  });
</script>

<div class="toast toast-{variant} slide-up" role="alert">
  <span class="toast-msg">{message}</span>
  <button class="toast-close" onclick={onclose}>×</button>
</div>

<style>
  .toast { display: flex; align-items: center; gap: 0.75rem; padding: 0.75rem 1rem; border-radius: var(--radius-md); background: var(--bg-card); border: 1px solid var(--border); box-shadow: var(--shadow-lg); font-size: 0.88rem; min-width: 280px; max-width: 400px; }
  .toast-success { border-left: 3px solid var(--success); }
  .toast-error { border-left: 3px solid var(--danger); }
  .toast-info { border-left: 3px solid var(--accent); }
  .toast-msg { flex: 1; }
  .toast-close { background: none; border: none; color: var(--text-dim); cursor: pointer; font-size: 1.2rem; padding: 0; line-height: 1; }
  .toast-close:hover { color: var(--text); }
</style>
