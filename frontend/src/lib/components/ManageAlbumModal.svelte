<!-- frontend/src/lib/components/ManageAlbumModal.svelte -->
<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { albumStore } from '$lib/stores/albumStore';
  import { filterStore } from '$lib/stores/filterStore';

  export let isOpen = false;
  export let album: {
    id: string;
    title: string;
    description?: string | null;
    media_count: number;
  } | null = null;

  const dispatch = createEventDispatcher<{
    close: void;
    updated: void;
    deleted: void;
  }>();

  let title = '';
  let description = '';
  let isSaving = false;
  let showDeleteConfirm = false;

  $: if (album) {
    title = album.title;
    description = album.description || '';
    showDeleteConfirm = false;
  }

  async function handleSave() {
    if (!album || !title.trim() || isSaving) return;
    isSaving = true;
    const ok = await albumStore.renameAlbum(album.id, title.trim(), description.trim() || undefined);
    isSaving = false;
    if (ok) {
      dispatch('updated');
      dispatch('close');
    }
  }

  async function handleDelete(deleteMedia: boolean) {
    if (!album || isSaving) return;
    isSaving = true;
    const ok = await albumStore.removeAlbum(album.id, deleteMedia);
    isSaving = false;
    if (ok) {
      filterStore.clearAlbum();
      dispatch('deleted');
      dispatch('close');
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !isSaving) {
      if (showDeleteConfirm) {
        showDeleteConfirm = false;
      } else {
        dispatch('close');
      }
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

{#if isOpen && album}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md p-4 select-none"
    on:click|self={() => { if (!isSaving) dispatch('close'); }}
  >
    <div class="liquid-modal rounded-3xl w-full max-w-sm flex flex-col p-6 space-y-4">
      {#if !showDeleteConfirm}
        <div class="flex items-center justify-between pb-1 border-b border-white/[0.08]">
          <h3 class="text-sm font-semibold text-white tracking-tight">Album Options</h3>
          <button
            type="button"
            on:click={() => dispatch('close')}
            class="liquid-icon-btn w-7 h-7 flex items-center justify-center rounded-full text-white/50 hover:text-white transition-all spring-tap cursor-pointer"
            title="Close"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
              <line x1="18" y1="6" x2="6" y2="18"></line>
              <line x1="6" y1="6" x2="18" y2="18"></line>
            </svg>
          </button>
        </div>

        <div class="space-y-3">
          <div class="space-y-1">
            <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40 block pl-1">Title</span>
            <input
              type="text"
              bind:value={title}
              class="liquid-input w-full rounded-xl px-3 py-2 text-xs text-white outline-none"
              placeholder="Album title"
            />
          </div>

          <div class="space-y-1">
            <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40 block pl-1">Description</span>
            <textarea
              bind:value={description}
              rows="2"
              class="liquid-input w-full rounded-xl px-3 py-2 text-xs text-white outline-none resize-none"
              placeholder="Optional description"
            ></textarea>
          </div>
        </div>

        <div class="pt-2 flex flex-col gap-2">
          <button
            type="button"
            on:click={handleSave}
            disabled={!title.trim() || isSaving}
            class="liquid-btn-primary py-2 rounded-xl text-xs font-medium text-white spring-tap disabled:opacity-40 cursor-pointer"
          >
            {isSaving ? 'Saving...' : 'Save Changes'}
          </button>

          <button
            type="button"
            on:click={() => (showDeleteConfirm = true)}
            class="py-2 text-xs text-rose-400/80 hover:text-rose-400 font-medium transition-colors spring-tap cursor-pointer text-center"
          >
            Delete Album...
          </button>
        </div>
      {:else}
        <!-- Delete Confirmation View -->
        <div class="text-center space-y-2 py-2">
          <h3 class="text-sm font-semibold text-white">Delete "{album.title}"?</h3>
          <p class="text-xs text-white/50 leading-relaxed">
            Choose whether to delete only this album container or move all {album.media_count} items to the trash.
          </p>
        </div>

        <div class="flex flex-col gap-2 pt-2">
          <button
            type="button"
            on:click={() => handleDelete(false)}
            class="liquid-card py-2.5 px-4 rounded-xl text-xs text-white/90 hover:text-white font-medium text-left flex justify-between items-center spring-tap cursor-pointer"
          >
            <span>Delete Album Only</span>
            <span class="text-[10px] text-white/40">Keeps photos</span>
          </button>

          <button
            type="button"
            on:click={() => handleDelete(true)}
            class="liquid-trash-btn py-2.5 px-4 rounded-xl text-xs text-rose-300 font-medium text-left flex justify-between items-center spring-tap cursor-pointer"
          >
            <span>Delete Album and Media</span>
            <span class="text-[10px] text-rose-400/60">Trash {album.media_count} items</span>
          </button>

          <button
            type="button"
            on:click={() => (showDeleteConfirm = false)}
            class="py-1.5 text-xs text-white/40 hover:text-white transition-colors cursor-pointer text-center"
          >
            Back
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .liquid-modal {
    background: rgba(18, 18, 22, 0.85);
    border: 1px solid rgba(255, 255, 255, 0.12);
    backdrop-filter: blur(40px) saturate(180%);
    box-shadow:
      0 30px 70px rgba(0, 0, 0, 0.75),
      inset 0 1px 0 rgba(255, 255, 255, 0.2);
  }

  .liquid-input {
    background: rgba(0, 0, 0, 0.4);
    border: 1px solid rgba(255, 255, 255, 0.12);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.4);
  }

  .liquid-input:focus {
    border-color: rgba(255, 255, 255, 0.25);
  }

  .liquid-btn-primary {
    background: rgba(255, 255, 255, 0.16);
    border: 1px solid rgba(255, 255, 255, 0.24);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.3), inset 0 1px 0 rgba(255, 255, 255, 0.35);
  }

  .liquid-card {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
  }

  .liquid-card:hover {
    background: rgba(255, 255, 255, 0.08);
  }

  .liquid-trash-btn {
    background: rgba(244, 63, 94, 0.12);
    border: 1px solid rgba(244, 63, 94, 0.25);
  }

  .liquid-trash-btn:hover {
    background: rgba(244, 63, 94, 0.18);
  }

  .liquid-icon-btn {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.12);
  }
</style>