<script lang="ts">
  import { browser } from '$app/environment';
  import { onMount, onDestroy } from 'svelte';
  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import type { SubAlbum, MediaItem, MediaPageResponse } from '$lib/types/media';

  import VirtualSection from '$lib/components/VirtualSection.svelte';
  import FolderGrid from '$lib/components/FolderGrid.svelte';
  import MediaCard from '$lib/components/MediaCard.svelte';
  import BatchActionBar from '$lib/components/BatchActionBar.svelte';
  import PhotoModal from '$lib/components/PhotoModal.svelte';

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
  let filterDebounceTimer: ReturnType<typeof setTimeout> | null = null;

  let pageAbortCtrl: AbortController | null = null;
  let lastFetchErrorTime = 0;
  const RETRY_BACKOFF_MS = 3000;

  let selectedMap: Record<string, boolean> = {};
  let selectedCount = 0;
  let lastSelectedId: string | null = null;
  let isActionLoading = false;

  let selectedIndex: number | null = null;
  $: selectedAsset = selectedIndex !== null ? items[selectedIndex] : null;

  $: folderSegments = $filterStore.folder_path
    ? $filterStore.folder_path.split('/').filter(Boolean)
    : [];

  function getGroupHeader(dateStr: string | null): string {
    if (!dateStr) return 'Undated';
    const d = new Date(dateStr);
    return isNaN(d.getTime())
      ? 'Undated'
      : d.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
  }

  function appendItemsToGroups(newItems: MediaItem[], startIndex: number) {
    for (let i = 0; i < newItems.length; i++) {
      const item = newItems[i];
      itemIndexMap.set(item.id, startIndex + i);

      const key = getGroupHeader(item.captured_at);
      const gIdx = groupIndexMap.get(key);

      if (gIdx === undefined) {
        groupIndexMap.set(key, groupedSections.length);
        groupedSections.push([key, [item]]);
      } else {
        groupedSections[gIdx][1].push(item);
      }
    }
    groupedSections = groupedSections;
  }

  async function fetchMedia(reset = false) {
    if (!browser || (!hasMore && !reset)) return;

    if (reset) {
      if (pageAbortCtrl) pageAbortCtrl.abort();
      items = [];
      itemIndexMap.clear();
      albums = [];
      groupedSections = [];
      groupIndexMap.clear();
      selectedMap = {};
      selectedCount = 0;
      lastSelectedId = null;
      nextCapturedAt = null;
      nextId = null;
      hasMore = true;
      lastFetchErrorTime = 0;
    } else {
      if (isLoading || Date.now() - lastFetchErrorTime < RETRY_BACKOFF_MS) return;
    }

    pageAbortCtrl = new AbortController();
    isLoading = true;

    try {
      const baseParams = new URLSearchParams($filterQueryString.replace(/^\?/, ''));
      baseParams.set('limit', '50');

      if (nextCapturedAt && nextId) {
        baseParams.set('cursor_captured_at', nextCapturedAt);
        baseParams.set('cursor_id', nextId);
      }

      const res = await fetch(`/api/media?${baseParams.toString()}`, {
        signal: pageAbortCtrl.signal
      });
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
      lastFetchErrorTime = 0;
    } catch (err: any) {
      if (err?.name === 'AbortError') return;
      lastFetchErrorTime = Date.now();
      console.error('Failed fetching media:', err);
    } finally {
      isLoading = false;
    }
  }

  $: if (browser && $filterQueryString !== undefined) {
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    filterDebounceTimer = setTimeout(() => {
      fetchMedia(true);
    }, 200);
  }

  function toggleSelect(id: string, e: MouseEvent) {
    e.stopPropagation();

    if (e.shiftKey && lastSelectedId && lastSelectedId !== id) {
      const startIdx = itemIndexMap.get(lastSelectedId);
      const endIdx = itemIndexMap.get(id);

      if (startIdx !== undefined && endIdx !== undefined) {
        const [low, high] = [Math.min(startIdx, endIdx), Math.max(startIdx, endIdx)];
        for (let i = low; i <= high; i++) {
          const target = items[i];
          if (target && !selectedMap[target.id]) {
            selectedMap[target.id] = true;
            selectedCount += 1;
          }
        }
        lastSelectedId = id;
        selectedMap = selectedMap;
        return;
      }
    }

    if (selectedMap[id]) {
      delete selectedMap[id];
      selectedCount -= 1;
      lastSelectedId = null;
    } else {
      selectedMap[id] = true;
      selectedCount += 1;
      lastSelectedId = id;
    }
    selectedMap = selectedMap;
  }

  function clearSelection() {
    selectedMap = {};
    selectedCount = 0;
    lastSelectedId = null;
  }

  async function handleBatchToggleDelete() {
    const ids = Object.keys(selectedMap);
    if (ids.length === 0 || isActionLoading) return;
    isActionLoading = true;

    try {
      const res = await fetch('/api/assets/batch/delete', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ids })
      });

      if (!res.ok) throw new Error(`HTTP ${res.status}`);

      clearSelection();
      fetchMedia(true);
    } catch (e) {
      console.error('Batch delete error', e);
    } finally {
      isActionLoading = false;
    }
  }

  async function handleBatchPurge() {
    const ids = Object.keys(selectedMap);
    if (ids.length === 0 || isActionLoading) return;
    if (!confirm(`Permanently delete ${ids.length} item(s)? This cannot be undone.`)) return;

    isActionLoading = true;
    try {
      const res = await fetch('/api/assets/batch/purge', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ids })
      });

      if (!res.ok) throw new Error(`HTTP ${res.status}`);

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
    if (idx !== undefined) selectedIndex = idx;
  }

  onMount(() => {
    const handleRefresh = () => fetchMedia(true);
    window.addEventListener('vault:refresh-timeline', handleRefresh);

    const scrollContainer = document.querySelector('main');
    observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasMore && !isLoading) {
          fetchMedia();
        }
      },
      {
        root: scrollContainer ?? null,
        rootMargin: '600px'
      }
    );
    if (scrollTrigger) observer.observe(scrollTrigger);

    return () => {
      window.removeEventListener('vault:refresh-timeline', handleRefresh);
    };
  });

  onDestroy(() => {
    if (pageAbortCtrl) pageAbortCtrl.abort();
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
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

  <!-- Folder Grid Component -->
  <FolderGrid {albums} />

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
        <VirtualSection itemCount={groupList.length} minHeight={240}>
          <h2 class="text-xs font-semibold text-neutral-400 uppercase tracking-wider mb-2 sticky top-0 bg-neutral-950/80 backdrop-blur-md py-1 z-10">
            {groupName}
          </h2>
          <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-2.5">
            {#each groupList as asset (asset.id)}
              <MediaCard
                {asset}
                isSelected={Boolean(selectedMap[asset.id])}
                on:open={() => openModalForAsset(asset.id)}
                on:select={(e) => toggleSelect(asset.id, e.detail)}
              />
            {/each}
          </div>
        </VirtualSection>
      {/each}
    </div>
  {/if}

  <!-- Infinite Scroll Intersection Target -->
  <div bind:this={scrollTrigger} class="py-6 text-center text-xs text-neutral-600">
    {#if isLoading}Loading more...{/if}
  </div>

  <!-- Floating Multi-Select Action Bar Component -->
  <BatchActionBar
    count={selectedCount}
    {isActionLoading}
    on:toggleDelete={handleBatchToggleDelete}
    on:purge={handleBatchPurge}
    on:clear={clearSelection}
  />

  <!-- Single Photo Modal View -->
  {#if selectedAsset && selectedIndex !== null}
    <PhotoModal
      asset={selectedAsset}
      prevAsset={selectedIndex > 0 ? items[selectedIndex - 1] : null}
      nextAsset={selectedIndex < items.length - 1 ? items[selectedIndex + 1] : null}
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