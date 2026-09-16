<script lang="ts">
  import { browser } from '$app/environment';
  import { onMount, onDestroy } from 'svelte';
  import { fade } from 'svelte/transition';

  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import { authStore } from '$lib/stores/authStore';
  import { createMediaSelection } from '$lib/stores/mediaSelection';
  import { buildGroupedSections, buildIndexMap } from '$lib/utils/mediaGrouper';
  import { initMediaEvents } from '$lib/utils/mediaEvents';
  import type { SubAlbum, MediaItem, MediaPageResponse } from '$lib/types/media';

  import FolderGrid from '$lib/components/FolderGrid.svelte';
  import MediaCard from '$lib/components/MediaCard.svelte';
  import BatchActionBar from '$lib/components/BatchActionBar.svelte';
  import PhotoModal from '$lib/components/PhotoModal.svelte';

  let albums: SubAlbum[] = [];
  let items: MediaItem[] = [];

  // Reactive projections
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

  // Real-time animation & batching
  let recentAssetIds = new Set<string>();
  let sseSubscription: { close: () => void } | null = null;
  let incomingQueue: string[] = [];
  let batchFlushTimer: ReturnType<typeof setTimeout> | null = null;

  // Selection domain
  const selection = createMediaSelection(() => fetchMedia(true));
  const { selectedMap, selectedCount, isActionLoading } = selection;

  // Modal inspection by stable ID
  let selectedAssetId: string | null = null;

  $: selectedIndex = selectedAssetId !== null && itemIndexMap.has(selectedAssetId)
    ? itemIndexMap.get(selectedAssetId)!
    : null;

  $: selectedAsset = selectedIndex !== null ? items[selectedIndex] : null;

  $: folderSegments = $filterStore.folder_path
    ? $filterStore.folder_path.split('/').filter(Boolean)
    : [];

  function compareItems(a: MediaItem, b: MediaItem): number {
    const timeA = a.captured_at ? new Date(a.captured_at).getTime() : -Infinity;
    const timeB = b.captured_at ? new Date(b.captured_at).getTime() : -Infinity;

    if (timeA !== timeB) return timeB - timeA;
    return b.id.localeCompare(a.id);
  }

  // Batch insert multiple incoming items at once to avoid layout thrashing
  function insertBatchSorted(newItems: MediaItem[]) {
    if (newItems.length === 0) return;

    const filtered = newItems.filter(item => !itemIndexMap.has(item.id));
    if (filtered.length === 0) return;

    for (const item of filtered) {
      recentAssetIds.add(item.id);
    }
    recentAssetIds = new Set(recentAssetIds);

    setTimeout(() => {
      for (const item of filtered) {
        recentAssetIds.delete(item.id);
      }
      recentAssetIds = new Set(recentAssetIds);
    }, 2500);

    // Merge and sort once instead of running multiple splices
    const combined = [...items, ...filtered];
    combined.sort(compareItems);
    items = combined;
  }

  // Optimized SSE handler: Batches requests into a single tick
  function handleAssetReady(assetId: string) {
    if (itemIndexMap.has(assetId) || incomingQueue.includes(assetId)) return;

    incomingQueue.push(assetId);

    if (!batchFlushTimer) {
      batchFlushTimer = setTimeout(async () => {
        const batch = [...incomingQueue];
        incomingQueue = [];
        batchFlushTimer = null;

        const fetchedItems: MediaItem[] = [];
        await Promise.allSettled(
          batch.map(async (id) => {
            try {
              const res = await fetch(`/api/media/${id}`);
              if (res.ok) {
                const payload = await res.json();
                const single: MediaItem = payload.asset || payload.item || payload;
                if (single && single.id) {
                  if (!$filterStore.folder_path || single.folder_path === $filterStore.folder_path) {
                    fetchedItems.push(single);
                  }
                }
              }
            } catch {}
          })
        );

        if (fetchedItems.length > 0) {
          insertBatchSorted(fetchedItems);
        } else {
          // Fallback if individual fetches failed
          fetchMedia(true);
        }
      }, 300);
    }
  }

  async function fetchMedia(reset = false) {
    if (!browser || (!hasMore && !reset)) return;

    if (reset) {
      if (pageAbortCtrl) pageAbortCtrl.abort();
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

  $: if (browser && $filterQueryString !== undefined) {
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    filterDebounceTimer = setTimeout(() => fetchMedia(true), 200);
  }

  onMount(() => {
    const handleRefresh = () => fetchMedia(true);
    window.addEventListener('vault:refresh-timeline', handleRefresh);

    sseSubscription = initMediaEvents(handleAssetReady);

    // Large rootMargin to prefetch before reaching viewport bottom
    observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasMore && !isLoading) {
          fetchMedia();
        }
      },
      { root: null, rootMargin: '800px 0px' }
    );

    if (scrollTrigger) observer.observe(scrollTrigger);

    return () => {
      window.removeEventListener('vault:refresh-timeline', handleRefresh);
    };
  });

  onDestroy(() => {
    if (pageAbortCtrl) pageAbortCtrl.abort();
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    if (batchFlushTimer) clearTimeout(batchFlushTimer);
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
      in:fade={{ duration: 150 }}
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
        <!-- Replaced conflicting virtualizer with pure browser content-visibility -->
        <section class="section-container">
          <h2 class="text-xs font-semibold text-neutral-400 uppercase tracking-wider mb-2 sticky top-0 bg-neutral-950/90 py-1.5 z-10">
            {groupName}
          </h2>
          <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-2.5">
            {#each groupList as asset (asset.id)}
              <div
                class="relative rounded-lg overflow-hidden bg-neutral-900 aspect-square contain-paint {recentAssetIds.has(asset.id) ? 'animate-incoming ring-2 ring-blue-500/60' : ''}"
              >
                <MediaCard
                  {asset}
                  isSelected={Boolean($selectedMap[asset.id])}
                  on:open={() => (selectedAssetId = asset.id)}
                  on:select={(e) => selection.toggleSelect(asset.id, e.detail, items, itemIndexMap)}
                />
              </div>
            {/each}
          </div>
        </section>
      {/each}
    </div>
  {/if}

  <div bind:this={scrollTrigger} class="py-6 text-center text-xs text-neutral-600 min-h-[2rem]">
    {#if isLoading}
      <span>Loading more...</span>
    {/if}
  </div>

  <BatchActionBar
    count={$selectedCount}
    isActionLoading={$isActionLoading}
    on:toggleDelete={selection.batchToggleDelete}
    on:purge={selection.batchPurge}
    on:clear={selection.clearSelection}
  />

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
          // Trigger local update on card without reassigning full array
          items = items;
        }
      }}
    />
  {/if}
</div>

<style>
  /* Native browser-level rendering virtualization */
  .section-container {
    content-visibility: auto;
    contain-intrinsic-size: auto 320px;
  }

  /* Isolates box painting for grid cards to prevent global repaints */
  .contain-paint {
    contain: paint;
  }

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