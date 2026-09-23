<script lang="ts">
  import type { SubAlbum } from '$lib/types/media';
  import { filterStore } from '$lib/stores/filterStore';

  export let albums: SubAlbum[] = [];

  function resolveUrl(path: string | null | undefined): string {
    if (!path) return '';
    if (path.startsWith('http')) return path;
    const clean = path.startsWith('/') ? path.slice(1) : path;
    return clean.startsWith('users/') ? `/${clean}` : `/thumbs/${clean}`;
  }
</script>

{#if albums.length > 0 && !$filterStore.show_trash}
  <div class="space-y-2 mb-4">
    <div class="flex items-center justify-between px-1">
      <span class="text-[10px] uppercase font-bold tracking-wider text-[var(--text-muted)]">
        Collections
      </span>
      <span class="text-[10px] font-mono text-[var(--text-muted)] opacity-60">
        {albums.length} {albums.length === 1 ? 'folder' : 'folders'}
      </span>
    </div>

    <!-- Square tiles matching the photo grid -->
    <div class="flex flex-wrap gap-1.5 md:gap-2.5 justify-start after:content-[''] after:flex-grow-[999999999]">
      {#each albums as album (album.path)}
        <button
          type="button"
          on:click={() => filterStore.setFolderPath(album.path)}
          style="flex-grow: 100; flex-basis: 140px; aspect-ratio: 1;"
          class="group relative rounded-xl overflow-hidden glass-panel border border-[var(--border-glass)] hover:border-purple-500/60 shadow-sm hover:shadow-xl transition-all spring-tap cursor-pointer min-w-[90px] max-h-[260px] md:max-h-[320px] text-left focus:outline-none focus:ring-2 focus:ring-purple-400"
        >
          <!-- Vignette Gradient -->
          <div class="absolute inset-0 bg-gradient-to-t from-black/85 via-black/20 to-black/10 z-10 pointer-events-none"></div>

          <!-- Cover Image -->
          {#if album.cover_thumb}
            <img
              src={resolveUrl(album.cover_thumb)}
              alt={album.name}
              loading="lazy"
              class="w-full h-full object-cover transition-transform duration-500 group-hover:scale-105 pointer-events-none"
            />
          {:else}
            <div class="w-full h-full flex items-center justify-center text-3xl bg-[var(--bg-surface-elevated)]">
              📁
            </div>
          {/if}

          <!-- Micro Badge -->
          <div class="absolute top-2 left-2 z-20 glass-panel px-2 py-0.5 rounded-full flex items-center gap-1 text-[10px] text-white font-medium border border-white/10 shadow-sm">
            <span>📁</span>
            <span class="font-mono text-[9px] opacity-80">{album.count}</span>
          </div>

          <!-- Title Details -->
          <div class="absolute bottom-2.5 inset-x-2.5 z-20 pointer-events-none">
            <h4 class="text-xs font-bold text-white tracking-tight truncate drop-shadow-sm">
              {album.name}
            </h4>
            <span class="text-[9px] text-white/60 font-mono tracking-wide uppercase">
              Album
            </span>
          </div>
        </button>
      {/each}
    </div>
  </div>
{/if}