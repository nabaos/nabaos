<script lang="ts">
  import type { Snippet } from 'svelte';
  interface Props {
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
    size?: 'sm' | 'md' | 'lg';
    loading?: boolean;
    disabled?: boolean;
    onclick?: (e: MouseEvent) => void;
    type?: 'button' | 'submit';
    children: Snippet;
  }
  let { variant = 'secondary', size = 'md', loading = false, disabled = false, onclick, type = 'button', children }: Props = $props();
</script>

<button
  class="btn btn-{variant} btn-size-{size}"
  {type}
  disabled={disabled || loading}
  {onclick}
>
  {#if loading}
    <span class="btn-spinner"></span>
  {/if}
  {@render children()}
</button>

<style>
  .btn-spinner {
    display: inline-block;
    width: 14px;
    height: 14px;
    border: 2px solid currentColor;
    border-right-color: transparent;
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
    margin-right: 6px;
    vertical-align: middle;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .btn-size-lg { padding: 0.65rem 1.25rem; font-size: 0.95rem; }
</style>
