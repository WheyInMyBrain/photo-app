<!-- photo-app/frontend/src/lib/components/BatchActionBar.svelte -->
<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { filterStore } from '$lib/stores/filterStore';
  import { fly } from 'svelte/transition';

  export let count = 0;
  export let isActionLoading = false;

  const dispatch = createEventDispatcher<{
    toggleDelete: void;
    purge: void;
    addToAlbum: void;
    share: void;
    download: void;
    clear: void;
  }>();

  $: inTrash = $filterStore.show_trash;
</script>

{#if count > 0}
  <aside
    transition:fly={{ y: 30, duration: 220 }}
    style="bottom: max(5.5rem, calc(var(--sab) + 4.5rem));"
    class="fixed left-1/2 -translate-x-1/2 z-40 max-w-[95vw] sm:max-w-xl w-full select-none"
    aria-label="Bulk selection actions"
  >
    <div class="glass-pill px-3.5 py-2.5 rounded-2xl shadow-2xl flex items-center justify-between gap-2 border border-[var(--border-glass)] backdrop-blur-2xl bg-[var(--card-bg)]/85">
      <!-- Item Counter -->
      <div class="flex items-center gap-2 min-w-0 pr-1">
        <span class="w-2 h-2 rounded-full bg-purple-500 animate-pulse flex-shrink-0"></span>
        <span class="text-xs font-semibold tracking-tight text-[var(--text-main)] truncate">
          {count} selected
        </span>
      </div>

      <!-- Action Cluster -->
      <div class="flex items-center gap-1.5 flex-shrink-0">
        {#if inTrash}
          <!-- Restore Button -->
          <button
            type="button"
            disabled={isActionLoading}
            on:click={() => dispatch('toggleDelete')}
            class="px-2.5 py-1.5 rounded-xl bg-emerald-500/15 border border-emerald-500/25 hover:bg-emerald-500/25 text-xs font-semibold text-emerald-600 dark:text-emerald-400 transition-all spring-tap cursor-pointer disabled:opacity-40 flex items-center gap-1.5"
            title="Restore selected items"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="1 4 1 10 7 10"></polyline>
              <path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10"></path>
            </svg>
            <span>Restore</span>
          </button>

          <!-- Purge Button -->
          <button
            type="button"
            disabled={isActionLoading}
            on:click={() => dispatch('purge')}
            class="px-2.5 py-1.5 rounded-xl bg-red-600 hover:bg-red-500 text-white text-xs font-semibold shadow-md shadow-red-600/25 transition-all spring-tap cursor-pointer disabled:opacity-40 flex items-center gap-1.5"
            title="Permanently purge selected items"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="3 6 5 6 21 6"></polyline>
              <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
              <line x1="10" y1="11" x2="10" y2="17"></line>
              <line x1="14" y1="11" x2="14" y2="17"></line>
            </svg>
            <span>Purge</span>
          </button>
        {:else}
          <!-- Share Button -->
          <button
            type="button"
            disabled={isActionLoading}
            on:click={() => dispatch('share')}
            class="p-2 sm:px-2.5 sm:py-1.5 rounded-xl bg-[var(--border-glass)]/50 hover:bg-[var(--border-glass)] text-[var(--text-main)] text-xs font-semibold transition-all spring-tap cursor-pointer disabled:opacity-40 flex items-center gap-1.5"
            title="Share selected"
            aria-label="Share selected"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M8.684 13.342C8.886 12.938 9 12.482 9 12c0-.482-.114-.938-.316-1.342m0 2.684a3 3 0 110-2.684m0 2.684l6.632 3.316m-6.632-6l6.632-3.316m0 0a3 3 0 105.367-2.684 3 3 0 00-5.367 2.684zm0 9.316a3 3 0 105.368 2.684 3 3 0 00-5.368-2.684z" />
            </svg>
            <span class="hidden sm:inline">Share</span>
          </button>

          <!-- Download Button -->
          <button
            type="button"
            disabled={isActionLoading}
            on:click={() => dispatch('download')}
            class="p-2 sm:px-2.5 sm:py-1.5 rounded-xl bg-[var(--border-glass)]/50 hover:bg-[var(--border-glass)] text-[var(--text-main)] text-xs font-semibold transition-all spring-tap cursor-pointer disabled:opacity-40 flex items-center gap-1.5"
            title="Download selected"
            aria-label="Download selected"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
              <polyline points="7 10 12 15 17 10"></polyline>
              <line x1="12" y1="15" x2="12" y2="3"></line>
            </svg>
            <span class="hidden sm:inline">Download</span>
          </button>

          <!-- Add to Album Button -->
          <button
            type="button"
            disabled={isActionLoading}
            on:click={() => dispatch('addToAlbum')}
            class="px-2.5 py-1.5 rounded-xl bg-purple-500/15 border border-purple-500/25 hover:bg-purple-500/25 text-purple-600 dark:text-purple-300 text-xs font-semibold transition-all spring-tap cursor-pointer disabled:opacity-40 flex items-center gap-1.5"
            title="Add to Album"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
            </svg>
            <span class="hidden sm:inline">Add to Album</span>
          </button>

          <!-- Move to Trash Button -->
          <button
            type="button"
            disabled={isActionLoading}
            on:click={() => dispatch('toggleDelete')}
            class="px-2.5 py-1.5 rounded-xl bg-red-500/15 border border-red-500/25 hover:bg-red-500/25 text-red-600 dark:text-red-300 text-xs font-semibold transition-all spring-tap cursor-pointer disabled:opacity-40 flex items-center gap-1.5"
            title="Move to Trash"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="3 6 5 6 21 6"></polyline>
              <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
            </svg>
            <span class="hidden sm:inline">Trash</span>
          </button>
        {/if}

        <!-- Deselect All Button -->
        <button
          type="button"
          disabled={isActionLoading}
          on:click={() => dispatch('clear')}
          class="p-1.5 rounded-full text-[var(--text-muted)] hover:text-[var(--text-main)] transition-colors spring-tap cursor-pointer ml-0.5"
          title="Deselect All"
          aria-label="Deselect all"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18"></line>
            <line x1="6" y1="6" x2="18" y2="18"></line>
          </svg>
        </button>
      </div>
    </div>
  </aside>
{/if}