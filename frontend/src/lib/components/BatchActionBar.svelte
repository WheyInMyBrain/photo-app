<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { filterStore } from '$lib/stores/filterStore';

  export let count = 0;
  export let isActionLoading = false;

  const dispatch = createEventDispatcher<{
    toggleDelete: void;
    purge: void;
    clear: void;
  }>();
</script>

{#if count > 0}
  <div class="fixed bottom-6 left-1/2 -translate-x-1/2 z-40 bg-neutral-900/95 border border-neutral-800 shadow-2xl backdrop-blur-md px-4 py-2 rounded-2xl flex items-center gap-3">
    <span class="text-xs text-neutral-300 font-medium">
      {count} selected
    </span>

    <div class="h-4 w-px bg-neutral-800"></div>

    {#if $filterStore.show_trash}
      <button
        type="button"
        on:click={() => dispatch('toggleDelete')}
        disabled={isActionLoading}
        class="text-xs px-2.5 py-1 bg-neutral-800 hover:bg-neutral-700 text-neutral-200 rounded-lg transition-colors cursor-pointer font-medium disabled:opacity-50"
      >
        ↺ Restore
      </button>

      <button
        type="button"
        on:click={() => dispatch('purge')}
        disabled={isActionLoading}
        class="text-xs px-2.5 py-1 bg-red-900/70 hover:bg-red-800 text-red-200 rounded-lg transition-colors cursor-pointer font-medium disabled:opacity-50"
      >
        Delete Forever
      </button>
    {:else}
      <button
        type="button"
        on:click={() => dispatch('toggleDelete')}
        disabled={isActionLoading}
        class="text-xs px-3 py-1 bg-red-950/80 hover:bg-red-900 text-red-300 border border-red-900/60 rounded-lg transition-colors cursor-pointer font-medium flex items-center gap-1.5 disabled:opacity-50"
      >
        <span>🗑️</span>
        <span>Move to Trash</span>
      </button>
    {/if}

    <button
      type="button"
      on:click={() => dispatch('clear')}
      class="text-xs text-neutral-500 hover:text-white px-1.5 py-1 cursor-pointer"
      title="Deselect All"
    >
      ✕
    </button>
  </div>
{/if}