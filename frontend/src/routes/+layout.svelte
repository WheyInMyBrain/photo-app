<!-- photo-app/frontend/src/routes/+layout.svelte -->
<script lang="ts">
  import '../app.css';
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';

  import { authStore } from '$lib/stores/authStore';
  import { modalStore } from '$lib/stores/modalStore';
  import { albumStore } from '$lib/stores/albumStore';
  import { filterStore, filterQueryString, type ViewMode } from '$lib/stores/filterStore';
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

  function handleSwitchMode(mode: ViewMode) {
    filterStore.setViewMode(mode);
    handleScrollToTop();
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

    <!-- Main Viewport -->
    <main
      bind:this={mainScrollContainer}
      on:scroll={handleMainScroll}
      class="flex-1 w-full h-full overflow-y-auto relative overscroll-none scroll-smooth bg-[var(--bg-primary)]"
    >
      <slot />

      <!-- Floating Bottom Navigation Dock -->
      <div
        style="bottom: max(1.25rem, calc(var(--sab) + 0.5rem));"
        class="fixed left-1/2 -translate-x-1/2 z-30 pointer-events-auto select-none transition-all duration-500 cubic-bezier(0.16, 1, 0.3, 1) {isNavHidden ? 'translate-y-24 opacity-0 scale-95 pointer-events-none' : 'translate-y-0 opacity-100 scale-100'}"
      >
        <nav aria-label="View switcher" class="liquid-dock p-1 rounded-full flex items-center gap-1 backdrop-blur-2xl shadow-xl">
          <!-- Shuffle / Random -->
          <button
            type="button"
            on:click={() => handleSwitchMode('random')}
            class="relative px-4 py-1.5 rounded-full text-xs font-medium tracking-tight transition-all duration-200 spring-tap cursor-pointer {$filterStore.view_mode === 'random' ? 'text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
          >
            {#if $filterStore.view_mode === 'random'}
              <div class="liquid-active-pill absolute inset-0 rounded-full -z-10"></div>
            {/if}
            Shuffle
          </button>

          <!-- Timeline -->
          <button
            type="button"
            on:click={() => handleSwitchMode('timeline')}
            class="relative px-4 py-1.5 rounded-full text-xs font-medium tracking-tight transition-all duration-200 spring-tap cursor-pointer {$filterStore.view_mode === 'timeline' ? 'text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
          >
            {#if $filterStore.view_mode === 'timeline'}
              <div class="liquid-active-pill absolute inset-0 rounded-full -z-10"></div>
            {/if}
            Timeline
          </button>

          <!-- Albums -->
          <button
            type="button"
            on:click={() => handleSwitchMode('albums')}
            class="relative px-4 py-1.5 rounded-full text-xs font-medium tracking-tight transition-all duration-200 spring-tap cursor-pointer {$filterStore.view_mode === 'albums' ? 'text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
          >
            {#if $filterStore.view_mode === 'albums'}
              <div class="liquid-active-pill absolute inset-0 rounded-full -z-10"></div>
            {/if}
            Albums
          </button>
        </nav>
      </div>

      <!-- Quick Upload Button -->
      <button
        type="button"
        on:click={() => modalStore.openUpload()}
        style="bottom: max(1.25rem, calc(var(--sab) + 0.5rem)); right: max(1.25rem, var(--sar));"
        class="liquid-btn fixed z-30 w-10 h-10 rounded-full text-[var(--text-main)] flex items-center justify-center spring-tap cursor-pointer select-none transition-all duration-500 cubic-bezier(0.16, 1, 0.3, 1) backdrop-blur-2xl shadow-xl {isNavHidden ? 'translate-y-24 opacity-0 scale-90 pointer-events-none' : 'translate-y-0 opacity-100 scale-100'}"
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

  .liquid-active-pill {
    background: var(--pill-bg);
    border: 1px solid var(--pill-border);
    box-shadow: 
      0 2px 8px var(--dock-shadow),
      inset 0 1px 0 var(--dock-highlight);
  }
</style>