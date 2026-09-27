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
        class="font-semibold transition-colors cursor-pointer {$filterStore.album_id ? 'text-[var(--text-muted)] hover:text-white' : 'text-purple-400'}"
      >
        All Albums
      </button>

      {#if activeAlbum}
        <span class="opacity-30">/</span>
        <div class="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-purple-500/15 border border-purple-500/30 text-purple-300 font-medium">
          <span>📁 {activeAlbum.title}</span>
          <button
            type="button"
            on:click={() => filterStore.clearAlbum()}
            class="hover:text-white transition-colors cursor-pointer ml-1 leading-none"
            title="Exit album"
          >
            ✕
          </button>
        </div>
      {/if}
    </div>

    <!-- Create Album Button -->
    <button
      type="button"
      on:click={() => dispatch('openCreate')}
      class="text-xs px-2.5 py-1 rounded-xl bg-purple-600/20 hover:bg-purple-600/30 text-purple-300 border border-purple-500/30 font-medium transition cursor-pointer flex items-center gap-1 spring-tap"
    >
      <span>+</span>
      <span>New Album</span>
    </button>
  </div>

  <!-- Album Cards Shelf (Visible when not drilling inside an album) -->
  {#if !$filterStore.album_id}
    {#if $albumStore.length === 0}
      <div class="py-8 text-center border border-dashed border-[var(--border-glass)] rounded-2xl bg-white/[0.02]">
        <p class="text-xs text-[var(--text-muted)]">No albums yet. Tap "+ New Album" or select photos to create one.</p>
      </div>
    {:else}
      <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-3">
        {#each $albumStore as album (album.id)}
          <button
            type="button"
            on:click={() => filterStore.setAlbumId(album.id)}
            class="group text-left p-2.5 rounded-2xl bg-white/5 hover:bg-white/10 border border-[var(--border-glass)] hover:border-purple-500/40 transition-all cursor-pointer flex flex-col gap-2 spring-tap"
          >
            <div class="w-full aspect-square rounded-xl overflow-hidden bg-black/25 flex items-center justify-center border border-white/5 relative">
              {#if album.cover_thumb}
                <img
                  src={album.cover_thumb.startsWith('/') ? album.cover_thumb : `/${album.cover_thumb}`}
                  alt={album.title}
                  class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                />
              {:else}
                <span class="text-2xl opacity-40">📁</span>
              {/if}

              <div class="absolute bottom-1.5 right-1.5 bg-black/60 backdrop-blur-md px-1.5 py-0.5 rounded text-[9px] text-white/90 font-mono">
                {album.media_count}
              </div>
            </div>

            <div class="min-w-0 px-0.5">
              <div class="text-xs font-semibold truncate text-[var(--text-main)] group-hover:text-purple-400 transition-colors">
                {album.title}
              </div>
              <div class="text-[10px] text-[var(--text-muted)]">
                {album.media_count} {album.media_count === 1 ? 'item' : 'items'}
              </div>
            </div>
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>