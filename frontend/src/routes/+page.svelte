<!-- photo-app/frontend/src/routes/+page.svelte -->
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fade, scale } from 'svelte/transition';

  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import { albumStore } from '$lib/stores/albumStore';
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

  // Touch disambiguation to prevent accidental selections while scrolling
  let touchStartX = 0;
  let touchStartY = 0;
  let isScrollDrag = false;

  $: selectedAsset = resolveAsset($sections, activeCoords);
  $: prevAsset = resolveAsset($sections, getPrevCoords($sections, activeCoords));
  $: nextAsset = resolveAsset($sections, getNextCoords($sections, activeCoords));

  $: currentAlbum = $filterStore.album_id
    ? $albumStore.find((a) => a.id === $filterStore.album_id)
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

  $: rootAlbums = $albumStore
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

  $: scrubMarkers = (() => {
    const seen = new Set<string>();
    const markers: { label: string; year: string; index: number; count?: number }[] = [];

    $sections.forEach((s, idx) => {
      let month = (s as any).month;
      let year = (s as any).year;

      if (!month || !year) {
        const parts = s.title.split(' ');
        month = parts[parts.length - 2] || '';
        year = parts[parts.length - 1] || '';
      }

      const key = `${month} ${year}`.trim();
      if (key && !seen.has(key)) {
        seen.add(key);
        markers.push({
          label: month,
          year: year,
          index: idx,
          count: s.items?.length
        });
      }
    });

    return markers;
  })();

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

  function scrollToSection(index: number) {
    const el = document.getElementById(`section-marker-${index}`);
    if (el) {
      const topOffset = el.getBoundingClientRect().top + window.scrollY - 70;
      window.scrollTo({ top: topOffset, behavior: 'smooth' });
    }
  }

  function handleTouchStart(e: TouchEvent) {
    if (e.touches.length === 1) {
      touchStartX = e.touches[0].clientX;
      touchStartY = e.touches[0].clientY;
      isScrollDrag = false;
    }
  }

  function handleTouchMove(e: TouchEvent) {
    if (e.touches.length === 1) {
      const dx = Math.abs(e.touches[0].clientX - touchStartX);
      const dy = Math.abs(e.touches[0].clientY - touchStartY);
      if (dx > 8 || dy > 8) {
        isScrollDrag = true;
      }
    }
  }

  function handleCardClick(e: MouseEvent, assetId: string, secIdx: number, itemIdx: number) {
    // If the touch was part of a swipe or scroll, do not register a tap
    if (isScrollDrag) {
      isScrollDrag = false;
      return;
    }

    const card = (e.currentTarget as HTMLElement);
    const target = e.target as HTMLElement;

    // Check if the click target was specifically the select button or if selection mode is currently on
    if (target.closest('[data-select-btn]') || $isSelectionActive) {
      e.stopPropagation();
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

  function formatTime(seconds?: number | null): string {
    if (!seconds) return '';
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    return `${m}:${s.toString().padStart(2, '0')}`;
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
      if (coords) activeCoords = coords;
    };
    window.addEventListener('vault:open-asset', handleOpenAsset);

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
  class="px-2 pt-16 sm:px-4 md:px-8 max-w-[1920px] mx-auto min-h-screen flex flex-col select-none relative transition-transform duration-300 ease-out {activeCoords !== null ? 'scale-[0.985] opacity-80 pointer-events-none' : 'scale-100 opacity-100'}"
>
  <!-- Apple-Style Floating Bar -->
  <div class="flex items-center justify-between gap-3 px-1 py-1 min-h-[38px] mb-2 sticky top-14 z-30">
    <div class="flex items-center gap-1.5 text-xs text-[var(--text-muted)] overflow-x-auto no-scrollbar py-1">
      <button
        type="button"
        on:click={() => {
          filterStore.clearAlbum();
          filterStore.setFolderPath('');
        }}
        class="h-8 px-3 rounded-full flex items-center gap-1.5 font-medium transition-all spring-tap cursor-pointer {!currentAlbumPath ? 'bg-purple-500/15 text-purple-600 dark:text-purple-300 border border-purple-500/30 shadow-sm' : 'hover:bg-[var(--card-bg)] text-[var(--text-muted)]'}"
        title="All Photos"
        aria-label="Root view"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
          <circle cx="8.5" cy="8.5" r="1.5"></circle>
          <polyline points="21 15 16 10 5 21"></polyline>
        </svg>
        <span class="text-[11px] font-semibold tracking-tight">Library</span>
      </button>

      {#if breadcrumbSegments.length > 0}
        {#each breadcrumbSegments as seg, idx (seg.path)}
          <span class="opacity-30 text-[11px]">/</span>
          {@const isLast = idx === breadcrumbSegments.length - 1}

          <div class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full liquid-breadcrumb text-[var(--text-main)] text-xs">
            <button
              type="button"
              on:click={() => navigateToAlbumSegment(seg)}
              class="font-medium truncate max-w-[140px] sm:max-w-[220px] transition-colors cursor-pointer {isLast ? 'font-semibold text-purple-600 dark:text-purple-300' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            >
              {seg.name}
            </button>

            {#if isLast}
              <button
                type="button"
                on:click={() => (showManageAlbumModal = true)}
                class="text-[var(--text-muted)] hover:text-[var(--text-main)] p-0.5 cursor-pointer transition-colors"
                title="Options"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2">
                  <circle cx="12" cy="12" r="1"></circle>
                  <circle cx="19" cy="12" r="1"></circle>
                  <circle cx="5" cy="12" r="1"></circle>
                </svg>
              </button>
            {/if}
          </div>
        {/each}
      {/if}
    </div>

    <!-- Right Mode Indicator / Selection Action Pill -->
    <div class="flex items-center gap-2">
      {#if $isSelectionActive}
        <button
          type="button"
          on:click={() => selection.clearSelection()}
          class="px-3 py-1 rounded-full bg-[var(--card-bg)] border border-[var(--border-glass)] text-xs font-semibold text-[var(--text-main)] hover:bg-[var(--dock-bg-hover)] transition-all cursor-pointer shadow-sm"
        >
          Done
        </button>
      {/if}

      {#if $filterStore.show_trash}
        <div class="px-2.5 py-1 rounded-full bg-rose-500/15 border border-rose-500/30 text-rose-600 dark:text-rose-300 text-[10px] font-mono flex items-center gap-1.5 shadow-sm">
          <span class="w-1.5 h-1.5 rounded-full bg-rose-500 animate-pulse"></span>
          <span>TRASH</span>
        </div>
      {/if}
    </div>
  </div>

  <!-- ROOT ALBUMS SHELF -->
  {#if $filterStore.view_mode === 'albums' && !currentAlbumPath && rootAlbums.length > 0}
    <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-3 mb-6" in:fade={{ duration: 150 }}>
      {#each rootAlbums as album (album.id)}
        <button
          type="button"
          on:click={() => filterStore.setAlbumId(album.id)}
          class="album-card group text-left p-2 rounded-2xl bg-[var(--card-bg)] hover:bg-[var(--dock-bg-hover)] border border-[var(--border-glass)] hover:border-purple-500/40 transition-all cursor-pointer flex flex-col gap-2 spring-tap shadow-sm"
        >
          <div class="w-full aspect-square rounded-xl overflow-hidden bg-black/10 dark:bg-black/30 flex items-center justify-center relative">
            {#if album.cover_thumb}
              <img
                src={album.cover_thumb.startsWith('/') ? album.cover_thumb : `/${album.cover_thumb}`}
                alt={album.title}
                loading="lazy"
                class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-500"
              />
            {:else}
              <div class="w-8 h-8 text-[var(--text-muted)] opacity-30">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-full h-full" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                  <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                </svg>
              </div>
            {/if}
            <div class="absolute bottom-1.5 right-1.5 bg-black/60 backdrop-blur-md px-2 py-0.5 rounded-lg text-[10px] text-white font-mono">
              {album.media_count}
            </div>
          </div>

          <div class="px-1 truncate text-xs font-semibold text-[var(--text-main)] group-hover:text-purple-500 transition-colors">
            {album.rootName}
          </div>
        </button>
      {/each}
    </div>
  {/if}

  <!-- CHILD SUB-ALBUMS SHELF -->
  {#if currentAlbumPath && childAlbums.length > 0}
    <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-3 mb-6" in:fade={{ duration: 150 }}>
      {#each childAlbums as subAlbum (subAlbum.id || subAlbum.directSubName)}
        <button
          type="button"
          on:click={() => openChildAlbum(subAlbum)}
          class="album-card group text-left p-2 rounded-2xl bg-[var(--card-bg)] hover:bg-[var(--dock-bg-hover)] border border-[var(--border-glass)] hover:border-purple-500/40 transition-all cursor-pointer flex flex-col gap-2 spring-tap shadow-sm"
        >
          <div class="w-full aspect-square rounded-xl overflow-hidden bg-black/10 dark:bg-black/30 flex items-center justify-center relative">
            {#if subAlbum.cover_thumb}
              <img
                src={subAlbum.cover_thumb.startsWith('/') ? subAlbum.cover_thumb : `/${subAlbum.cover_thumb}`}
                alt={subAlbum.displayTitle}
                loading="lazy"
                class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-500"
              />
            {:else}
              <div class="w-8 h-8 text-[var(--text-muted)] opacity-30">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-full h-full" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                  <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                </svg>
              </div>
            {/if}
            <div class="absolute bottom-1.5 right-1.5 bg-black/60 backdrop-blur-md px-2 py-0.5 rounded-lg text-[10px] text-white font-mono">
              {subAlbum.media_count}
            </div>
          </div>

          <div class="px-1 truncate text-xs font-semibold text-[var(--text-main)] group-hover:text-purple-500 transition-colors">
            {subAlbum.displayTitle}
          </div>
        </button>
      {/each}
    </div>
  {/if}

  <!-- MAIN GALLERY TIMELINE -->
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
      <p class="text-xs text-[var(--text-muted)] mt-1">Try clearing filters or uploading new photos</p>
    </div>
  {:else}
    <!-- Timeline Container (Always smooth scrolling, no touch lockouts) -->
    <div
      role="region"
      aria-label="Media grid"
      class="space-y-6 md:space-y-8 select-none touch-pan-y"
      style="--grid-cols: {currentDensity.cols}; --grid-cols-mobile: {currentDensity.colsMobile};"
      on:touchstart={handleTouchStart}
      on:touchmove={handleTouchMove}
    >
      {#each $sections as section, secIdx (section.id || section.title)}
        <section id="section-marker-{secIdx}" class="section-container">
          <!-- Floating Apple-Style Date Header -->
          <div class="sticky top-12 z-20 py-2.5 px-2 flex items-center justify-between backdrop-blur-2xl bg-[var(--bg-primary)]/80 mb-2 border-b border-[var(--border-glass)]/50 rounded-xl transition-all">
            <div class="flex items-center gap-2">
              <span class="w-2 h-2 rounded-full bg-purple-500 shadow-[0_0_8px_rgba(168,85,247,0.7)]"></span>
              <h2 class="text-xs sm:text-sm font-bold tracking-tight text-[var(--text-main)]">
                {section.title}
              </h2>
            </div>
            <span class="text-[11px] font-mono text-[var(--text-muted)] bg-[var(--pill-bg)] px-2.5 py-0.5 rounded-full border border-[var(--border-glass)]">
              {section.items.length} {section.items.length === 1 ? 'item' : 'items'}
            </span>
          </div>

          <!-- Uniform Grid Cells -->
          <div class="gallery-grid">
            {#each section.items as asset, itemIdx (asset.id)}
              {@const isPriority = secIdx === 0 && itemIdx < 20}
              {@const isSelected = selectedAssetIds.includes(asset.id)}

              <div
                role="button"
                tabindex="0"
                aria-pressed={isSelected}
                aria-label={asset.file_name}
                data-asset-id={asset.id}
                data-sec-idx={secIdx}
                data-item-idx={itemIdx}
                on:click={(e) => handleCardClick(e, asset.id, secIdx, itemIdx)}
                on:keydown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    handleCardClick(e as any, asset.id, secIdx, itemIdx);
                  }
                }}
                class="tile-card group relative aspect-square rounded-lg overflow-hidden cursor-pointer focus:outline-none transition-transform duration-150 {isSelected ? 'scale-[0.93] ring-3 ring-purple-500 shadow-md' : 'hover:scale-[1.015]'}"
              >
                <!-- Thumbnail Image with Static Native Placeholder -->
                <img
                  src={asset.thumb_path.startsWith('/') ? asset.thumb_path : `/${asset.thumb_path}`}
                  alt={asset.file_name}
                  loading={isPriority ? 'eager' : 'lazy'}
                  decoding="async"
                  fetchpriority={isPriority ? 'high' : 'auto'}
                  on:load={(e) => (e.currentTarget as HTMLElement).classList.add('loaded')}
                  class="tile-image w-full h-full object-cover pointer-events-none"
                />

                <!-- Selection Circle Badge -->
                <button
                  type="button"
                  data-select-btn
                  class="select-btn absolute top-1.5 left-1.5 w-6 h-6 rounded-full flex items-center justify-center transition-all z-20 cursor-pointer shadow-md active:scale-90 {isSelected ? 'bg-purple-600 border border-white text-white opacity-100' : 'bg-black/35 backdrop-blur-md border border-white/40 text-white opacity-0 group-hover:opacity-100'}"
                  title="Select"
                  aria-label="Select photo"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 pointer-events-none {isSelected ? 'opacity-100' : 'opacity-0'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
                    <polyline points="20 6 9 17 4 12"></polyline>
                  </svg>
                </button>

                <!-- Favorite Badge -->
                {#if asset.is_favorite}
                  <div class="absolute top-1.5 right-1.5 bg-black/50 backdrop-blur-md px-1.5 py-1 rounded-full text-amber-300 z-10 pointer-events-none shadow-sm border border-white/10 flex items-center justify-center">
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5 fill-amber-300 stroke-amber-300" viewBox="0 0 24 24" stroke-width="2">
                      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon>
                    </svg>
                  </div>
                {/if}

                <!-- Video Duration HUD -->
                {#if asset.duration_seconds && $gridDensity > 0}
                  <div class="absolute bottom-1.5 right-1.5 bg-black/65 backdrop-blur-md border border-white/15 px-2 py-0.5 rounded-md text-[9px] text-white font-mono font-medium z-10 pointer-events-none flex items-center gap-1 shadow-sm">
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-2 h-2 fill-current" viewBox="0 0 24 24">
                      <polygon points="5 3 19 12 5 21 5 3"></polygon>
                    </svg>
                    <span>{formatTime(asset.duration_seconds)}</span>
                  </div>
                {/if}

                <!-- Days Remaining Badge in Trash View -->
                {#if asset.days_remaining !== null && asset.days_remaining !== undefined && $gridDensity > 0}
                  <div class="absolute bottom-1.5 left-1.5 bg-rose-600/70 backdrop-blur-md border border-rose-400/40 px-2 py-0.5 rounded-md text-[9px] text-white font-mono font-medium z-10 pointer-events-none shadow-sm">
                    <span>{asset.days_remaining}d left</span>
                  </div>
                {:else if asset.mime_type === 'image/gif' && $gridDensity > 0}
                  <div class="absolute bottom-1.5 left-1.5 bg-black/65 backdrop-blur-md border border-white/20 px-1.5 py-0.5 rounded text-[8px] text-white font-mono font-bold tracking-wider z-10 pointer-events-none">
                    GIF
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </section>
      {/each}
    </div>
  {/if}

  <!-- Prefetch Scroll Anchor -->
  <div bind:this={scrollTrigger} class="py-12 flex justify-center items-center min-h-[4rem]">
    {#if $isLoading}
      <div class="flex items-center gap-2 text-xs font-mono text-[var(--text-muted)] bg-[var(--card-bg)] px-4 py-2 rounded-full border border-[var(--border-glass)] shadow-sm">
        <div class="w-3.5 h-3.5 border-2 border-purple-500/30 border-t-purple-500 rounded-full animate-spin"></div>
        <span>Loading...</span>
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
    contain-intrinsic-size: auto 420px;
  }

  .gallery-grid {
    display: grid;
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
      grid-template-columns: repeat(var(--grid-cols, 7), minmax(0, 1fr));
    }
  }

  .tile-card {
    position: relative;
    width: 100%;
    contain: strict;
    -webkit-touch-callout: none;
    background-color: var(--card-bg, #1e1e24);
  }

  .tile-image {
    opacity: 0;
    transition: opacity 0.2s ease-out;
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