<!-- photo-app/frontend/src/routes/+page.svelte -->
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fade } from 'svelte/transition';

  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import { albumStore } from '$lib/stores/albumStore';
  import { albumNav } from '$lib/stores/albumNavStore';
  import { createMediaSelection } from '$lib/stores/mediaSelection';
  import { createTimelineStore } from '$lib/stores/timelineStore';
  import { gridDensity, DENSITY_PRESETS } from '$lib/stores/gridDensityStore';
  import { initMediaEvents } from '$lib/utils/mediaEvents';
  import { createPinchZoomHandler } from '$lib/utils/pinchZoom';
  import {
    resolveAsset,
    getPrevCoords,
    getNextCoords,
    findCoordsById,
    type Coords
  } from '$lib/utils/coordinateNav';

  import AlbumShelf from '$lib/components/AlbumShelf.svelte';
  import BatchActionBar from '$lib/components/BatchActionBar.svelte';
  import PhotoModal from '$lib/components/PhotoModal.svelte';
  import TimelineScrubber from '$lib/components/TimelineScrubber.svelte';
  import AddToAlbumModal from '$lib/components/AddToAlbumModal.svelte';
  import ManageAlbumModal from '$lib/components/ManageAlbumModal.svelte';

  const timeline = createTimelineStore();
  const { sections, isLoading, hasMore } = timeline;

  const selection = createMediaSelection(() => timeline.fetchMedia($filterQueryString, true));
  const { selectedCount, isSelectionActive, isActionLoading } = selection;

  let activeCoords: Coords | null = null;
  let clickedCardRect: DOMRect | null = null;
  let scrollTrigger: HTMLDivElement;
  let observer: IntersectionObserver | null = null;
  let filterDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  let sseSubscription: { close: () => void } | null = null;

  let showAddToAlbumModal = false;
  let showManageAlbumModal = false;
  let isScrolledDown = false;

  $: selectedAsset = resolveAsset($sections, activeCoords);
  $: prevAsset = resolveAsset($sections, getPrevCoords($sections, activeCoords));$: nextAsset = resolveAsset($sections, getNextCoords($sections, activeCoords));

  $: currentDensity = DENSITY_PRESETS[$gridDensity] ?? DENSITY_PRESETS[0];

  $: selectedSet = (() => {
    void $selectedCount;
    void $isSelectionActive;
    const raw =
      (selection as any).selectedIds ??
      (typeof (selection as any).getSelectedIds === 'function'
        ? (selection as any).getSelectedIds()
        : []);
    return raw instanceof Set ? new Set<string>(raw) : new Set<string>(Array.from(raw || []));
  })();

  $: selectedAssetIds = Array.from(selectedSet);

  const pinchZoom = createPinchZoomHandler(
    () => gridDensity.zoomIn(),
    () => gridDensity.zoomOut(),
    () => activeCoords === null && !$isSelectionActive
  );

  function scrollToSection(index: number) {
    const el = document.getElementById(`section-marker-${index}`);
    if (el) {
      const topOffset = el.getBoundingClientRect().top + window.scrollY - 75;
      window.scrollTo({ top: topOffset, behavior: 'smooth' });
    }
  }

  function handleCardClick(
    e: MouseEvent | KeyboardEvent,
    assetId: string,
    secIdx: number,
    itemIdx: number
  ) {
    const card = e.currentTarget as HTMLElement;
    const target = e.target as HTMLElement;

    if (target.closest('[data-select-btn]') || $isSelectionActive) {
      e.stopPropagation();
      selection.toggle(assetId, card);
      return;
    }

    clickedCardRect = card.getBoundingClientRect();
    activeCoords = [secIdx, itemIdx];
  }

  async function handleRemoveFromAlbum() {
    if (!$filterStore.album_id || selectedAssetIds.length === 0) return;
    const ok = await albumStore.removeItems($filterStore.album_id, selectedAssetIds);
    if (ok) {
      selection.clearSelection();
      timeline.fetchMedia($filterQueryString, true);
      albumStore.load();
    }
  }

  async function handleSetCover(assetId: string, thumbPath: string) {
    if (!$filterStore.album_id) return;
    await albumStore.setCover($filterStore.album_id, assetId, thumbPath);
  }

  function formatTime(seconds?: number | null): string {
    if (!seconds) return '';
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    return `${m}:${s.toString().padStart(2, '0')}`;
  }

  // Pure server-driven scrubber markers
  $: scrubMarkers = (() => {
    if ($filterStore.sort !== 'timeline') return [];
    const seen = new Set<string>();
    const markers: { label: string; year: string; index: number; count?: number }[] = [];

    $sections.forEach((s, idx) => {
      const month = s.month || '';
      const year = s.year || '';
      const key = `${month} ${year}`.trim();

      if (key && !seen.has(key)) {
        seen.add(key);
        markers.push({
          label: month,
          year,
          index: idx,
          count: s.items?.length
        });
      }
    });

    return markers;
  })();

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
    albumStore.load();

    const handleRefresh = () => {
      timeline.fetchMedia($filterQueryString, true);
      albumStore.load();
    };
    window.addEventListener('vault:refresh-timeline', handleRefresh);
    sseSubscription = initMediaEvents(handleRefresh);

    const handleOpenAsset = (e: Event) => {
      const customEvent = e as CustomEvent<{ id: string }>;
      if (!customEvent.detail?.id) return;
      const coords = findCoordsById($sections, customEvent.detail.id);
      if (coords) activeCoords = coords;
    };
    window.addEventListener('vault:open-asset', handleOpenAsset);

    const scrollParent = document.querySelector('main');
    const handleScroll = () => {
      if (scrollParent) isScrolledDown = scrollParent.scrollTop > 80;
    };
    scrollParent?.addEventListener('scroll', handleScroll, { passive: true });

    observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && $hasMore && !$isLoading) {
          timeline.fetchMedia($filterQueryString, false);
        }
      },
      { rootMargin: '1800px 0px', threshold: 0.01 }
    );

    if (scrollTrigger) observer.observe(scrollTrigger);

    return () => {
      window.removeEventListener('vault:refresh-timeline', handleRefresh);
      window.removeEventListener('vault:open-asset', handleOpenAsset);
      scrollParent?.removeEventListener('scroll', handleScroll);
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
  style="padding-bottom: max(3.5rem, calc(var(--sab) + 2.5rem));"
  class="px-2 pt-14 sm:px-4 md:px-8 max-w-[1920px] mx-auto min-h-screen flex flex-col select-none relative transition-transform duration-300 ease-out {activeCoords !== null ? 'scale-[0.985] opacity-80 pointer-events-none' : 'scale-100 opacity-100'}"
>
  <!-- Modular Album Header & Shelf -->
  <AlbumShelf
    isScrolled={isScrolledDown}
    on:manageAlbum={() => (showManageAlbumModal = true)}
  />

  <!-- MAIN PHOTO GRID -->
  {#if $sections.length === 0 && !$isLoading}
    <div in:fade={{ duration: 180 }} class="flex-1 flex flex-col items-center justify-center text-center py-28 text-[var(--text-muted)]">
      <div class="w-12 h-12 opacity-30 mb-3">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-full h-full" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <rect x="3" y="3" width="18" height="18" rx="3" ry="3"></rect>
          <circle cx="8.5" cy="8.5" r="1.5"></circle>
          <polyline points="21 15 16 10 5 21"></polyline>
        </svg>
      </div>
      <p class="text-sm font-semibold tracking-tight text-[var(--text-main)]">No Media Found</p>
      <p class="text-xs text-[var(--text-muted)] mt-1">Try clearing filters</p>
    </div>
  {:else}
    <!-- UNIFIED SERVER-DRIVEN STREAM -->
    <div
      role="region"
      aria-label="Media grid"
      class="space-y-6 md:space-y-7 select-none touch-pan-y"
      style="--grid-cols: {currentDensity.cols}; --grid-cols-mobile: {currentDensity.colsMobile};"
    >
      {#each $sections as section, secIdx (section.id)}
        <section id="section-marker-{secIdx}" class="section-container">
          <!-- Server-controlled title: renders ONLY when section.title is non-null -->
          {#if section.title}
            <div class="pt-2 pb-1.5 px-0.5 flex items-baseline justify-between mb-1.5">
              <h2 class="text-xs sm:text-sm font-semibold tracking-tight text-[var(--text-main)]">
                {section.title}
              </h2>
              <span class="text-[10px] font-mono text-[var(--text-muted)] opacity-60">
                {section.items.length}
              </span>
            </div>
          {/if}

          <!-- Grid layout -->
          <div class="gallery-grid">
            {#each section.items as asset, itemIdx (asset.id)}
              {@const isPriority = secIdx === 0 && itemIdx < 20}
              {@const isSelected = selectedSet.has(asset.id)}

              <div
                role="button"
                tabindex="0"
                aria-pressed={isSelected}
                aria-label={asset.file_name}
                data-asset-id={asset.id}
                on:click={(e) => handleCardClick(e, asset.id, secIdx, itemIdx)}
                on:keydown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') handleCardClick(e, asset.id, secIdx, itemIdx);
                }}
                class="tile-card group relative aspect-square rounded-lg overflow-hidden cursor-pointer focus:outline-none transition-transform duration-200 {isSelected ? 'scale-[0.92]' : 'hover:scale-[1.01]'}"
              >
                <!-- Thumbnail -->
                <img
                  src={asset.thumb_path.startsWith('/') ? asset.thumb_path : `/${asset.thumb_path}`}
                  alt={asset.file_name}
                  loading={isPriority ? 'eager' : 'lazy'}
                  decoding="async"
                  fetchpriority={isPriority ? 'high' : 'auto'}
                  on:load={(e) => (e.currentTarget as HTMLElement).classList.add('loaded')}
                  class="tile-image w-full h-full object-cover pointer-events-none rounded-lg"
                />

                <!-- Selection Inset Ring -->
                {#if isSelected}
                  <div class="pointer-events-none absolute inset-0 z-10 rounded-lg ring-3 ring-purple-500 ring-inset bg-purple-500/10"></div>
                {/if}

                <!-- Checkmark Badge -->
                <button
                  type="button"
                  data-select-btn
                  class="select-btn absolute top-1.5 left-1.5 w-6 h-6 rounded-full flex items-center justify-center transition-all z-20 cursor-pointer active:scale-90 {isSelected ? 'opacity-100 scale-100 bg-purple-600 text-white shadow-md border-2 border-white' : ($isSelectionActive ? 'opacity-100 scale-95 bg-black/40 border border-white/70 text-transparent' : 'opacity-0 scale-90 group-hover:opacity-100 bg-black/40 border border-white/70 text-transparent')}"
                  title="Select"
                  aria-label="Select photo"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 pointer-events-none stroke-[3] {isSelected ? 'opacity-100 text-white' : 'opacity-0'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">
                    <polyline points="20 6 9 17 4 12"></polyline>
                  </svg>
                </button>

                <!-- Favorite Badge -->
                {#if asset.is_favorite}
                  <div class="absolute top-1.5 right-1.5 bg-black/55 backdrop-blur-md px-1.5 py-1 rounded-full text-amber-300 z-10 pointer-events-none shadow-sm border border-white/10 flex items-center justify-center">
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5 fill-amber-300 stroke-amber-300" viewBox="0 0 24 24" stroke-width="2">
                      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon>
                    </svg>
                  </div>
                {/if}

                <!-- Video Duration HUD -->
                {#if asset.duration_seconds && $gridDensity > 0}
                  <div class="absolute bottom-1.5 right-1.5 bg-black/70 backdrop-blur-md border border-white/15 px-2 py-0.5 rounded-md text-[9px] text-white font-mono font-medium z-10 pointer-events-none flex items-center gap-1 shadow-sm">
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-2 h-2 fill-current" viewBox="0 0 24 24">
                      <polygon points="5 3 19 12 5 21 5 3"></polygon>
                    </svg>
                    <span>{formatTime(asset.duration_seconds)}</span>
                  </div>
                {/if}

                <!-- Days Remaining in Trash -->
                {#if asset.days_remaining !== null && asset.days_remaining !== undefined && $gridDensity > 0}
                  <div class="absolute bottom-1.5 left-1.5 bg-rose-600/75 backdrop-blur-md border border-rose-400/40 px-2 py-0.5 rounded-md text-[9px] text-white font-mono font-medium z-10 pointer-events-none shadow-sm">
                    <span>{asset.days_remaining}d left</span>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </section>
      {/each}
    </div>
  {/if}

  <!-- Pre-fetch Scroll Anchor -->
  <div bind:this={scrollTrigger} class="py-12 flex justify-center items-center min-h-[4rem]">
    {#if $isLoading}
      <div class="flex items-center gap-2 text-xs font-mono text-[var(--text-muted)] bg-[var(--card-bg)] px-4 py-2 rounded-full border border-[var(--border-glass)] shadow-sm">
        <div class="w-3.5 h-3.5 border-2 border-purple-500/30 border-t-purple-500 rounded-full animate-spin"></div>
        <span>Loading...</span>
      </div>
    {/if}
  </div>
</div>

<!-- Scrubber (Timeline mode only) -->
{#if $filterStore.sort === 'timeline'}
  <TimelineScrubber markers={scrubMarkers} on:jump={(e) => scrollToSection(e.detail.index)} />
{/if}

<!-- Actions and Modals -->
<BatchActionBar
  count={$selectedCount}
  isActionLoading={$isActionLoading}
  on:toggleDelete={selection.batchToggleDelete}
  on:purge={selection.batchPurge}
  on:addToAlbum={() => (showAddToAlbumModal = true)}
  on:removeFromAlbum={handleRemoveFromAlbum}
  on:clear={selection.clearSelection}
/>

{#if showAddToAlbumModal}
  <AddToAlbumModal
    assetIds={selectedAssetIds}
    on:close={() => (showAddToAlbumModal = false)}
    on:completed={() => {
      showAddToAlbumModal = false;
      selection.clearSelection();
      timeline.fetchMedia($filterQueryString, true);
      albumStore.load();
    }}
  />
{/if}

<ManageAlbumModal
  isOpen={showManageAlbumModal}
  album={$albumNav.currentAlbum}
  on:close={() => (showManageAlbumModal = false)}
  on:updated={() => albumStore.load()}
  on:deleted={() => {
    timeline.fetchMedia($filterQueryString, true);
    albumStore.load();
  }}
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
      if (selectedAsset) timeline.patchFavorite(selectedAsset.id, e.detail.is_favorite);
    }}
    on:setAsCover={() => {
      if (selectedAsset) handleSetCover(selectedAsset.id, selectedAsset.thumb_path);
    }}
  />
{/if}

<style>
  .section-container {
    width: 100%;
    contain: layout style;
  }

  .gallery-grid {
    display: grid !important;
    width: 100%;
    gap: 0.25rem;
    grid-template-columns: repeat(var(--grid-cols-mobile, 3), minmax(0, 1fr));
  }

  @media (min-width: 640px) {
    .gallery-grid {
      gap: 0.375rem;
      grid-template-columns: repeat(var(--grid-cols, 5), minmax(0, 1fr));
    }
  }

  @media (min-width: 1024px) {
    .gallery-grid {
      gap: 0.5rem;
    }
  }

  .tile-card {
    position: relative;
    width: 100%;
    min-width: 0;
    aspect-ratio: 1 / 1;
    overflow: hidden;
    -webkit-touch-callout: none;
    background-color: var(--card-bg, #1e1e24);
  }

  .select-btn {
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    transition: transform 0.18s cubic-bezier(0.34, 1.56, 0.64, 1), background-color 0.15s ease, opacity 0.15s ease;
  }

  .tile-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 1;
    transition: transform 0.2s cubic-bezier(0.16, 1, 0.3, 1);
  }

  :global(.tile-image.loaded) {
    opacity: 1;
  }
</style>