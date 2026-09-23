<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { filterStore } from '$lib/stores/filterStore';
  import { fly } from 'svelte/transition';

  export let count = 0;
  export let isActionLoading = false;

  const dispatch = createEventDispatcher<{
    toggleDelete: void;
    purge: void;
    clear: void;
  }>();

  $: inTrash = $filterStore.show_trash;
</script>

{#if count > 0}
  <aside
    transition:fly={{ y: 30, duration: 220 }}
    style="bottom: max(5.5rem, calc(var(--sab) + 4.5rem));"
    class="fixed left-1/2 -translate-x-1/2 z-40 max-w-[92vw] sm:max-w-md w-full select-none"
    aria-label="Bulk selection actions"
  >
    <div class="glass-pill px-4 py-2.5 rounded-2xl shadow-2xl flex items-center justify-between gap-3 border border-[var(--border-glass)]">
      <!-- Item Counter -->
      <div class="flex items-center gap-2 min-w-0">
        <span class="w-2 h-2 rounded-full bg-purple-500 animate-pulse flex-shrink-0"></span>
        <span class="text-xs font-bold tracking-tight text-[var(--text-main)] truncate">
          {count} {count === 1 ? 'selected' : 'selected'}
        </span>
      </div>

      <!-- Action Cluster -->
      <div class="flex items-center gap-1.5 flex-shrink-0">
        {#if inTrash}
          <button
            type="button"
            disabled={isActionLoading}
            on:click={() => dispatch('toggleDelete')}
            class="px-3 py-1.5 rounded-xl glass-panel text-xs font-semibold text-emerald-400 hover:text-emerald-300 transition-all spring-tap cursor-pointer disabled:opacity-40"
          >
            ↺ Restore
          </button>
          <button
            type="button"
            disabled={isActionLoading}
            on:click={() => dispatch('purge')}
            class="px-3 py-1.5 rounded-xl bg-red-600 hover:bg-red-500 text-white text-xs font-semibold shadow-md shadow-red-600/25 transition-all spring-tap cursor-pointer disabled:opacity-40"
          >
            Purge
          </button>
        {:else}
          <button
            type="button"
            disabled={isActionLoading}
            on:click={() => dispatch('toggleDelete')}
            class="px-3 py-1.5 rounded-xl bg-red-500/15 border border-red-500/30 hover:bg-red-500/25 text-red-300 text-xs font-semibold transition-all spring-tap cursor-pointer disabled:opacity-40 flex items-center gap-1.5"
          >
            <span>🗑️</span>
            <span>Move to Trash</span>
          </button>
        {/if}

        <button
          type="button"
          disabled={isActionLoading}
          on:click={() => dispatch('clear')}
          class="p-1.5 rounded-full text-[var(--text-muted)] hover:text-[var(--text-main)] transition-colors spring-tap cursor-pointer"
          title="Deselect All"
          aria-label="Deselect all"
        >
          ✕
        </button>
      </div>
    </div>
  </aside>
{/if}