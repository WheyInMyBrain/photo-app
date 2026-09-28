<!-- photo-app/frontend/src/lib/components/AlbumsHeaderView.svelte -->
<script lang="ts">
  import { albumStore } from '$lib/stores/albumStore';
  import { filterStore } from '$lib/stores/filterStore';
  import { createEventDispatcher } from 'svelte';

  const dispatch = createEventDispatcher<{
    openCreate: void;
  }>();

  $: activeAlbum = $albumStore.find((a) => a.id ===$filterStore.album_id);
</script>

<div class="space-y-4 pt-1 pb-2">
  <!-- Albums Breadcrumb & Toolbar -->
  <div class="flex items-center justify-between gap-2 px-1">
    <div class="flex items-center gap-2 text-xs">
      <button
        type="button"
        on:click={() => filterStore.clearAlbum()}
        class="font-semibold transition-colors cursor-pointer {$filterStore.album_id ? 'text-[var(--text-muted)] hover:text-[var(--text-main)]' : 'text-purple-600 dark:text-purple-400'}"
      >
        All Albums
      </button>

      {#if activeAlbum}
        <span class="text-[var(--text-muted)] opacity-40">/</span>
        <div class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full liquid-pill text-xs font-medium text-[var(--text-main)]">
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 text-purple-600 dark:text-purple-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
          </svg>
          <span class="truncate max-w-[200px]">{activeAlbum.title}</span>
          <button
            type="button"
            on:click={() => filterStore.clearAlbum()}
            class="text-[var(--text-muted)] hover:text-[var(--text-main)] transition-colors cursor-pointer ml-1 p-0.5"
            title="Exit album"
            aria-label="Exit album"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <line x1="18" y1="6" x2="6" y2="18"></line>
              <line x1="6" y1="6" x2="18" y2="18"></line>
            </svg>
          </button>
        </div>
      {/if}
    </div>

    <!-- Create Album Button -->
    <button
      type="button"
      on:click={() => dispatch('openCreate')}
      class="text-xs px-3 py-1.5 rounded-xl bg-purple-500/10 hover:bg-purple-500/20 text-purple-600 dark:text-purple-300 border border-purple-500/25 font-medium transition cursor-pointer flex items-center gap-1.5 spring-tap"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
        <line x1="12" y1="5" x2="12" y2="19"></line>
        <line x1="5" y1="12" x2="19" y2="12"></line>
      </svg>
      <span>New Album</span>
    </button>
  </div>

  <!-- Album Cards Shelf (Visible when not drilling inside an album) -->
  {#if !$filterStore.album_id}
    {#if $albumStore.length === 0}
      <div class="py-10 text-center border border-dashed border-[var(--border-glass)] rounded-2xl bg-[var(--card-bg)]">
        <div class="w-8 h-8 mx-auto mb-2 text-[var(--text-muted)] opacity-50">
          <svg xmlns="http://www.w3.org/2000/svg" class="w-full h-full" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
          </svg>
        </div>
        <p class="text-xs text-[var(--text-muted)]">No albums yet. Tap "New Album" or select photos to create one.</p>
      </div>
    {:else}
      <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-3">
        {#each $albumStore as album (album.id)}
          <button
            type="button"
            on:click={() => filterStore.setAlbumId(album.id)}
            class="group text-left p-2.5 rounded-2xl bg-[var(--card-bg)] hover:bg-[var(--dock-bg-hover)] border border-[var(--border-glass)] hover:border-purple-500/40 transition-all cursor-pointer flex flex-col gap-2.5 spring-tap"
          >
            <div class="w-full aspect-square rounded-xl overflow-hidden bg-black/10 dark:bg-black/40 flex items-center justify-center border border-[var(--border-glass)] relative">
              {#if album.cover_thumb}
                <img
                  src={album.cover_thumb.startsWith('/') ? album.cover_thumb : `/${album.cover_thumb}`}
                  alt={album.title}
                  class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                />
              {:else}
                <div class="w-8 h-8 text-[var(--text-muted)] opacity-40">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-full h-full" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                  </svg>
                </div>
              {/if}

              <div class="absolute bottom-1.5 right-1.5 bg-black/60 dark:bg-black/75 backdrop-blur-md px-1.5 py-0.5 rounded-md text-[9px] text-white font-mono shadow-sm">
                {album.media_count}
              </div>
            </div>

            <div class="min-w-0 px-0.5">
              <div class="text-xs font-semibold truncate text-[var(--text-main)] group-hover:text-purple-600 dark:group-hover:text-purple-400 transition-colors">
                {album.title}
              </div>
              <div class="text-[10px] text-[var(--text-muted)] font-mono">
                {album.media_count} {album.media_count === 1 ? 'item' : 'items'}
              </div>
            </div>
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .liquid-pill {
    background: var(--dock-bg);
    border: 1px solid var(--dock-border);
    backdrop-filter: blur(20px) saturate(180%);
    -webkit-backdrop-filter: blur(20px) saturate(180%);
    box-shadow: 0 4px 14px var(--dock-shadow), inset 0 1px 0 var(--dock-highlight);
  }
</style>