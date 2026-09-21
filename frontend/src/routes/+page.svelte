<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fade } from 'svelte/transition';

  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import { createMediaSelection } from '$lib/stores/mediaSelection';
  import { createTimelineStore } from '$lib/stores/timelineStore';
  import { initMediaEvents } from '$lib/utils/mediaEvents';
  import {
    resolveAsset,
    getPrevCoords,
    getNextCoords,
    findCoordsById,
    type Coords
  } from '$lib/utils/coordinateNav';

  import FolderGrid from '$lib/components/FolderGrid.svelte';
  import BatchActionBar from '$lib/components/BatchActionBar.svelte';
  import PhotoModal from '$lib/components/PhotoModal.svelte';

  const timeline = createTimelineStore();
  const { sections, albums, isLoading, hasMore } = timeline;

  const selection = createMediaSelection(() => timeline.fetchMedia($filterQueryString, true));
  const { selectedIds, selectedCount, isActionLoading } = selection;

  let activeCoords: Coords | null = null;
  let scrollTrigger: HTMLDivElement;
  let observer: IntersectionObserver | null = null;
  let filterDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  let sseSubscription: { close: () => void } | null = null;

  $: selectedAsset = resolveAsset($sections, activeCoords);
  $: prevAsset = resolveAsset($sections, getPrevCoords($sections, activeCoords));$: nextAsset = resolveAsset($sections, getNextCoords($sections, activeCoords));

  $: folderSegments =$filterStore.folder_path
    ? $filterStore.folder_path.split('/').filter(Boolean)
    : [];

  function handleContainerClick(e: MouseEvent) {
    const target = e.target as HTMLElement;
    const card = target.closest<HTMLElement>('[data-asset-id]');
    if (!card) return;

    const assetId = card.dataset.assetId!;
    const secIdx = parseInt(card.dataset.secIdx!, 10);
    const itemIdx = parseInt(card.dataset.itemIdx!, 10);

    if (target.closest('[data-select-btn]')) {
      e.stopPropagation();
      selection.toggle(assetId);
      return;
    }
    activeCoords = [secIdx, itemIdx];
  }

  $: if ($filterQueryString !== undefined) {
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    filterDebounceTimer = setTimeout(() => {
      selection.clearSelection();
      activeCoords = null;
      timeline.fetchMedia($filterQueryString, true);
    }, 200);
  }

  onMount(() => {
    const handleRefresh = () => timeline.fetchMedia($filterQueryString, true);
    window.addEventListener('vault:refresh-timeline', handleRefresh);
    sseSubscription = initMediaEvents(handleRefresh);

    observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && $hasMore && !$isLoading) {
          timeline.fetchMedia($filterQueryString, false);
        }
      },
      { rootMargin: '800px 0px' }
    );

    if (scrollTrigger) observer.observe(scrollTrigger);

    return () => {
      window.removeEventListener('vault:refresh-timeline', handleRefresh);
    };
  });

  onDestroy(() => {
    timeline.destroy();
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    if (observer) observer.disconnect();
    if (sseSubscription) sseSubscription.close();
  });

  function handleContainerKeydown(e: KeyboardEvent) {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    const target = e.target as HTMLElement;
    const card = target.closest<HTMLElement>('[data-asset-id]');
    if (!card) return;

    const secIdx = parseInt(card.dataset.secIdx!, 10);
    const itemIdx = parseInt(card.dataset.itemIdx!, 10);
    activeCoords = [secIdx, itemIdx];
  }
</script>

<div class="px-2 pt-14 pb-[max(6rem,env(safe-area-inset-bottom))] md:p-6 max-w-7xl mx-auto space-y-4 md:space-y-6 min-h-full flex flex-col select-none">
  <div class="flex items-center justify-between gap-2 px-1">
    <div class="flex items-center gap-1.5 text-xs text-neutral-400 overflow-x-auto no-scrollbar py-1">
      <button
        type="button"
        on:click={() => filterStore.setFolderPath('')}
        class="hover:text-white transition-colors cursor-pointer font-medium whitespace-nowrap"
      >
        Root
      </button>
      {#each folderSegments as seg, i}
        <span class="text-neutral-600">/</span>
        <button
          type="button"
          on:click={() => filterStore.setFolderPath(folderSegments.slice(0, i + 1).join('/'))}
          class="hover:text-white transition-colors cursor-pointer whitespace-nowrap {i === folderSegments.length - 1 ? 'text-white font-semibold' : ''}"
        >
          {seg}
        </button>
      {/each}
    </div>

    {#if $filterStore.show_trash}
      <span class="text-[11px] bg-red-950/80 border border-red-800/80 text-red-300 px-2 py-0.5 rounded-full font-medium whitespace-nowrap flex-shrink-0">
        Trash: 30d Auto-Purge
      </span>
    {/if}
  </div>

  <FolderGrid albums={$albums} />

  {#if $sections.length === 0 && $albums.length === 0 && !$isLoading}
    <div in:fade={{ duration: 150 }} class="flex-1 flex flex-col items-center justify-center text-center py-20 text-neutral-500 text-xs">
      <div class="text-3xl mb-2">{$filterStore.show_trash ? '🗑️' : '📷'}</div>
      {#if $filterStore.show_trash}Trash is empty.{:else}No media found in this view.{/if}
    </div>
  {:else}
    <div
      role="region"
      aria-label="Media timeline grid"
      class="space-y-4 md:space-y-6"
      on:click={handleContainerClick}
      on:keydown={handleContainerKeydown}
    >
      {#each $sections as section, secIdx (section.title)}
        <section class="section-container">
          <h2 class="text-[11px] md:text-xs font-semibold text-neutral-400 uppercase tracking-wider mb-1.5 md:mb-2 sticky top-0 bg-neutral-950/85 backdrop-blur-md py-2 px-1 z-10">
            {section.title}
          </h2>

          <div class="flex flex-wrap gap-1.5 md:gap-2 justify-start after:content-[''] after:flex-grow-[999999999]">
            {#each section.items as asset, itemIdx (asset.id)}
              {@const isSelected = $selectedIds.has(asset.id)}
              {@const ratio = asset.aspect_ratio || 1.0}

              <div
                role="button"
                tabindex="0"
                aria-label="{asset.file_name}"
                data-asset-id={asset.id}
                data-sec-idx={secIdx}
                data-item-idx={itemIdx}
                style="flex-grow: {ratio * 100}; flex-basis: {ratio * 130}px; aspect-ratio: {ratio}; contain: layout style paint;"
                class="group relative rounded-md md:rounded-lg overflow-hidden bg-neutral-900 border transition-all cursor-pointer min-w-[80px] max-h-[260px] md:max-h-[320px] focus:outline-none focus:ring-2 focus:ring-purple-400 {isSelected ? 'border-purple-500 ring-2 ring-purple-500/40' : 'border-neutral-800/80 hover:border-neutral-700'}"
              >
                <img
                  src={asset.thumb_path.startsWith('/') ? asset.thumb_path : `/${asset.thumb_path}`}
                  alt={asset.file_name}
                  loading="lazy"
                  decoding="async"
                  class="w-full h-full object-cover transition-transform duration-300 group-hover:scale-102 pointer-events-none"
                />

                <button
                  type="button"
                  data-select-btn
                  class="absolute top-1.5 left-1.5 w-5 h-5 rounded-md flex items-center justify-center transition-all z-30 cursor-pointer {isSelected ? 'bg-purple-600 text-white' : 'bg-black/40 text-transparent hover:bg-black/70 hover:text-neutral-400 border border-neutral-700/60'}"
                  title="Select media"
                  aria-label="Select {asset.file_name}"
                >
                  <span class="text-xs font-bold leading-none pointer-events-none">✓</span>
                </button>

                {#if asset.is_favorite}
                  <div class="absolute top-1.5 right-1.5 bg-black/60 p-1 rounded-full text-amber-400 text-[10px] leading-none z-20 pointer-events-none">★</div>
                {/if}

                {#if asset.days_remaining !== null && asset.days_remaining !== undefined}
                  <div class="absolute bottom-1.5 left-1.5 bg-red-950/90 border border-red-800/80 px-1.5 py-0.5 rounded text-[9px] text-red-300 font-mono z-20 pointer-events-none">
                    🗑️ {asset.days_remaining}d left
                  </div>
                {:else if asset.mime_type === 'image/gif'}
                  <div class="absolute bottom-1.5 left-1.5 bg-black/75 px-1.5 py-0.5 rounded text-[9px] text-neutral-200 font-mono font-medium tracking-wider z-20 pointer-events-none">
                    GIF
                  </div>
                {/if}

                {#if asset.duration_seconds}
                  <div class="absolute bottom-1.5 right-1.5 bg-black/75 px-1.5 py-0.5 rounded text-[9px] text-white font-mono z-20 pointer-events-none flex items-center gap-1">
                    <span>▶</span>
                    <span>{Math.floor(asset.duration_seconds / 60)}:{Math.floor(asset.duration_seconds % 60).toString().padStart(2, '0')}</span>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </section>
      {/each}
    </div>
  {/if}

  <div bind:this={scrollTrigger} class="py-6 text-center text-xs text-neutral-600 min-h-[2rem]">
    {#if $isLoading}<span>Loading more...</span>{/if}
  </div>

  <BatchActionBar
    count={$selectedCount}
    isActionLoading={$isActionLoading}
    on:toggleDelete={selection.batchToggleDelete}
    on:purge={selection.batchPurge}
    on:clear={selection.clearSelection}
  />

  {#if selectedAsset && activeCoords !== null}
    <PhotoModal
      asset={selectedAsset}
      {prevAsset}
      {nextAsset}
      hasPrev={prevAsset !== null}
      hasNext={nextAsset !== null}
      on:close={() => (activeCoords = null)}
      on:prev={() => (activeCoords = getPrevCoords($sections, activeCoords))}
      on:next={() => (activeCoords = getNextCoords($sections, activeCoords))}
      on:selectAsset={(e) => {
        const found = findCoordsById($sections, e.detail.id);
        if (found) activeCoords = found;
      }}
      on:toggleFavorite={(e) => {
        if (activeCoords) timeline.patchFavorite(activeCoords, e.detail.is_favorite);
      }}
    />
  {/if}
</div>

<style>
  .section-container {
    content-visibility: auto;
    contain-intrinsic-size: auto 320px;
    contain: layout style paint;
  }
  .no-scrollbar::-webkit-scrollbar {
    display: none;
  }
  .no-scrollbar {
    -ms-overflow-style: none;
    scrollbar-width: none;
  }
</style>