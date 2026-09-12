<script lang="ts">
  import type { SubAlbum } from '$lib/types/media';
  import { filterStore } from '$lib/stores/filterStore';

  export let albums: SubAlbum[] = [];
</script>

{#if albums.length > 0 && !$filterStore.show_trash}
  <div>
    <h3 class="text-[11px] uppercase font-semibold text-neutral-500 tracking-wider mb-2">Folders</h3>
    <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-2.5">
      {#each albums as album (album.path)}
        <button
          type="button"
          on:click={() => filterStore.setFolderPath(album.path)}
          class="group bg-neutral-900 border border-neutral-800 hover:border-neutral-700 p-2.5 rounded-xl flex items-center gap-2.5 text-left transition-all cursor-pointer"
        >
          <div class="w-9 h-9 rounded-lg bg-neutral-800 flex items-center justify-center overflow-hidden flex-shrink-0">
            {#if album.cover_thumb}
              <img
                src="/{album.cover_thumb}"
                alt={album.name}
                class="w-full h-full object-cover group-hover:scale-105 transition-transform"
              />
            {:else}
              📁
            {/if}
          </div>
          <div class="truncate">
            <div class="text-xs font-medium text-neutral-200 group-hover:text-white truncate">{album.name}</div>
            <div class="text-[10px] text-neutral-500">{album.count} items</div>
          </div>
        </button>
      {/each}
    </div>
  </div>
{/if}