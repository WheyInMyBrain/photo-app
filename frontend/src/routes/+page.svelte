<script lang="ts">
  import { browser } from '$app/environment';
  import { onMount, onDestroy } from 'svelte';
  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import PhotoModal from '$lib/components/PhotoModal.svelte';
  import VirtualSection from '$lib/components/VirtualSection.svelte';

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
    deleted_at: string | null;
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
  let itemIndexMap = new Map<string, number>();

  let groupedSections: [string, MediaItem[]][] = [];
  let groupIndexMap = new Map<string, number>();

  let nextCapturedAt: string | null = null;
  let nextId: string | null = null;
  let hasMore = true;
  let isLoading = false;
  let scrollTrigger: HTMLDivElement;
  let observer: IntersectionObserver | null = null;

  // Multi-select state
  let selectedIds = new Set<string>();
  let isActionLoading = false;

  $: folderSegments = $filterStore.folder_path
    ? $filterStore.folder_path.split('/').filter(Boolean)
    : [];

  let selectedIndex: number | null = null;
  $: selectedAsset = selectedIndex !== null ? items[selectedIndex] : null;

  function getGroupHeader(dateStr: string | null): string {
    if (!dateStr) return 'Undated';
    const d = new Date(dateStr);
    return isNaN(d.getTime())
      ? 'Undated'
      : d.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
  }

  function getDaysRemaining(deletedAt: string | null): number {
    if (!deletedAt) return 30;
    const diffMs = Date.now() - new Date(deletedAt).getTime();
    const daysPassed = Math.floor(diffMs / (1000 * 60 * 60 * 24));
    return Math.max(0, 30 - daysPassed);
  }

  function appendItemsToGroups(newItems: MediaItem[], startIndex: number) {
    for (let i = 0; i < newItems.length; i++) {
      const item = newItems[i];
      const globalIdx = startIndex + i;
      itemIndexMap.set(item.id, globalIdx);

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
    groupedSections = groupedSections;
  }

  async function fetchMedia(reset = false) {
    if (!browser || isLoading || (!hasMore && !reset)) return;
    isLoading = true;

    if (reset) {
      items = [];
      itemIndexMap.clear();
      albums = [];
      groupedSections = [];
      groupIndexMap.clear();
      selectedIds.clear();
      selectedIds = selectedIds;
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

      albums = reset ? data.albums ?? [] : albums;

      const newItems = data.items;
      const startIndex = items.length;
      items = reset ? newItems : [...items, ...newItems];

      appendItemsToGroups(newItems, startIndex);

      nextCapturedAt = data.next_cursor_captured_at;
      nextId = data.next_cursor_id;
      hasMore = data.has_more;
    } catch (err) {
      console.error('Failed fetching media:', err);
    } finally {
      isLoading = false;
    }
  }

  $: if (browser && $filterQueryString !== undefined) {
    fetchMedia(true);
  }

  function toggleSelect(id: string, e: MouseEvent) {
    e.stopPropagation();
    if (selectedIds.has(id)) {
      selectedIds.delete(id);
    } else {
      selectedIds.add(id);
    }
    selectedIds = selectedIds;
  }

  function clearSelection() {
    selectedIds.clear();
    selectedIds = selectedIds;
  }

  async function handleBatchToggleDelete() {
    if (selectedIds.size === 0 || isActionLoading) return;
    isActionLoading = true;

    try {
      const promises = Array.from(selectedIds).map((id) =>
        fetch(`/api/assets/${id}/delete`, { method: 'POST' })
      );
      await Promise.all(promises);
      clearSelection();
      fetchMedia(true);
    } catch (e) {
      console.error('Batch delete error', e);
    } finally {
      isActionLoading = false;
    }
  }

  async function handleBatchPurge() {
    if (selectedIds.size === 0 || isActionLoading) return;
    const confirmed = confirm(
      `Are you sure you want to permanently delete ${selectedIds.size} item(s)? This cannot be undone.`
    );
    if (!confirmed) return;

    isActionLoading = true;
    try {
      const promises = Array.from(selectedIds).map((id) =>
        fetch(`/api/assets/${id}/purge`, { method: 'POST' })
      );
      await Promise.all(promises);
      clearSelection();
      fetchMedia(true);
    } catch (e) {
      console.error('Batch purge error', e);
    } finally {
      isActionLoading = false;
    }
  }

  function openModalForAsset(id: string) {
    const idx = itemIndexMap.get(id);
    if (idx !== undefined) {
      selectedIndex = idx;
    }
  }

  onMount(() => {
    fetchMedia(true);
    observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasMore && !isLoading) {
          fetchMedia();
        }
      },
      { rootMargin: '600px' }
    );
    if (scrollTrigger) observer.observe(scrollTrigger);
  });

  onDestroy(() => {
    if (observer) observer.disconnect();
  });
</script>

<div class="p-6 max-w-7xl mx-auto space-y-6 min-h-full flex flex-col select-none pb-24">
  <!-- Breadcrumbs & Status -->
  <div class="flex items-center justify-between">
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

    {#if $filterStore.show_trash}
      <span class="text-xs bg-red-950/80 border border-red-800/80 text-red-300 px-2 py-0.5 rounded-full font-medium">
        Trash: Items auto-delete after 30 days
      </span>
    {/if}
  </div>

  <!-- Folders -->
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

  <!-- Media Grid with Section Windowing -->
  {#if items.length === 0 && albums.length === 0 && !isLoading}
    <div class="flex-1 flex flex-col items-center justify-center text-center py-16 text-neutral-500 text-xs">
      <div class="text-2xl mb-1">
        {$filterStore.show_trash ? '🗑️' : $filterStore.is_private ? '🔒' : '📷'}
      </div>
      {#if $filterStore.show_trash}
        Trash is empty.
      {:else if $filterStore.is_private}
        No private media in this location.
      {:else}
        No media in this location.
      {/if}
    </div>
  {:else}
    <div class="space-y-6">
      {#each groupedSections as [groupName, groupList] (groupName)}
        <VirtualSection minHeight={240}>
          <h2 class="text-xs font-semibold text-neutral-400 uppercase tracking-wider mb-2 sticky top-0 bg-neutral-950/80 backdrop-blur-md py-1 z-10">
            {groupName}
          </h2>
          <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-2.5">
            {#each groupList as asset (asset.id)}
              <div
                role="button"
                tabindex="0"
                on:click={() => openModalForAsset(asset.id)}
                on:keydown={(e) => {
                  if (e.key === 'Enter') openModalForAsset(asset.id);
                }}
                class="group relative aspect-square bg-neutral-900 rounded-lg overflow-hidden border transition-all cursor-pointer will-change-transform {selectedIds.has(asset.id) ? 'border-purple-500 ring-2 ring-purple-500/40' : 'border-neutral-800/80 hover:border-neutral-700'}"
              >
                <img
                  src="/{asset.thumb_path}"
                  alt={asset.file_name}
                  loading="lazy"
                  decoding="async"
                  class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-200 pointer-events-none"
                />

                <!-- Checkbox Multi-Select Dot -->
                <button
                  type="button"
                  on:click={(e) => toggleSelect(asset.id, e)}
                  class="absolute top-1.5 left-1.5 w-5 h-5 rounded-md flex items-center justify-center transition-all z-20 cursor-pointer {selectedIds.has(asset.id) ? 'bg-purple-600 text-white' : 'bg-black/40 text-transparent hover:bg-black/70 hover:text-neutral-400 border border-neutral-700/60'}"
                  title="Select photo"
                >
                  <span class="text-xs font-bold leading-none">✓</span>
                </button>

                <!-- Trash Badge -->
                {#if asset.deleted_at}
                  <div class="absolute bottom-1.5 left-1.5 bg-red-950/90 border border-red-800/80 px-1.5 py-0.5 rounded text-[9px] text-red-300 font-mono z-10 shadow">
                    🗑️ {getDaysRemaining(asset.deleted_at)}d left
                  </div>
                {/if}

                <!-- Favorite Badge -->
                {#if asset.is_favorite}
                  <div class="absolute top-1.5 right-1.5 bg-black/60 p-1 rounded-full text-amber-400 text-[10px] leading-none">★</div>
                {/if}

                <!-- Duration Badge -->
                {#if asset.duration_seconds}
                  <div class="absolute bottom-1 right-1 bg-black/75 px-1 py-0.5 rounded text-[9px] text-white font-mono">
                    {Math.floor(asset.duration_seconds / 60)}:{Math.floor(asset.duration_seconds % 60).toString().padStart(2, '0')}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </VirtualSection>
      {/each}
    </div>
  {/if}

  <div bind:this={scrollTrigger} class="py-6 text-center text-xs text-neutral-600">
    {#if isLoading}Loading more...{/if}
  </div>

  <!-- Action Bar -->
  {#if selectedIds.size > 0}
    <div class="fixed bottom-6 left-1/2 -translate-x-1/2 z-40 bg-neutral-900/95 border border-neutral-800 shadow-2xl backdrop-blur-md px-4 py-2 rounded-2xl flex items-center gap-3">
      <span class="text-xs text-neutral-300 font-medium">
        {selectedIds.size} selected
      </span>

      <div class="h-4 w-px bg-neutral-800"></div>

      {#if $filterStore.show_trash}
        <button
          type="button"
          on:click={handleBatchToggleDelete}
          disabled={isActionLoading}
          class="text-xs px-2.5 py-1 bg-neutral-800 hover:bg-neutral-700 text-neutral-200 rounded-lg transition-colors cursor-pointer font-medium"
        >
          ↺ Restore
        </button>

        <button
          type="button"
          on:click={handleBatchPurge}
          disabled={isActionLoading}
          class="text-xs px-2.5 py-1 bg-red-900/70 hover:bg-red-800 text-red-200 rounded-lg transition-colors cursor-pointer font-medium"
        >
          Delete Forever
        </button>
      {:else}
        <button
          type="button"
          on:click={handleBatchToggleDelete}
          disabled={isActionLoading}
          class="text-xs px-3 py-1 bg-red-950/80 hover:bg-red-900 text-red-300 border border-red-900/60 rounded-lg transition-colors cursor-pointer font-medium flex items-center gap-1.5"
        >
          <span>🗑️</span>
          <span>Move to Trash</span>
        </button>
      {/if}

      <button
        type="button"
        on:click={clearSelection}
        class="text-xs text-neutral-500 hover:text-white px-1.5 py-1 cursor-pointer"
        title="Deselect All"
      >
        ✕
      </button>
    </div>
  {/if}

  <!-- Single Photo Modal View -->
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