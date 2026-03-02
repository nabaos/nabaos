<script lang="ts">
  import type { Snippet } from 'svelte';
  interface Props { icon?: string; title: string; description: string; children?: Snippet; }
  let { icon = '', title, description, children }: Props = $props();
</script>

<div class="empty-state fade-in">
  {#if icon}
    <div class="empty-icon">{icon}</div>
  {/if}
  <h3 class="empty-title">{title}</h3>
  <p class="empty-desc">{description}</p>
  {#if children}
    <div class="empty-actions">
      {@render children()}
    </div>
  {/if}
</div>

<style>
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 3rem 1.5rem;
    text-align: center;
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    background: linear-gradient(135deg, var(--bg-card) 0%, rgba(124, 111, 255, 0.02) 100%);
  }
  .empty-icon {
    font-size: 2.5rem;
    margin-bottom: 1rem;
    width: 64px;
    height: 64px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--accent-subtle);
    border-radius: 16px;
  }
  .empty-title {
    font-size: 1.05rem;
    font-weight: 600;
    margin-bottom: 0.4rem;
    color: var(--text);
    letter-spacing: -0.01em;
  }
  .empty-desc {
    color: var(--text-dim);
    font-size: 0.88rem;
    max-width: 360px;
    line-height: 1.55;
  }
  .empty-actions {
    margin-top: 1.25rem;
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
    justify-content: center;
  }
</style>
