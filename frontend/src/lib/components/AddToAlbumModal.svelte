<!-- frontend/src/lib/components/AddToAlbumModal.svelte -->
<script lang="ts">
  import { onMount, createEventDispatcher } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import { albumStore } from '$lib/stores/albumStore';

  export let assetIds: string[] = [];

  const dispatch = createEventDispatcher<{
    close: void;
    completed: { albumId: string; count: number };
  }>();

  let isCreating = false;
  let newTitle = '';
  let isSubmitting = false;
  let errorMsg = '';

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  onMount(() => {
    albumStore.load();
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !isSubmitting) {
      dispatch('close');
    }
  }

  async function handleSelectAlbum(albumId: string) {
    if (isSubmitting) return;
    isSubmitting = true;
    errorMsg = '';
    try {
      await albumStore.addItems(albumId, assetIds);
      dispatch('completed', { albumId, count: assetIds.length });
    } catch (e: any) {
      errorMsg = e?.message || 'Error adding to album';
      isSubmitting = false;
    }
  }

  async function handleCreateAndAdd() {
    if (!newTitle.trim() || isSubmitting) return;
    isSubmitting = true;
    errorMsg = '';
    try {
      const albumId = await albumStore.create(newTitle.trim());
      await albumStore.addItems(albumId, assetIds);
      dispatch('completed', { albumId, count: assetIds.length });
    } catch (e: any) {
      errorMsg = e?.message || 'Failed to create album';
      isSubmitting = false;
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  transition:fade={{ duration: 150 }}
  class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm"
  on:click|self={() => dispatch('close')}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div
    transition:scale={{ start: 0.95, duration: 150 }}
    class="w-full max-w-sm bg-[var(--bg-primary)] border border-[var(--border-glass)] rounded-2xl p-5 shadow-2xl space-y-4 text-[var(--text-main)]"
  >
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-semibold tracking-tight">Add to Album</h3>
      <button
        type="button"
        on:click={() => dispatch('close')}
        class="text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] transition-colors cursor-pointer p-1"
        aria-label="Close modal"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>
    </div>

    {#if errorMsg}
      <p class="text-xs text-red-500 dark:text-red-400 bg-red-500/10 px-2.5 py-1.5 rounded-xl border border-red-500/20">{errorMsg}</p>
    {/if}

    <p class="text-xs text-[var(--text-muted)]">
      Adding {assetIds.length} selected {assetIds.length === 1 ? 'item' : 'items'}
    </p>

    {#if !isCreating}
      <div class="max-h-60 overflow-y-auto space-y-1.5 pr-1 no-scrollbar">
        {#each $albumStore as album (album.id)}
          <button
            type="button"
            disabled={isSubmitting}
            on:click={() => handleSelectAlbum(album.id)}
            class="w-full flex items-center gap-3 p-2 rounded-xl bg-[var(--card-bg)] hover:bg-[var(--dock-bg-hover)] border border-[var(--border-glass)] hover:border-purple-500/40 transition text-left cursor-pointer group disabled:opacity-50"
          >
            <div class="w-10 h-10 rounded-lg overflow-hidden bg-black/10 dark:bg-black/40 flex-shrink-0 flex items-center justify-center border border-[var(--border-glass)]">
              {#if album.cover_thumb}
                <img
                  src={album.cover_thumb.startsWith('/') ? album.cover_thumb : `/${album.cover_thumb}`}
                  alt=""
                  class="w-full h-full object-cover"
                />
              {:else}
                <div class="w-4 h-4 text-[var(--text-muted)] opacity-50">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-full h-full" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                  </svg>
                </div>
              {/if}
            </div>

            <div class="flex-1 min-w-0">
              <div class="text-xs font-medium truncate group-hover:text-purple-500 dark:group-hover:text-purple-400 transition-colors">
                {album.title}
              </div>
              <div class="text-[10px] text-[var(--text-muted)] font-mono">
                {album.media_count} items
              </div>
            </div>
          </button>
        {:else}
          <div class="text-center py-6 text-xs text-[var(--text-muted)]">
            No albums found.
          </div>
        {/each}
      </div>

      <button
        type="button"
        on:click={() => (isCreating = true)}
        class="w-full py-2.5 px-3 rounded-xl border border-dashed border-[var(--border-glass)] text-xs text-purple-600 dark:text-purple-400 hover:bg-purple-500/10 transition-colors flex items-center justify-center gap-2 font-medium cursor-pointer"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="12" y1="5" x2="12" y2="19"></line>
          <line x1="5" y1="12" x2="19" y2="12"></line>
        </svg>
        <span>Create New Album</span>
      </button>
    {:else}
      <form on:submit|preventDefault={handleCreateAndAdd} class="space-y-3">
        <input
          type="text"
          use:focusOnMount
          bind:value={newTitle}
          placeholder="Album Title..."
          class="w-full text-xs px-3.5 py-2.5 rounded-xl bg-[var(--card-bg)] border border-[var(--border-glass)] text-[var(--text-main)] placeholder-[var(--text-muted)] focus:outline-none focus:border-purple-500"
        />

        <div class="flex items-center gap-2 justify-end pt-1">
          <button
            type="button"
            on:click={() => (isCreating = false)}
            class="px-3 py-1.5 text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] transition cursor-pointer"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!newTitle.trim() || isSubmitting}
            class="px-4 py-1.5 text-xs bg-purple-600 hover:bg-purple-500 text-white font-medium rounded-xl transition cursor-pointer disabled:opacity-50 shadow-sm"
          >
            {isSubmitting ? 'Creating...' : 'Create & Add'}
          </button>
        </div>
      </form>
    {/if}
  </div>
</div>

<style>
  .no-scrollbar::-webkit-scrollbar {
    display: none;
  }
  .no-scrollbar {
    -ms-overflow-style: none;
    scrollbar-width: none;
  }
</style>