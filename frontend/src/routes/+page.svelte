<!-- photo-app/frontend/src/routes/+page.svelte -->
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fade } from 'svelte/transition';

  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import { albumStore } from '$lib/stores/albumStore';
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

  $: selectedAsset = resolveAsset($sections, activeCoords);
  $: prevAsset = resolveAsset($sections, getPrevCoords($sections, activeCoords));$: nextAsset = resolveAsset($sections, getNextCoords($sections, activeCoords));

  $: currentAlbum =$filterStore.album_id
    ? $albumStore.find((a) => a.id ===$filterStore.album_id)
    : null;

  $: currentAlbumPath = currentAlbum
    ? (currentAlbum.title || '').replace(/^\/+|\/+$/g, '')
    : ($filterStore.folder_path || '').replace(/^\/+|\/+$/g, '');

  $: breadcrumbSegments = (() => {
    if (!currentAlbumPath) return [];
    const parts = currentAlbumPath.split('/');
    let cumulative = '';
    return parts.map((part) => {
      cumulative = cumulative ? `${cumulative}/${part}` : part;
      const matchedAlbum = $albumStore.find(
        (a) => (a.title || '').replace(/^\/+|\/+$/g, '') === cumulative
      );
      return {
        name: part,
        path: cumulative,
        albumId: matchedAlbum?.id ?? null
      };
    });
  })();

  $: rootAlbums =$albumStore
    .map((alb) => {
      const clean = (alb.title || '').replace(/^\/+|\/+$/g, '');
      const parts = clean.split('/');
      return { ...alb, cleanPath: clean, rootName: parts[0] };
    })
    .filter((alb) => !currentAlbumPath && alb.cleanPath === alb.rootName);

  $: childAlbums = (() => {
    if (!currentAlbumPath) return [];
    const prefix = `${currentAlbumPath}/`;
    return $albumStore
      .filter((alb) => {
        const clean = (alb.title || '').replace(/^\/+|\/+$/g, '');
        return clean.startsWith(prefix) && clean !== currentAlbumPath;
      })
      .map((alb) => {
        const clean = (alb.title || '').replace(/^\/+|\/+$/g, '');
        const remainder = clean.slice(prefix.length);
        const directSubName = remainder.split('/')[0];
        return {
          ...alb,
          directSubName,
          displayTitle: directSubName
        };
      })
      .filter((item, index, self) =>
        index === self.findIndex((t) => t.directSubName === item.directSubName)
      );
  })();

  $: scrubMarkers =$sections.map((s, idx) => ({
    label: s.title.split(' ')[0],
    year: s.title.split(' ')[1] || '',
    index: idx
  }));

  $: currentDensity = DENSITY_PRESETS[$gridDensity];

  $: selectedAssetIds = Array.from(
    (selection as any).selectedIds ??
    (typeof (selection as any).getSelectedIds === 'function' ? (selection as any).getSelectedIds() : [])
  );

  const pinchZoom = createPinchZoomHandler(
    () => gridDensity.zoomIn(),
    () => gridDensity.zoomOut(),
    () => activeCoords === null && !$isSelectionActive
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

  function navigateToAlbumSegment(seg: { name: string; path: string; albumId: string | null }) {
    if (seg.albumId) {
      filterStore.setAlbumId(seg.albumId);
    } else {
      filterStore.setFolderPath(seg.path);
    }
  }

  function openChildAlbum(album: any) {
    if (album.id) {
      filterStore.setAlbumId(album.id);
    } else {
      const targetPath = `${currentAlbumPath}/${album.directSubName}`;
      filterStore.setFolderPath(targetPath);
    }
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
  class="px-2 pt-16 md:px-6 md:pt-16 max-w-[1700px] mx-auto space-y-4 md:space-y-5 min-h-full flex flex-col select-none relative transition-all duration-300 ease-out {activeCoords !== null ? 'scale-[0.97] opacity-85 pointer-events-none' : 'scale-100 opacity-100'} {$isSelectionActive ? 'selection-active' : ''}"
>
  <!-- Minimal Header Bar -->
  <div class="flex items-center justify-between gap-2 px-1 min-h-[32px]">
    <div class="flex items-center gap-1.5 text-xs text-[var(--text-muted)] overflow-x-auto no-scrollbar py-0.5">
      <!-- Root Icon Button -->
      <button
        type="button"
        on:click={() => {
          filterStore.clearAlbum();
          filterStore.setFolderPath('');
        }}
        class="w-7 h-7 rounded-full flex items-center justify-center transition-colors cursor-pointer {!currentAlbumPath ? 'bg-[var(--card-bg)] text-[var(--text-main)] shadow-sm border border-[var(--border-glass)]' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
        title="All Photos"
        aria-label="Root view"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
          <circle cx="8.5" cy="8.5" r="1.5"></circle>
          <polyline points="21 15 16 10 5 21"></polyline>
        </svg>
      </button>

      <!-- Path Badges -->
      {#if breadcrumbSegments.length > 0}
        {#each breadcrumbSegments as seg, idx (seg.path)}
          <span class="opacity-30 text-[10px]">/</span>
          {@const isLast = idx === breadcrumbSegments.length - 1}

          <div class="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full liquid-breadcrumb text-[var(--text-main)] text-xs">
            <button
              type="button"
              on:click={() => navigateToAlbumSegment(seg)}
              class="font-medium truncate max-w-[130px] sm:max-w-[200px] hover:text-purple-500 dark:hover:text-purple-400 transition-colors cursor-pointer {isLast ? 'font-semibold text-purple-600 dark:text-purple-300' : ''}"
            >
              {seg.name}
            </button>

            {#if isLast}
              <button
                type="button"
                on:click={() => (showManageAlbumModal = true)}
                class="text-[var(--text-muted)] hover:text-[var(--text-main)] px-0.5 cursor-pointer transition-colors"
                title="Options"
                aria-label="Album options"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 inline" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <circle cx="12" cy="12" r="1"></circle>
                  <circle cx="19" cy="12" r="1"></circle>
                  <circle cx="5" cy="12" r="1"></circle>
                </svg>
              </button>

              <button
                type="button"
                on:click={() => {
                  if (idx === 0) {
                    filterStore.clearAlbum();
                    filterStore.setFolderPath('');
                  } else {
                    navigateToAlbumSegment(breadcrumbSegments[idx - 1]);
                  }
                }}
                class="text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer leading-none"
                title="Back"
                aria-label="Back"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5 inline" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <line x1="18" y1="6" x2="6" y2="18"></line>
                  <line x1="6" y1="6" x2="18" y2="18"></line>
                </svg>
              </button>
            {/if}
          </div>
        {/each}
      {/if}
    </div>

    <!-- Minimal Trash Icon Indicator -->
    {#if $filterStore.show_trash}
      <div class="w-6 h-6 rounded-full bg-red-500/15 border border-red-500/30 text-red-500 dark:text-red-400 flex items-center justify-center flex-shrink-0" title="Viewing Trash">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"></polyline>
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
        </svg>
      </div>
    {/if}
  </div>

  <!-- 1. ROOT ALBUMS (Quiet Grid) -->
  {#if $filterStore.view_mode === 'albums' && !currentAlbumPath && rootAlbums.length > 0}
    <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-2.5" in:fade={{ duration: 150 }}>
      {#each rootAlbums as album (album.id)}
        <button
          type="button"
          on:click={() => filterStore.setAlbumId(album.id)}
          class="group text-left p-1.5 rounded-2xl bg-[var(--card-bg)] hover:bg-[var(--dock-bg-hover)] border border-[var(--border-glass)] hover:border-purple-500/40 transition-all cursor-pointer flex flex-col gap-1.5 spring-tap"
        >
          <div class="w-full aspect-square rounded-xl overflow-hidden bg-black/10 dark:bg-black/40 flex items-center justify-center border border-[var(--border-glass)] relative">
            {#if album.cover_thumb}
              <img
                src={album.cover_thumb.startsWith('/') ? album.cover_thumb : `/${album.cover_thumb}`}
                alt={album.title}
                class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
              />
            {:else}
              <div class="w-7 h-7 text-[var(--text-muted)] opacity-40">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-full h-full" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                </svg>
              </div>
            {/if}
            <div class="absolute bottom-1 right-1 bg-black/65 backdrop-blur-md px-1.5 py-0.2 rounded-md text-[9px] text-white font-mono">
              {album.media_count}
            </div>
          </div>

          <div class="px-1 truncate text-xs font-semibold text-[var(--text-main)] group-hover:text-purple-500 dark:group-hover:text-purple-400 transition-colors">
            {album.rootName}
          </div>
        </button>
      {/each}
    </div>
  {/if}

  <!-- 2. SUB-ALBUMS SHELF (No Section Headers, Pure Minimal Tiles) -->
  {#if currentAlbumPath && childAlbums.length > 0}
    <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-2.5 pb-1" in:fade={{ duration: 150 }}>
      {#each childAlbums as subAlbum (subAlbum.id || subAlbum.directSubName)}
        <button
          type="button"
          on:click={() => openChildAlbum(subAlbum)}
          class="group text-left p-1.5 rounded-2xl bg-[var(--card-bg)] hover:bg-[var(--dock-bg-hover)] border border-[var(--border-glass)] hover:border-purple-500/40 transition-all cursor-pointer flex flex-col gap-1.5 spring-tap shadow-sm"
        >
          <div class="w-full aspect-square rounded-xl overflow-hidden bg-black/10 dark:bg-black/40 flex items-center justify-center border border-[var(--border-glass)] relative">
            {#if subAlbum.cover_thumb}
              <img
                src={subAlbum.cover_thumb.startsWith('/') ? subAlbum.cover_thumb : `/${subAlbum.cover_thumb}`}
                alt={subAlbum.displayTitle}
                class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
              />
            {:else}
              <div class="w-6 h-6 text-[var(--text-muted)] opacity-40">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-full h-full" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                </svg>
              </div>
            {/if}
            <div class="absolute bottom-1 right-1 bg-black/65 backdrop-blur-md px-1.5 py-0.2 rounded-md text-[9px] text-white font-mono">
              {subAlbum.media_count}
            </div>
          </div>

          <div class="px-1 truncate text-xs font-semibold text-[var(--text-main)] group-hover:text-purple-500 dark:group-hover:text-purple-400 transition-colors">
            {subAlbum.displayTitle}
          </div>
        </button>
      {/each}
    </div>
  {/if}

  <!-- 3. MEDIA TIMELINE GRID -->
  {#if $sections.length === 0 && !$isLoading}
    <div in:fade={{ duration: 150 }} class="flex-1 flex flex-col items-center justify-center text-center py-20 text-[var(--text-muted)]">
      <div class="w-8 h-8 opacity-40 mb-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-full h-full" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
          <circle cx="8.5" cy="8.5" r="1.5"></circle>
          <polyline points="21 15 16 10 5 21"></polyline>
        </svg>
      </div>
      <p class="text-xs font-medium">Empty</p>
    </div>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      role="region"
      aria-label="Media grid"
      class="space-y-4 md:space-y-6 select-none {$isSelectionActive ? 'touch-none' : 'touch-pan-y'}"
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
          <!-- Minimal Sticky Timeline Header -->
          <div class="sticky top-0 z-20 py-1.5 px-1 flex items-baseline justify-between backdrop-blur-xl bg-[var(--bg-primary)]/80 mb-1.5">
            <h2 class="text-xs font-semibold tracking-tight text-[var(--text-main)]">
              {section.title}
            </h2>
            <span class="text-[10px] text-[var(--text-muted)] font-mono">
              {section.items.length}
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
                  on:load={(e) => (e.currentTarget as HTMLElement).classList.add('loaded')}
                  class="tile-image w-full h-full object-cover pointer-events-none group-hover:scale-102"
                />

                <button
                  type="button"
                  data-select-btn
                  class="select-btn absolute top-1.5 left-1.5 w-5 h-5 md:w-6 md:h-6 rounded-full flex items-center justify-center transition-all z-30 cursor-pointer bg-black/40 backdrop-blur-md opacity-0 group-hover:opacity-100 text-white hover:text-white border border-white/20"
                  title="Select"
                  aria-label="Select"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 text-white pointer-events-none" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
                    <polyline points="20 6 9 17 4 12"></polyline>
                  </svg>
                </button>

                {#if asset.is_favorite}
                  <div class="absolute top-1.5 right-1.5 bg-black/50 backdrop-blur-md px-1.5 py-1 rounded-full text-amber-300 leading-none z-20 pointer-events-none shadow-sm border border-white/10 flex items-center justify-center">
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5 fill-amber-300 stroke-amber-300" viewBox="0 0 24 24" stroke-width="2">
                      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon>
                    </svg>
                  </div>
                {/if}

                {#if asset.days_remaining !== null && asset.days_remaining !== undefined && $gridDensity > 0}
                  <div class="absolute bottom-1.5 left-1.5 bg-red-500/30 backdrop-blur-md border border-red-500/40 px-1.5 py-0.5 rounded-full text-[9px] text-red-200 font-mono font-medium z-20 pointer-events-none shadow-sm flex items-center gap-1">
                    <span>{asset.days_remaining}d</span>
                  </div>
                {:else if asset.mime_type === 'image/gif' && $gridDensity > 0}
                  <div class="absolute bottom-1.5 left-1.5 bg-black/60 backdrop-blur-md border border-white/15 px-1.5 py-0.5 rounded-md text-[8px] text-white font-mono font-bold tracking-wider z-20 pointer-events-none">
                    GIF
                  </div>
                {/if}

                {#if asset.duration_seconds && $gridDensity > 0}
                  <div class="absolute bottom-1.5 right-1.5 bg-black/60 backdrop-blur-md border border-white/15 px-1.5 py-0.5 rounded-full text-[9px] text-white font-mono z-20 pointer-events-none flex items-center gap-1 shadow-sm">
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-2 h-2 fill-current" viewBox="0 0 24 24">
                      <polygon points="5 3 19 12 5 21 5 3"></polygon>
                    </svg>
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

  <div bind:this={scrollTrigger} class="py-6 text-center text-xs text-[var(--text-muted)] min-h-[2.5rem]">
    {#if $isLoading}
      <div class="w-4 h-4 border-2 border-purple-500/20 border-t-purple-500 rounded-full animate-spin mx-auto"></div>
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
  album={currentAlbum}
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
      if (activeCoords) timeline.patchFavorite(activeCoords, e.detail.is_favorite);
    }}
    on:setAsCover={(e) => {
      if (selectedAsset) handleSetCover(selectedAsset.id, selectedAsset.thumb_path);
    }}
  />
{/if}

<style>
  .liquid-breadcrumb {
    background: var(--dock-bg);
    border: 1px solid var(--dock-border);
    backdrop-filter: blur(20px) saturate(180%);
    box-shadow: 0 2px 8px var(--dock-shadow), inset 0 1px 0 var(--dock-highlight);
  }

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
    -webkit-touch-callout: none;
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