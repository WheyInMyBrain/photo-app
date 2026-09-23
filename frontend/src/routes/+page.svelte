<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fade } from 'svelte/transition';

  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import { createMediaSelection } from '$lib/stores/mediaSelection';
  import { createTimelineStore } from '$lib/stores/timelineStore';
  import { gridDensity, DENSITY_PRESETS } from '$lib/stores/gridDensityStore';
  import { initMediaEvents } from '$lib/utils/mediaEvents';
  import { createPinchZoomHandler } from '$lib/utils/pinchZoom';
  import { createDragSelectHandler } from '$lib/utils/dragSelect';
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
  import TimelineScrubber from '$lib/components/TimelineScrubber.svelte';

  const timeline = createTimelineStore();
  const { sections, albums, isLoading, hasMore } = timeline;

  const selection = createMediaSelection(() => timeline.fetchMedia($filterQueryString, true));
  const { selectedCount, isSelectionActive, isActionLoading } = selection;

  let activeCoords: Coords | null = null;
  let clickedCardRect: DOMRect | null = null;
  let scrollTrigger: HTMLDivElement;
  let observer: IntersectionObserver | null = null;
  let filterDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  let sseSubscription: { close: () => void } | null = null;

  $: selectedAsset = resolveAsset($sections, activeCoords);
  $: prevAsset = resolveAsset($sections, getPrevCoords($sections, activeCoords));$: nextAsset = resolveAsset($sections, getNextCoords($sections, activeCoords));

  $: folderSegments =$filterStore.folder_path
    ? $filterStore.folder_path.split('/').filter(Boolean)
    : [];

  $: scrubMarkers =$sections.map((s, idx) => ({
    label: s.title.split(' ')[0],
    year: s.title.split(' ')[1] || '',
    index: idx
  }));

  $: currentDensity = DENSITY_PRESETS[$gridDensity];

  const pinchZoom = createPinchZoomHandler(
    () => gridDensity.zoomIn(),
    () => gridDensity.zoomOut(),
    () => activeCoords === null
  );

  const dragSelect = createDragSelectHandler({
    isSelectionActive: () => $isSelectionActive,
    toggle: selection.toggle,
    setTargetState: selection.setTargetState
  });

  function scrollToSection(index: number) {
    const el = document.getElementById(`section-marker-${index}`);
    if (el) {
      el.scrollIntoView({ behavior: 'auto', block: 'start' });
    }
  }

  function handleContainerClick(e: MouseEvent) {
    if (dragSelect.isDragging()) return;

    const target = e.target as HTMLElement;
    const card = target.closest<HTMLElement>('[data-asset-id]');
    if (!card) return;

    const assetId = card.dataset.assetId!;
    const secIdx = parseInt(card.dataset.secIdx!, 10);
    const itemIdx = parseInt(card.dataset.itemIdx!, 10);

    if (target.closest('[data-select-btn]')) {
      e.stopPropagation();
      selection.toggle(assetId, card);
      return;
    }

    if ($isSelectionActive) {
      selection.toggle(assetId, card);
      return;
    }

    clickedCardRect = card.getBoundingClientRect();
    activeCoords = [secIdx, itemIdx];
  }

  function handleContainerKeydown(e: KeyboardEvent) {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    const target = e.target as HTMLElement;
    const card = target.closest<HTMLElement>('[data-asset-id]');
    if (!card) return;

    const assetId = card.dataset.assetId!;
    const secIdx = parseInt(card.dataset.secIdx!, 10);
    const itemIdx = parseInt(card.dataset.itemIdx!, 10);

    if ($isSelectionActive) {
      selection.toggle(assetId, card);
      return;
    }

    clickedCardRect = card.getBoundingClientRect();
    activeCoords = [secIdx, itemIdx];
  }

  $: if ($filterQueryString !== undefined) {
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    filterDebounceTimer = setTimeout(() => {
      selection.clearSelection();
      activeCoords = null;
      clickedCardRect = null;
      timeline.fetchMedia($filterQueryString, true);
    }, 180);
  }

  onMount(() => {
    const handleRefresh = () => timeline.fetchMedia($filterQueryString, true);
    window.addEventListener('vault:refresh-timeline', handleRefresh);
    sseSubscription = initMediaEvents(handleRefresh);

    const handleOpenAsset = (e: Event) => {
      const customEvent = e as CustomEvent<{ id: string }>;
      if (!customEvent.detail?.id) return;
      const coords = findCoordsById($sections, customEvent.detail.id);
      if (coords) {
        activeCoords = coords;
      }
    };
    window.addEventListener('vault:open-asset', handleOpenAsset);

    observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && $hasMore && !$isLoading) {
          timeline.fetchMedia($filterQueryString, false);
        }
      },
      { rootMargin: '900px 0px' }
    );

    if (scrollTrigger) observer.observe(scrollTrigger);

    return () => {
      window.removeEventListener('vault:refresh-timeline', handleRefresh);
      window.removeEventListener('vault:open-asset', handleOpenAsset);
    };
  });

  onDestroy(() => {
    timeline.destroy();
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    if (observer) observer.disconnect();
    if (sseSubscription) sseSubscription.close();
  });
</script>

<svelte:window
  on:touchstart={pinchZoom.handleTouchStart}
  on:touchmove={pinchZoom.handleTouchMove}
  on:touchend={pinchZoom.handleTouchEnd}
  on:wheel|nonpassive={pinchZoom.handleWheel}
/>

<div
  style="padding-bottom: max(2rem, calc(var(--sab) + 1.5rem));"
  class="px-2 pt-14 md:px-6 md:pt-6 max-w-[1700px] mx-auto space-y-4 md:space-y-6 min-h-full flex flex-col select-none relative transition-all duration-300 ease-out {activeCoords !== null ? 'scale-[0.97] opacity-85 pointer-events-none' : 'scale-100 opacity-100'} {$isSelectionActive ? 'selection-active' : ''}"
>
  <div class="flex items-center justify-between gap-2 px-1">
    <div class="flex items-center gap-1.5 text-xs text-[var(--text-muted)] overflow-x-auto no-scrollbar py-1">
      <button
        type="button"
        on:click={() => filterStore.setFolderPath('')}
        class="hover:text-[var(--text-main)] transition-colors cursor-pointer font-medium whitespace-nowrap"
      >
        Root
      </button>
      {#each folderSegments as seg, i}
        <span class="opacity-40">/</span>
        <button
          type="button"
          on:click={() => filterStore.setFolderPath(folderSegments.slice(0, i + 1).join('/'))}
          class="hover:text-[var(--text-main)] transition-colors cursor-pointer whitespace-nowrap {i === folderSegments.length - 1 ? 'text-[var(--text-main)] font-semibold' : ''}"
        >
          {seg}
        </button>
      {/each}
    </div>

    {#if $filterStore.show_trash}
      <span class="text-[10px] bg-red-500/15 border border-red-500/30 text-red-400 px-2.5 py-0.5 rounded-full font-mono font-medium whitespace-nowrap flex-shrink-0">
        Trash: 30d Auto-Purge
      </span>
    {/if}
  </div>

  <FolderGrid albums={$albums} />

  {#if $sections.length === 0 && $albums.length === 0 && !$isLoading}
    <div in:fade={{ duration: 150 }} class="flex-1 flex flex-col items-center justify-center text-center py-24 text-[var(--text-muted)] text-xs">
      <div class="text-4xl mb-3 opacity-60">{$filterStore.show_trash ? '🗑️' : '📷'}</div>
      <p class="font-medium text-sm">{$filterStore.show_trash ? 'Trash is empty.' : 'No media found in this view.'}</p>
    </div>
  {:else}
    <div
      role="region"
      aria-label="Media timeline grid"
      class="space-y-6 md:space-y-8 touch-pan-y"
      style="--grid-cols: {currentDensity.cols}; --grid-cols-mobile: {currentDensity.colsMobile};"
      on:click={handleContainerClick}
      on:keydown={handleContainerKeydown}
      on:pointerdown={dragSelect.handlePointerDown}
      on:pointermove={dragSelect.handlePointerMove}
      on:pointerup={dragSelect.handlePointerUp}
      on:pointercancel={dragSelect.handlePointerCancel}
    >
      {#each $sections as section, secIdx (section.title)}
        <section id="section-marker-{secIdx}" class="section-container">
          <div class="sticky top-0 z-20 py-2.5 px-1 flex items-center justify-between backdrop-blur-xl bg-[var(--bg-primary)]/80 border-b border-[var(--border-glass)] mb-2.5">
            <h2 class="text-xs md:text-sm font-semibold tracking-tight text-[var(--text-main)]">
              {section.title}
            </h2>
            <span class="text-[10px] text-[var(--text-muted)] font-mono tracking-wide">
              {section.items.length} {section.items.length === 1 ? 'item' : 'items'}
            </span>
          </div>

          <div class="gallery-grid">
            {#each section.items as asset, itemIdx (asset.id)}
              {@const ratio = asset.aspect_ratio || 1.0}

              <div
                role="button"
                tabindex="0"
                aria-pressed="false"
                aria-label={asset.file_name}
                data-asset-id={asset.id}
                data-sec-idx={secIdx}
                data-item-idx={itemIdx}
                style="--ratio: {ratio};"
                class="tile-card group relative rounded-lg md:rounded-xl overflow-hidden tile-shimmer spring-tap cursor-pointer focus:outline-none focus:ring-2 focus:ring-purple-400 border border-[var(--border-glass)]"
              >
                <img
                  src={asset.thumb_path.startsWith('/') ? asset.thumb_path : `/${asset.thumb_path}`}
                  alt={asset.file_name}
                  loading="lazy"
                  decoding="async"
                  on:load={(e) => e.currentTarget.classList.add('loaded')}
                  class="tile-image w-full h-full object-cover pointer-events-none group-hover:scale-102"
                />

                <button
                  type="button"
                  data-select-btn
                  class="select-btn absolute top-1.5 left-1.5 w-5 h-5 md:w-6 md:h-6 rounded-full flex items-center justify-center transition-all z-30 cursor-pointer bg-black/35 backdrop-blur-md opacity-0 group-hover:opacity-100 text-white/80 hover:text-white border border-white/20"
                  title="Select media"
                  aria-label="Select {asset.file_name}"
                >
                  <span class="text-[10px] md:text-[11px] font-bold leading-none pointer-events-none">✓</span>
                </button>

                {#if asset.is_favorite}
                  <div class="absolute top-1.5 right-1.5 bg-black/40 backdrop-blur-md px-1.5 py-0.5 rounded-full text-amber-300 text-[10px] leading-none z-20 pointer-events-none shadow-sm border border-white/10">
                    ★
                  </div>
                {/if}

                {#if asset.days_remaining !== null && asset.days_remaining !== undefined && $gridDensity > 0}
                  <div class="absolute bottom-1.5 left-1.5 bg-red-500/20 backdrop-blur-md border border-red-500/30 px-1.5 py-0.5 rounded-full text-[9px] text-red-300 font-mono font-medium z-20 pointer-events-none shadow-sm">
                    🗑️ {asset.days_remaining}d
                  </div>
                {:else if asset.mime_type === 'image/gif' && $gridDensity > 0}
                  <div class="absolute bottom-1.5 left-1.5 bg-black/50 backdrop-blur-md border border-white/10 px-2 py-0.5 rounded-full text-[8px] text-white font-mono font-bold tracking-wider z-20 pointer-events-none">
                    GIF
                  </div>
                {/if}

                {#if asset.duration_seconds && $gridDensity > 0}
                  <div class="absolute bottom-1.5 right-1.5 bg-black/50 backdrop-blur-md border border-white/10 px-1.5 py-0.5 rounded-full text-[9px] text-white font-mono z-20 pointer-events-none flex items-center gap-1 shadow-sm">
                    <span class="text-[7px]">▶</span>
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

  <div bind:this={scrollTrigger} class="py-8 text-center text-xs text-[var(--text-muted)] min-h-[3rem]">
    {#if $isLoading}
      <div class="inline-flex items-center gap-2 font-mono">
        <div class="w-4 h-4 border-2 border-purple-500/20 border-t-purple-500 rounded-full animate-spin"></div>
        <span>Loading library...</span>
      </div>
    {/if}
  </div>
</div>

<TimelineScrubber
  markers={scrubMarkers}
  on:jump={(e) => scrollToSection(e.detail.index)}
/>

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
    hasPrev={prevAsset !== null}
    hasNext={nextAsset !== null}
    initialRect={clickedCardRect}
    on:close={() => {
      activeCoords = null;
      clickedCardRect = null;
    }}
    on:prev={() => {
      clickedCardRect = null;
      activeCoords = getPrevCoords($sections, activeCoords);
    }}
    on:next={() => {
      clickedCardRect = null;
      activeCoords = getNextCoords($sections, activeCoords);
    }}
    on:selectAsset={(e) => {
      clickedCardRect = null;
      const found = findCoordsById($sections, e.detail.id);
      if (found) activeCoords = found;
    }}
    on:toggleFavorite={(e) => {
      if (activeCoords) timeline.patchFavorite(activeCoords, e.detail.is_favorite);
    }}
  />
{/if}

<style>
  .section-container {
    content-visibility: auto;
    contain-intrinsic-size: auto 380px;
  }

  .gallery-grid {
    display: grid;
    gap: 0.375rem;
    grid-template-columns: repeat(var(--grid-cols-mobile, 4), minmax(0, 1fr));
  }

  @media (min-width: 768px) {
    .gallery-grid {
      gap: 0.625rem;
      grid-template-columns: repeat(var(--grid-cols, 6), minmax(0, 1fr));
    }
  }

  .tile-card {
    position: relative;
    width: 100%;
    aspect-ratio: var(--ratio, 1);
    contain: layout paint;
  }

  :global(.tile-card[aria-pressed="true"]) {
    box-shadow: 0 0 0 3px #9333ea, 0 12px 24px -6px rgba(147, 51, 234, 0.4);
  }

  :global(.tile-card[aria-pressed="true"] .select-btn) {
    opacity: 1 !important;
    background-color: #9333ea !important;
    border-color: #ffffff !important;
    color: #ffffff !important;
  }

  :global(.selection-active .tile-card .select-btn) {
    opacity: 1;
  }

  .tile-image {
    opacity: 0;
    transition: opacity 0.25s ease-out;
  }

  :global(.tile-image.loaded) {
    opacity: 1;
  }

  .no-scrollbar::-webkit-scrollbar {
    display: none;
  }
  .no-scrollbar {
    -ms-overflow-style: none;
    scrollbar-width: none;
  }
</style>