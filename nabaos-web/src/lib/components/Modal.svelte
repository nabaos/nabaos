<script lang="ts">
  import type { Snippet } from 'svelte';
  interface Props { open: boolean; onclose: () => void; title?: string; children: Snippet; }
  let { open = false, onclose, title, children }: Props = $props();
</script>

{#if open}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="modal-backdrop" onclick={onclose} onkeydown={(e) => e.key === 'Escape' && onclose()} role="dialog" tabindex="-1">
    <div class="modal-content slide-up" onclick={(e) => e.stopPropagation()}>
      {#if title}
        <div class="modal-header">
          <h3>{title}</h3>
          <button class="modal-close" onclick={onclose}>×</button>
        </div>
      {/if}
      {@render children()}
    </div>
  </div>
{/if}

<style>
  .modal-backdrop { position: fixed; inset: 0; background: rgba(0,0,0,0.6); backdrop-filter: blur(4px); display: flex; align-items: center; justify-content: center; z-index: 1000; animation: fadeIn 0.15s ease; }
  .modal-content { background: var(--bg-card); border: 1px solid var(--border); border-radius: var(--radius-lg); padding: 1.5rem; max-width: 480px; width: 90%; max-height: 80vh; overflow-y: auto; box-shadow: var(--shadow-lg); }
  .modal-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; }
  .modal-header h3 { margin: 0; font-size: 1.1rem; font-weight: 600; }
  .modal-close { background: none; border: none; color: var(--text-dim); font-size: 1.5rem; cursor: pointer; padding: 0 4px; line-height: 1; }
  .modal-close:hover { color: var(--text); }
</style>
