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

  onMount(() => {
    albumStore.load();
  });

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

<div
  transition:fade={{ duration: 150 }}
  class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm"
  on:click|self={() => dispatch('close')}
  role="dialog"
  aria-modal="true"
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
        class="text-xs text-[var(--text-muted)] hover:text-white transition-colors"
      >
        ✕
      </button>
    </div>

    {#if errorMsg}
      <p class="text-xs text-red-400 bg-red-500/10 px-2 py-1 rounded">{errorMsg}</p>
    {/if}

    <p class="text-xs text-[var(--text-muted)]">
      Adding {assetIds.length} selected {assetIds.length === 1 ? 'item' : 'items'}
    </p>

    {#if !isCreating}
      <div class="max-h-60 overflow-y-auto space-y-1.5 pr-1">
        {#each $albumStore as album (album.id)}
          <button
            type="button"
            disabled={isSubmitting}
            on:click={() => handleSelectAlbum(album.id)}
            class="w-full flex items-center gap-3 p-2 rounded-xl hover:bg-white/5 border border-transparent hover:border-[var(--border-glass)] transition text-left cursor-pointer group disabled:opacity-50"
          >
            <div class="w-10 h-10 rounded-lg overflow-hidden bg-white/5 flex-shrink-0 flex items-center justify-center border border-white/10">
              {#if album.cover_thumb}
                <img
                  src={album.cover_thumb.startsWith('/') ? album.cover_thumb : `/${album.cover_thumb}`}
                  alt=""
                  class="w-full h-full object-cover"
                />
              {:else}
                <span class="text-sm opacity-40">📁</span>
              {/if}
            </div>

            <div class="flex-1 min-w-0">
              <div class="text-xs font-medium truncate group-hover:text-purple-400 transition-colors">
                {album.title}
              </div>
              <div class="text-[10px] text-[var(--text-muted)]">
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
        class="w-full py-2 px-3 rounded-xl border border-dashed border-[var(--border-glass)] text-xs text-purple-400 hover:bg-purple-500/10 transition-colors flex items-center justify-center gap-1.5 font-medium cursor-pointer"
      >
        <span>+</span>
        <span>Create New Album</span>
      </button>
    {:else}
      <form on:submit|preventDefault={handleCreateAndAdd} class="space-y-3">
        <input
          type="text"
          bind:value={newTitle}
          placeholder="Album Title..."
          class="w-full text-xs px-3 py-2 rounded-xl bg-white/5 border border-[var(--border-glass)] text-[var(--text-main)] placeholder-[var(--text-muted)] focus:outline-none focus:border-purple-500"
          autofocus
        />

        <div class="flex items-center gap-2 justify-end pt-1">
          <button
            type="button"
            on:click={() => (isCreating = false)}
            class="px-3 py-1.5 text-xs text-[var(--text-muted)] hover:text-white transition cursor-pointer"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!newTitle.trim() || isSubmitting}
            class="px-3 py-1.5 text-xs bg-purple-600 hover:bg-purple-500 text-white font-medium rounded-xl transition cursor-pointer disabled:opacity-50"
          >
            {isSubmitting ? 'Creating...' : 'Create & Add'}
          </button>
        </div>
      </form>
    {/if}
  </div>
</div>