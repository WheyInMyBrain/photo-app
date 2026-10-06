<!-- photo-app/frontend/src/lib/components/ManageAlbumModal.svelte -->
<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { albumStore } from '$lib/stores/albumStore';
  import { filterStore } from '$lib/stores/filterStore';
  import { fade, scale } from 'svelte/transition';
  import type { AlbumRecord } from '$lib/api/albums';

  export let isOpen = false;
  export let album: (AlbumRecord | {
    id: string;
    title: string;
    description?: string | null;
    media_count: number;
  }) | null = null;

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
    try {
      const ok = await albumStore.renameAlbum(album.id, title.trim(), description.trim() || undefined);
      if (ok) {
        window.dispatchEvent(new CustomEvent('vault:refresh-timeline'));
        dispatch('updated');
        dispatch('close');
      }
    } catch (err) {
      console.error('Failed to rename album:', err);
    } finally {
      isSaving = false;
    }
  }

  async function handleDelete(deleteMedia: boolean) {
    if (!album || isSaving) return;
    isSaving = true;
    try {
      const ok = await albumStore.removeAlbum(album.id, deleteMedia);
      if (ok) {
        filterStore.clearAlbum();
        filterStore.setFolderPath('');
        window.dispatchEvent(new CustomEvent('vault:refresh-timeline'));
        dispatch('deleted');
        dispatch('close');
      }
    } catch (err) {
      console.error('Failed to delete album:', err);
    } finally {
      isSaving = false;
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
  <div
    transition:fade={{ duration: 150 }}
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md p-4 select-none"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    on:click|self={() => { if (!isSaving) dispatch('close'); }}
  >
    <div
      transition:scale={{ start: 0.95, duration: 150 }}
      class="liquid-modal rounded-3xl w-full max-w-sm flex flex-col p-6 space-y-4 text-[var(--text-main)]"
    >
      {#if !showDeleteConfirm}
        <div class="flex items-center justify-between pb-2 border-b border-[var(--border-glass)]">
          <h3 class="text-sm font-semibold tracking-tight text-[var(--text-main)]">Album Options</h3>
          <button
            type="button"
            on:click={() => dispatch('close')}
            class="liquid-icon-btn w-7 h-7 flex items-center justify-center rounded-full text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
            title="Close"
            aria-label="Close modal"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
              <line x1="18" y1="6" x2="6" y2="18"></line>
              <line x1="6" y1="6" x2="18" y2="18"></line>
            </svg>
          </button>
        </div>

        <div class="space-y-3">
          <div class="space-y-1">
            <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-1">Title</span>
            <input
              type="text"
              bind:value={title}
              class="liquid-input w-full rounded-xl px-3.5 py-2 text-xs text-[var(--text-main)] outline-none"
              placeholder="Album title"
            />
          </div>

          <div class="space-y-1">
            <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-1">Description</span>
            <textarea
              bind:value={description}
              rows="2"
              class="liquid-input w-full rounded-xl px-3.5 py-2 text-xs text-[var(--text-main)] outline-none resize-none"
              placeholder="Optional description"
            ></textarea>
          </div>
        </div>

        <div class="pt-2 flex flex-col gap-2">
          <button
            type="button"
            on:click={handleSave}
            disabled={!title.trim() || isSaving}
            class="liquid-btn-primary py-2.5 rounded-xl text-xs font-semibold text-white spring-tap disabled:opacity-40 cursor-pointer"
          >
            {isSaving ? 'Saving...' : 'Save Changes'}
          </button>

          <button
            type="button"
            on:click={() => (showDeleteConfirm = true)}
            class="py-2 text-xs text-rose-500 dark:text-rose-400 hover:text-rose-600 dark:hover:text-rose-300 font-medium transition-colors spring-tap cursor-pointer text-center"
          >
            Delete Album...
          </button>
        </div>
      {:else}
        <!-- Delete Confirmation View -->
        <div class="text-center space-y-2 py-2">
          <div class="w-10 h-10 mx-auto rounded-full bg-rose-500/10 border border-rose-500/20 text-rose-500 flex items-center justify-center">
            <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="3 6 5 6 21 6"></polyline>
              <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
            </svg>
          </div>
          <h3 class="text-sm font-semibold text-[var(--text-main)]">Delete "{album.title}"?</h3>
          <p class="text-xs text-[var(--text-muted)] leading-relaxed">
            Choose whether to delete only this album container or move all {album.media_count} items to the trash.
          </p>
        </div>

        <div class="flex flex-col gap-2 pt-2">
          <button
            type="button"
            disabled={isSaving}
            on:click={() => handleDelete(false)}
            class="liquid-card py-2.5 px-4 rounded-xl text-xs text-[var(--text-main)] font-medium text-left flex justify-between items-center spring-tap cursor-pointer disabled:opacity-50"
          >
            <span>{isSaving ? 'Deleting...' : 'Delete Album Only'}</span>
            <span class="text-[10px] text-[var(--text-muted)] font-mono">Keeps photos</span>
          </button>

          <button
            type="button"
            disabled={isSaving}
            on:click={() => handleDelete(true)}
            class="liquid-trash-btn py-2.5 px-4 rounded-xl text-xs font-semibold text-rose-600 dark:text-rose-300 text-left flex justify-between items-center spring-tap cursor-pointer disabled:opacity-50"
          >
            <span>{isSaving ? 'Trashing...' : 'Delete Album and Media'}</span>
            <span class="text-[10px] text-rose-500/70 font-mono">Trash {album.media_count} items</span>
          </button>

          <button
            type="button"
            disabled={isSaving}
            on:click={() => (showDeleteConfirm = false)}
            class="py-1.5 text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] transition-colors cursor-pointer text-center disabled:opacity-50"
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
    background: var(--bg-surface-elevated);
    border: 1px solid var(--border-glass);
    backdrop-filter: blur(40px) saturate(190%);
    -webkit-backdrop-filter: blur(40px) saturate(190%);
    box-shadow:
      0 30px 70px var(--dock-shadow),
      inset 0 1px 0 0 var(--border-specular);
  }

  .liquid-input {
    background: var(--card-bg);
    border: 1px solid var(--border-glass);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.08);
  }

  .liquid-input:focus {
    border-color: var(--border-subtle);
    box-shadow: 0 0 0 1px var(--border-subtle);
  }

  .liquid-btn-primary {
    background: #9333ea;
    box-shadow: 0 4px 14px rgba(147, 51, 234, 0.35);
  }

  .liquid-btn-primary:hover:not(:disabled) {
    background: #7e22ce;
  }

  .liquid-card {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
  }

  .liquid-card:hover:not(:disabled) {
    background: var(--dock-bg-hover);
    border-color: var(--border-subtle);
  }

  .liquid-trash-btn {
    background: rgba(244, 63, 94, 0.12);
    border: 1px solid rgba(244, 63, 94, 0.25);
  }

  .liquid-trash-btn:hover:not(:disabled) {
    background: rgba(244, 63, 94, 0.18);
  }

  .liquid-icon-btn {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
  }

  .liquid-icon-btn:hover:not(:disabled) {
    background: var(--dock-bg-hover);
    border-color: var(--border-subtle);
  }
</style>