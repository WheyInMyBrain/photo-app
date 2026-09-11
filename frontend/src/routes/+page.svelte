<script lang="ts">
  import { browser } from '$app/environment';
  import { onMount, onDestroy } from 'svelte';
  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import PhotoModal from '$lib/components/PhotoModal.svelte';

  interface SubAlbum {
    name: string;
    path: string;
    count: number;
    cover_thumb: string | null;
  }

  interface MediaItem {
    id: string;
    file_name: string;
    thumb_path: string;
    preview_path: string;
    aspect_ratio: number | null;
    duration_seconds: number | null;
    mime_type: string;
    captured_at: string | null;
    is_favorite: number;
  }

  interface MediaPageResponse {
    albums: SubAlbum[];
    items: MediaItem[];
    next_cursor_captured_at: string | null;
    next_cursor_id: string | null;
    has_more: boolean;
  }

  let albums: SubAlbum[] = [];
  let items: MediaItem[] = [];
  
  // High-efficiency grouped storage: ordered list of [groupKey, itemsInGroup]
  let groupedSections: [string, MediaItem[]][] = [];
  let groupIndexMap = new Map<string, number>();

  let nextCapturedAt: string | null = null;
  let nextId: string | null = null;
  let hasMore = true;
  let isLoading = false;
  let scrollTrigger: HTMLDivElement;
  let observer: IntersectionObserver | null = null;

  $: folderSegments = $filterStore.folder_path ? $filterStore.folder_path.split('/').filter(Boolean) : [];

  let selectedIndex: number | null = null;
  $: selectedAsset = selectedIndex !== null ? items[selectedIndex] : null;

  function getGroupHeader(dateStr: string | null): string {
    if (!dateStr) return 'Undated';
    const d = new Date(dateStr);
    return isNaN(d.getTime()) ? 'Undated' : d.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
  }

  // Incremental append: O(batch_size) instead of O(total_media)
  function appendItemsToGroups(newItems: MediaItem[]) {
    for (const item of newItems) {
      const key = getGroupHeader(item.captured_at);
      let gIdx = groupIndexMap.get(key);

      if (gIdx === undefined) {
        gIdx = groupedSections.length;
        groupIndexMap.set(key, gIdx);
        groupedSections.push([key, [item]]);
      } else {
        groupedSections[gIdx][1].push(item);
      }
    }
    groupedSections = groupedSections; // Single tick trigger
  }

  async function fetchMedia(reset = false) {
    if (!browser || isLoading || (!hasMore && !reset)) return;
    isLoading = true;

    if (reset) {
      items = [];
      albums = [];
      groupedSections = [];
      groupIndexMap.clear();
      nextCapturedAt = null;
      nextId = null;
      hasMore = true;
    }

    try {
      const baseParams = new URLSearchParams($filterQueryString.replace(/^\?/, ''));
      baseParams.set('limit', '50');

      if (nextCapturedAt && nextId) {
        baseParams.set('cursor_captured_at', nextCapturedAt);
        baseParams.set('cursor_id', nextId);
      }

      const res = await fetch(`/api/media?${baseParams.toString()}`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data: MediaPageResponse = await res.json();

      albums = reset ? (data.albums ?? []) : albums;
      
      const newItems = data.items;
      items = reset ? newItems : [...items, ...newItems];
      appendItemsToGroups(newItems);

      nextCapturedAt = data.next_cursor_captured_at;
      nextId = data.next_cursor_id;
      hasMore = data.has_more;
    } finally {
      isLoading = false;
    }
  }

  $: if (browser && $filterQueryString !== undefined) {
    fetchMedia(true);
  }

  function openModal(globalIdx: number) {
    selectedIndex = globalIdx;
  }

  onMount(() => {
    fetchMedia(true);
    observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasMore && !isLoading) {
          fetchMedia();
        }
      },
      { rootMargin: '400px' }
    );
    if (scrollTrigger) observer.observe(scrollTrigger);
  });

  onDestroy(() => {
    if (observer) observer.disconnect();
  });
</script>

<div class="p-6 max-w-7xl mx-auto space-y-6 min-h-full flex flex-col select-none">
  <!-- Breadcrumbs -->
  <div class="flex items-center gap-1.5 text-xs text-neutral-400">
    <button
      type="button"
      on:click={() => filterStore.setFolderPath('')}
      class="hover:text-white transition-colors cursor-pointer font-medium"
    >
      Root
    </button>
    {#each folderSegments as seg, i}
      <span class="text-neutral-600">/</span>
      <button
        type="button"
        on:click={() => filterStore.setFolderPath(folderSegments.slice(0, i + 1).join('/'))}
        class="hover:text-white transition-colors cursor-pointer {i === folderSegments.length - 1 ? 'text-white font-semibold' : ''}"
      >
        {seg}
      </button>
    {/each}
  </div>

  <!-- Sub-Albums Display -->
  {#if albums.length > 0}
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
                <img src="/{album.cover_thumb}" alt={album.name} class="w-full h-full object-cover group-hover:scale-105 transition-transform" />
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

  <!-- Media Grid -->
  {#if items.length === 0 && albums.length === 0 && !isLoading}
    <div class="flex-1 flex flex-col items-center justify-center text-center py-16 text-neutral-500 text-xs">
      <div class="text-2xl mb-1">
        {$filterStore.is_private ? '🔒' : '📷'}
      </div>
      {$filterStore.is_private ? 'No private media in this location.' : 'No media in this location.'}
    </div>
  {:else}
    <div class="space-y-6">
      {#each groupedSections as [groupName, groupList] (groupName)}
        <!-- content-visibility skips rendering computations for off-screen months -->
        <section class="section-contain">
          <h2 class="text-xs font-semibold text-neutral-400 uppercase tracking-wider mb-2 sticky top-0 bg-neutral-950/80 backdrop-blur-md py-1 z-10">
            {groupName}
          </h2>
          <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-2.5">
            {#each groupList as asset, localIdx (asset.id)}
              <button
                type="button"
                on:click={() => {
                  const globalIdx = items.indexOf(asset);
                  openModal(globalIdx !== -1 ? globalIdx : 0);
                }}
                class="group relative aspect-square bg-neutral-900 rounded-lg overflow-hidden border border-neutral-800/80 hover:border-neutral-700 transition-all cursor-pointer"
              >
                <img
                  src="/{asset.thumb_path}"
                  alt={asset.file_name}
                  loading="lazy"
                  decoding="async"
                  class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-200"
                />
                {#if asset.is_favorite}
                  <div class="absolute top-1.5 right-1.5 bg-black/60 p-1 rounded-full text-amber-400 text-[10px]">★</div>
                {/if}
                {#if asset.duration_seconds}
                  <div class="absolute bottom-1 right-1 bg-black/75 px-1 py-0.5 rounded text-[9px] text-white font-mono">
                    {Math.floor(asset.duration_seconds / 60)}:{Math.floor(asset.duration_seconds % 60).toString().padStart(2, '0')}
                  </div>
                {/if}
              </button>
            {/each}
          </div>
        </section>
      {/each}
    </div>
  {/if}

  <div bind:this={scrollTrigger} class="py-6 text-center text-xs text-neutral-600">
    {#if isLoading}Loading more...{/if}
  </div>

  {#if selectedAsset && selectedIndex !== null}
    <PhotoModal
      asset={selectedAsset}
      hasPrev={selectedIndex > 0}
      hasNext={selectedIndex < items.length - 1}
      on:close={() => (selectedIndex = null)}
      on:prev={() => selectedIndex && (selectedIndex -= 1)}
      on:next={() => selectedIndex !== null && (selectedIndex += 1)}
      on:toggleFavorite={(e) => {
        const item = items.find((i) => i.id === e.detail.id);
        if (item) {
          item.is_favorite = e.detail.is_favorite ? 1 : 0;
          items = [...items];
        }
      }}
    />
  {/if}
</div>

<style>
  /* Instructs Chromium & WebKit to skip layout and painting for off-screen month groups */
  .section-contain {
    content-visibility: auto;
    contain-intrinsic-size: 1px 300px;
  }
</style>