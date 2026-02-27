<script lang="ts">
  import type { Snippet } from 'svelte';
  interface Props { icon?: string; title: string; description: string; children?: Snippet; }
  let { icon = '📭', title, description, children }: Props = $props();
</script>

<div class="empty-state fade-in">
  <div class="empty-icon">{icon}</div>
  <h3 class="empty-title">{title}</h3>
  <p class="empty-desc">{description}</p>
  {#if children}
    <div class="empty-actions">
      {@render children()}
    </div>
  {/if}
</div>

<style>
  .empty-state { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 3rem 1.5rem; text-align: center; }
  .empty-icon { font-size: 3rem; margin-bottom: 1rem; }
  .empty-title { font-size: 1.1rem; font-weight: 600; margin-bottom: 0.5rem; color: var(--text); }
  .empty-desc { color: var(--text-dim); font-size: 0.9rem; max-width: 360px; line-height: 1.5; }
  .empty-actions { margin-top: 1.25rem; display: flex; gap: 0.5rem; flex-wrap: wrap; justify-content: center; }
</style>
