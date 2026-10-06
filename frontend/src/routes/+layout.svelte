<!-- photo-app/frontend/src/routes/+layout.svelte -->
<script lang="ts">
  import '../app.css';
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';

  import { authStore } from '$lib/stores/authStore';
  import { modalStore } from '$lib/stores/modalStore';
  import { albumStore } from '$lib/stores/albumStore';
  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import { filterOptionsStore } from '$lib/stores/filterOptionsStore';
  import { createWindowFileDrop } from '$lib/utils/dragDrop';
  import { initMediaEvents } from '$lib/utils/mediaEvents';

  import AuthScreen from '$lib/components/AuthScreen.svelte';
  import FilterSidebar from '$lib/components/FilterSidebar.svelte';
  import UploadModal from '$lib/components/UploadModal.svelte';
  import UploadProgressWidget from '$lib/components/UploadProgressWidget.svelte';
  import PeopleModal from '$lib/components/PeopleModal.svelte';
  import PlacesMapModal from '$lib/components/PlacesMapModal.svelte';

  let isSidebarOpen = false;
  let isDraggingOverWindow = false;
  let droppedFiles: File[] = [];

  let mainScrollContainer: HTMLElement;
  let lastScrollY = 0;
  let isNavHidden = false;
  const scrollThreshold = 10;

  // Touch Edge-Swipe State for Sidebar
  let touchStartX = 0;
  let touchStartY = 0;
  let isEligibleEdgeSwipe = false;

  let sseSubscription: { close: () => void } | null = null;

  function handleMainScroll() {
    if (!mainScrollContainer) return;
    const currentScrollY = mainScrollContainer.scrollTop;
    const diff = currentScrollY - lastScrollY;

    if (currentScrollY < 40) {
      isNavHidden = false;
    } else if (diff > scrollThreshold) {
      isNavHidden = true;
    } else if (diff < -scrollThreshold) {
      isNavHidden = false;
    }

    lastScrollY = currentScrollY;
  }

  function handleScrollToTop() {
    if (mainScrollContainer) {
      mainScrollContainer.scrollTo({ top: 0, behavior: 'smooth' });
    }
  }

  function handleToggleAlbums() {
    filterStore.toggleAlbums();
    handleScrollToTop();
  }

  function handleTouchStart(e: TouchEvent) {
    if (e.touches.length === 1) {
      touchStartX = e.touches[0].clientX;
      touchStartY = e.touches[0].clientY;
      // Triggers if swipe starts within 35px from left edge
      isEligibleEdgeSwipe = touchStartX < 35 && !isSidebarOpen;
    }
  }

  function handleTouchMove(e: TouchEvent) {
    if (e.touches.length === 1 && isEligibleEdgeSwipe) {
      const dx = e.touches[0].clientX - touchStartX;
      const dy = Math.abs(e.touches[0].clientY - touchStartY);

      if (dx > 45 && dy < 30) {
        isSidebarOpen = true;
        isEligibleEdgeSwipe = false;
      }
    }
  }

  const dragDropHandler = createWindowFileDrop((files: File[]) => {
    droppedFiles = files;
    modalStore.openUpload();
  });

  function handleDragEnter(e: DragEvent) {
    dragDropHandler.handleDragEnter(e, (v: boolean) => (isDraggingOverWindow = v));
  }

  function handleDragOver(e: DragEvent) {
    dragDropHandler.handleDragOver(e);
  }

  function handleDragLeave(e: DragEvent) {
    dragDropHandler.handleDragLeave(e, (v: boolean) => (isDraggingOverWindow = v));
  }

  function handleDrop(e: DragEvent) {
    dragDropHandler.handleDrop(e, (v: boolean) => (isDraggingOverWindow = v));
  }

  function handleMapSelectPhoto(e: CustomEvent<{ id: string }>) {
    modalStore.close();
    window.dispatchEvent(new CustomEvent('vault:open-asset', { detail: { id: e.detail.id } }));
  }

  function setupEventStream() {
    if (!browser || sseSubscription) return;

    sseSubscription = initMediaEvents({
      onAssetReady: (data: any) => {
        window.dispatchEvent(new CustomEvent('vault:refresh-timeline', { detail: data }));
        filterOptionsStore.scheduleRefresh($filterQueryString, 0);
      },
      onAlbumUpdated: () => {
        albumStore.load();
      },
      onPeopleUpdated: (data: any) => {
        window.dispatchEvent(new CustomEvent('vault:refresh-people', { detail: data }));
      },
      onAiCompleted: (_data: any) => {
        filterOptionsStore.scheduleRefresh($filterQueryString, 100);
      },
      onAssetFailed: (data: any) => {
        console.warn(`[Vault] Processing failed for asset ${data?.asset_id}: ${data?.error}`);
      }
    });
  }

  function teardownEventStream() {
    if (sseSubscription) {
      sseSubscription.close();
      sseSubscription = null;
    }
  }

  onMount(() => {
    authStore.checkStatus();
    albumStore.load();
    window.addEventListener('vault:scroll-top', handleScrollToTop);

    return () => {
      window.removeEventListener('vault:scroll-top', handleScrollToTop);
    };
  });

  $: if (browser) {
    if ($authStore.isAuthenticated) {
      setupEventStream();
    } else {
      teardownEventStream();
    }
  }

  $: if (browser && $authStore.isAuthenticated &&$filterQueryString !== undefined) {
    filterOptionsStore.scheduleRefresh($filterQueryString);
  }

  onDestroy(() => {
    teardownEventStream();
    filterOptionsStore.destroy();
  });
</script>

<svelte:window
  on:touchstart={handleTouchStart}
  on:touchmove={handleTouchMove}
  on:dragenter={handleDragEnter}
  on:dragover={handleDragOver}
  on:dragleave={handleDragLeave}
  on:drop={handleDrop}
/>

{#if $authStore.isLoading}
  <div class="h-screen w-screen flex flex-col items-center justify-center gap-3 bg-[var(--bg-primary)] text-[var(--text-main)]">
    <div class="w-7 h-7 border-2 border-[var(--border-subtle)] border-t-[var(--text-main)] rounded-full animate-spin"></div>
    <span class="text-xs font-medium tracking-tight text-[var(--text-muted)] font-mono">Opening Vault...</span>
  </div>
{:else if !$authStore.isAuthenticated}
  <AuthScreen />
{:else}
  <div class="h-screen w-screen flex overflow-hidden relative font-sans bg-[var(--bg-primary)] text-[var(--text-main)] isolate">
    <!-- Modular Filter Drawer -->
    <FilterSidebar
      isOpen={isSidebarOpen}
      on:close={() => (isSidebarOpen = false)}
      on:openPeople={() => modalStore.openPeople()}
    />

    <!-- Top Left Sidebar Trigger Button (Hides on Scroll Down) -->
    <div
      style="top: max(0.85rem, var(--sat)); left: max(1rem, var(--sal));"
      class="fixed z-40 transition-all duration-300 ease-out select-none {isNavHidden ? '-translate-y-16 opacity-0 pointer-events-none' : 'translate-y-0 opacity-100'}"
    >
      <button
        type="button"
        on:click={() => (isSidebarOpen = true)}
        class="liquid-btn w-9 h-9 rounded-full flex items-center justify-center text-[var(--text-muted)] hover:text-[var(--text-main)] spring-tap cursor-pointer shadow-lg"
        title="Open Filters"
        aria-label="Open Filters"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="4" y1="21" x2="4" y2="14"></line>
          <line x1="4" y1="10" x2="4" y2="3"></line>
          <line x1="12" y1="21" x2="12" y2="12"></line>
          <line x1="12" y1="8" x2="12" y2="3"></line>
          <line x1="20" y1="21" x2="20" y2="16"></line>
          <line x1="20" y1="12" x2="20" y2="3"></line>
          <line x1="1" y1="14" x2="7" y2="14"></line>
          <line x1="9" y1="8" x2="15" y2="8"></line>
          <line x1="17" y1="16" x2="23" y2="16"></line>
        </svg>
      </button>
    </div>

    <!-- Top Right Albums Toggle Button (Hides on Scroll Down) -->
    <div
      style="top: max(0.85rem, var(--sat)); right: max(1rem, var(--sar));"
      class="fixed z-40 transition-all duration-300 ease-out select-none {isNavHidden ? '-translate-y-16 opacity-0 pointer-events-none' : 'translate-y-0 opacity-100'}"
    >
      <button
        type="button"
        on:click={handleToggleAlbums}
        class="liquid-btn px-3.5 py-1.5 rounded-full text-xs font-semibold tracking-tight transition-all spring-tap cursor-pointer flex items-center gap-2 shadow-lg {$filterStore.view_mode === 'albums' ? 'bg-purple-600 text-white border-purple-400' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
        title={$filterStore.view_mode === 'albums' ? 'Back to Photos' : 'View Albums'}
        aria-label="Toggle Albums View"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
        </svg>
        <span>{$filterStore.view_mode === 'albums' ? 'Exit Albums' : 'Albums'}</span>
      </button>
    </div>

    <!-- Main Viewport -->
    <main
      bind:this={mainScrollContainer}
      on:scroll={handleMainScroll}
      class="flex-1 w-full h-full overflow-y-auto relative overscroll-none scroll-smooth bg-[var(--bg-primary)]"
    >
      <slot />

      <!-- Quick Upload Button (Floating at bottom-right, hides on scroll down) -->
      <button
        type="button"
        on:click={() => modalStore.openUpload()}
        style="bottom: max(1.25rem, calc(var(--sab) + 0.5rem)); right: max(1.25rem, var(--sar));"
        class="liquid-btn fixed z-30 w-11 h-11 rounded-full text-[var(--text-main)] flex items-center justify-center spring-tap cursor-pointer select-none transition-all duration-500 cubic-bezier(0.16, 1, 0.3, 1) backdrop-blur-2xl shadow-xl {isNavHidden ? 'translate-y-24 opacity-0 scale-90 pointer-events-none' : 'translate-y-0 opacity-100 scale-100'}"
        title="Upload Media"
        aria-label="Upload Media"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.2" d="M12 4v16m8-8H4" />
        </svg>
      </button>
    </main>

    <!-- Window Drag-and-Drop Overlay -->
    {#if isDraggingOverWindow}
      <div class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center pointer-events-none">
        <div class="liquid-dock px-8 py-4 rounded-3xl text-sm font-medium tracking-tight text-[var(--text-main)] animate-pulse">
          Drop photos or videos to upload
        </div>
      </div>
    {/if}

    <!-- Persistent Floating Upload Widget -->
    <UploadProgressWidget />

    <!-- Modals -->
    <UploadModal
      isOpen={$modalStore === 'upload'}
      initialFiles={droppedFiles}
      on:close={() => modalStore.close()}
      on:uploaded={() => {
        albumStore.load();
        window.dispatchEvent(new CustomEvent('vault:refresh-timeline'));
        filterOptionsStore.scheduleRefresh($filterQueryString, 0);
      }}
    />

    <PeopleModal
      isOpen={$modalStore === 'people'}
      on:close={() => modalStore.close()}
    />

    <PlacesMapModal
      isOpen={$modalStore === 'map'}
      on:close={() => modalStore.close()}
      on:selectPhoto={handleMapSelectPhoto}
    />
  </div>
{/if}

<style>
  .liquid-dock {
    background: var(--dock-bg);
    border: 1px solid var(--dock-border);
    box-shadow: 
      0 12px 32px var(--dock-shadow),
      inset 0 1px 0 var(--dock-highlight);
  }

  .liquid-btn {
    background: var(--dock-bg);
    border: 1px solid var(--dock-border);
    box-shadow: 
      0 8px 24px var(--dock-shadow),
      inset 0 1px 0 var(--dock-highlight);
  }

  .liquid-btn:hover {
    background: var(--dock-bg-hover);
    border-color: var(--dock-border-hover);
  }
</style>