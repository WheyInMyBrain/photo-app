<script lang="ts">
  import { browser } from '$app/environment';
  import { onMount, onDestroy } from 'svelte';
  import { flip } from 'svelte/animate';
  import { scale, fade } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';

  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import { authStore } from '$lib/stores/authStore';
  import { createMediaSelection } from '$lib/stores/mediaSelection';
  import { buildGroupedSections, buildIndexMap } from '$lib/utils/mediaGrouper';
  import { initMediaEvents } from '$lib/utils/mediaEvents';
  import type { SubAlbum, MediaItem, MediaPageResponse } from '$lib/types/media';

  import VirtualSection from '$lib/components/VirtualSection.svelte';
  import FolderGrid from '$lib/components/FolderGrid.svelte';
  import MediaCard from '$lib/components/MediaCard.svelte';
  import BatchActionBar from '$lib/components/BatchActionBar.svelte';
  import PhotoModal from '$lib/components/PhotoModal.svelte';

  // Core media state
  let albums: SubAlbum[] = [];
  let items: MediaItem[] = [];
  $: itemIndexMap = buildIndexMap(items);
  $: groupedSections = buildGroupedSections(items);

  // Pagination cursors
  let nextCapturedAt: string | null = null;
  let nextId: string | null = null;
  let hasMore = true;
  let isLoading = false;
  let scrollTrigger: HTMLDivElement;
  let observer: IntersectionObserver | null = null;
  let filterDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  let pageAbortCtrl: AbortController | null = null;

  // Real-time animation tracking
  let recentAssetIds = new Set<string>();
  let sseSubscription: { close: () => void } | null = null;

  // Selection domain
  const selection = createMediaSelection(() => fetchMedia(true));
  const { selectedMap, selectedCount, isActionLoading } = selection;

  // Modal inspection
  // Track the asset by unique ID:
  let selectedAssetId: string | null = null;

  // Reactively resolve index & asset via the index map:
  $: selectedIndex = selectedAssetId !== null && itemIndexMap.has(selectedAssetId)
    ? itemIndexMap.get(selectedAssetId)!
    : null;

  $: selectedAsset = selectedIndex !== null ? items[selectedIndex] : null;

  $: folderSegments = $filterStore.folder_path
    ? $filterStore.folder_path.split('/').filter(Boolean)
    : [];

  function compareItems(a: MediaItem, b: MediaItem): number {
    // 1. Primary sort: captured_at descending (nulls placed at the very bottom)
    const timeA = a.captured_at ? new Date(a.captured_at).getTime() : -Infinity;
    const timeB = b.captured_at ? new Date(b.captured_at).getTime() : -Infinity;

    if (timeA !== timeB) {
      return timeB - timeA; // Descending (newer dates first)
    }

    // 2. Secondary sort tie-breaker matching backend cursor: id descending
    return b.id.localeCompare(a.id);
  }

  function insertItemSorted(item: MediaItem) {
    if (itemIndexMap.has(item.id)) return;

    // Track ID for highlight animation
    recentAssetIds.add(item.id);
    recentAssetIds = new Set(recentAssetIds);
    setTimeout(() => {
      recentAssetIds.delete(item.id);
      recentAssetIds = new Set(recentAssetIds);
    }, 2000);

    // Binary search insertion (O(log n) location search, O(n) array splice)
    let low = 0;
    let high = items.length;

    while (low < high) {
      const mid = (low + high) >>> 1;
      if (compareItems(item, items[mid]) < 0) {
        high = mid;
      } else {
        low = mid + 1;
      }
    }

    // If the item belongs past the currently loaded cursor and more pages exist,
    // do not splice it into the view (it will naturally appear when scrolling down).
    if (low === items.length && hasMore) {
      return;
    }

    const updated = [...items];
    updated.splice(low, 0, item);
    items = updated;
  }

  async function fetchMedia(reset = false) {
    if (!browser || (!hasMore && !reset)) return;

    if (reset) {
      if (pageAbortCtrl) pageAbortCtrl.abort();
      // Retaining array allows Svelte FLIP transitions on remaining items
      selection.clearSelection();
      nextCapturedAt = null;
      nextId = null;
      hasMore = true;
    } else if (isLoading) {
      return;
    }

    pageAbortCtrl = new AbortController();
    isLoading = true;

    try {
      const params = new URLSearchParams($filterQueryString.replace(/^\?/, ''));
      params.set('limit', '50');

      if (nextCapturedAt && nextId) {
        params.set('cursor_captured_at', nextCapturedAt);
        params.set('cursor_id', nextId);
      }

      const res = await fetch(`/api/media?${params.toString()}`, { signal: pageAbortCtrl.signal });

      if (res.status === 401) {
        authStore.checkStatus();
        return;
      }

      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data: MediaPageResponse = await res.json();

      albums = reset ? data.albums ?? [] : albums;
      items = reset ? data.items : [...items, ...data.items];

      nextCapturedAt = data.next_cursor_captured_at;
      nextId = data.next_cursor_id;
      hasMore = data.has_more;
    } catch (err: any) {
      if (err?.name !== 'AbortError') console.error('Failed fetching media:', err);
    } finally {
      isLoading = false;
    }
  }

  async function handleAssetReady(assetId: string) {
    try {
      const params = new URLSearchParams($filterQueryString.replace(/^\?/, ''));
      params.set('limit', '50');
      params.delete('cursor_id');
      params.delete('cursor_captured_at');

      const res = await fetch(`/api/media?${params.toString()}`);
      if (!res.ok) return;

      const data: MediaPageResponse = await res.json();
      const match = data.items.find((i) => i.id === assetId);

      if (match) {
        insertItemSorted(match);
      } else {
        fetchMedia(true);
      }
    } catch (err) {
      fetchMedia(true);
    }
  }

  $: if (browser && $filterQueryString !== undefined) {
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    filterDebounceTimer = setTimeout(() => fetchMedia(true), 200);
  }

  onMount(() => {
    const handleRefresh = () => fetchMedia(true);
    window.addEventListener('vault:refresh-timeline', handleRefresh);

    sseSubscription = initMediaEvents(handleAssetReady);

    const scrollContainer = document.querySelector('main');
    observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasMore && !isLoading) fetchMedia();
      },
      { root: scrollContainer ?? null, rootMargin: '600px' }
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
    if (sseSubscription) sseSubscription.close();
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

  <FolderGrid {albums} />

  {#if items.length === 0 && albums.length === 0 && !isLoading}
    <div
      in:fade={{ duration: 250 }}
      class="flex-1 flex flex-col items-center justify-center text-center py-16 text-neutral-500 text-xs"
    >
      <div class="text-2xl mb-1">
        {$filterStore.show_trash ? '🗑️' : '📷'}
      </div>
      {#if $filterStore.show_trash}
        Trash is empty.
      {:else}
        No media found in this view.
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
              <div
                animate:flip={{ duration: 380, easing: cubicOut }}
                in:scale={{ start: 0.88, duration: 300, opacity: 0, easing: cubicOut }}
                out:scale={{ start: 0.88, duration: 220, opacity: 0, easing: cubicOut }}
                class="relative will-change-transform rounded-lg overflow-hidden transition-shadow duration-500 {recentAssetIds.has(asset.id) ? 'animate-incoming ring-2 ring-blue-500/60' : ''}"
              >
                <MediaCard
                  {asset}
                  isSelected={Boolean($selectedMap[asset.id])}
                  on:open={() => {
                    selectedAssetId = asset.id;
                  }}
                  on:select={(e) => selection.toggleSelect(asset.id, e.detail, items, itemIndexMap)}
                />
              </div>
            {/each}
          </div>
        </VirtualSection>
      {/each}
    </div>
  {/if}

  <div bind:this={scrollTrigger} class="py-6 text-center text-xs text-neutral-600 min-h-[2rem]">
    {#if isLoading}
      <span in:fade={{ duration: 150 }}>Loading more...</span>
    {/if}
  </div>

  <BatchActionBar
    count={$selectedCount}
    isActionLoading={$isActionLoading}
    on:toggleDelete={selection.batchToggleDelete}
    on:purge={selection.batchPurge}
    on:clear={selection.clearSelection}
  />

  <!-- In PhotoModal: -->
  {#if selectedAsset && selectedIndex !== null}
    <PhotoModal
      asset={selectedAsset}
      prevAsset={selectedIndex > 0 ? items[selectedIndex - 1] : null}
      nextAsset={selectedIndex < items.length - 1 ? items[selectedIndex + 1] : null}
      hasPrev={selectedIndex > 0}
      hasNext={selectedIndex < items.length - 1}
      on:close={() => (selectedAssetId = null)}
      on:prev={() => {
        if (selectedIndex !== null && selectedIndex > 0) {
          selectedAssetId = items[selectedIndex - 1].id;
        }
      }}
      on:next={() => {
        if (selectedIndex !== null && selectedIndex < items.length - 1) {
          selectedAssetId = items[selectedIndex + 1].id;
        }
      }}
      on:selectAsset={(e) => {
        selectedAssetId = e.detail.id;
      }}
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
  @keyframes incoming-fade {
    0% {
      box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.7), 0 8px 24px rgba(59, 130, 246, 0.25);
    }
    100% {
      box-shadow: 0 0 0 0 transparent, 0 0 0 transparent;
    }
  }

  :global(.animate-incoming) {
    animation: incoming-fade 2s cubic-bezier(0.16, 1, 0.3, 1) forwards;
  }
</style>