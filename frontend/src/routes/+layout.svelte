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

  import AuthScreen from '$lib/components/AuthScreen.svelte';
  import FilterSidebar from '$lib/components/FilterSidebar.svelte';
  import UploadModal from '$lib/components/UploadModal.svelte';
  import PeopleModal from '$lib/components/PeopleModal.svelte';
  import PlacesMapModal from '$lib/components/PlacesMapModal.svelte';

  let isSidebarOpen = false;
  let isDraggingOverWindow = false;
  let droppedFiles: File[] = [];

  let mainScrollContainer: HTMLElement;
  let lastScrollY = 0;
  let isNavHidden = false;
  const scrollThreshold = 8;

  function handleMainScroll() {
    if (!mainScrollContainer) return;
    const currentScrollY = mainScrollContainer.scrollTop;
    const diff = currentScrollY - lastScrollY;

    if (currentScrollY < 30) {
      isNavHidden = false;
    } else if (diff > scrollThreshold) {
      isNavHidden = true;
    } else if (diff < -scrollThreshold) {
      isNavHidden = false;
    }

    lastScrollY = currentScrollY;
  }

  const dragDropHandler = createWindowFileDrop((files) => {
    droppedFiles = files;
    modalStore.openUpload();
  });

  onMount(() => {
    authStore.checkStatus();
    albumStore.load();
  });

  $: if (browser && $authStore.isAuthenticated && $filterQueryString !== undefined) {
    filterOptionsStore.scheduleRefresh($filterQueryString);
  }

  onDestroy(() => {
    filterOptionsStore.destroy();
  });
</script>

<svelte:window
  on:dragenter={(e) => dragDropHandler.handleDragEnter(e, (v) => (isDraggingOverWindow = v))}
  on:dragover={dragDropHandler.handleDragOver}
  on:dragleave={(e) => dragDropHandler.handleDragLeave(e, (v) => (isDraggingOverWindow = v))}
  on:drop={(e) => dragDropHandler.handleDrop(e, (v) => (isDraggingOverWindow = v))}
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
    <!-- Menu Button (Top Left) -->
    {#if !isSidebarOpen}
      <button
        type="button"
        on:click={() => (isSidebarOpen = true)}
        style="top: max(1rem, var(--sat)); left: max(1rem, var(--sal));"
        class="liquid-btn fixed z-30 w-10 h-10 rounded-full text-[var(--text-main)] transition-all duration-300 spring-tap cursor-pointer flex items-center justify-center select-none {isNavHidden ? '-translate-y-16 opacity-0 pointer-events-none' : 'translate-y-0 opacity-100'}"
        title="Open menu"
        aria-label="Open menu"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.2" d="M4 6h16M4 12h16M4 18h16" />
        </svg>
      </button>
    {/if}

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

      <!-- Apple Liquid-Glass Navigation Dock -->
      <div
        style="bottom: max(1.5rem, calc(var(--sab) + 0.75rem));"
        class="fixed left-1/2 -translate-x-1/2 z-30 pointer-events-auto select-none transition-all duration-500 cubic-bezier(0.16, 1, 0.3, 1) {isNavHidden ? 'translate-y-24 opacity-0 scale-95 pointer-events-none' : 'translate-y-0 opacity-100 scale-100'}"
      >
        <nav aria-label="View switcher" class="liquid-dock p-1 rounded-full flex items-center gap-1 backdrop-blur-2xl">
          <!-- Photos Tab -->
          <button
            type="button"
            on:click={() => filterStore.setViewMode('timeline')}
            class="relative px-5 py-2 rounded-full text-xs font-medium tracking-tight transition-all duration-200 spring-tap cursor-pointer {$filterStore.view_mode === 'timeline' ? 'text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
          >
            {#if $filterStore.view_mode === 'timeline'}
              <div class="liquid-active-pill absolute inset-0 rounded-full -z-10"></div>
            {/if}
            Photos
          </button>

          <!-- Albums Tab -->
          <button
            type="button"
            on:click={() => filterStore.setViewMode('albums')}
            class="relative px-5 py-2 rounded-full text-xs font-medium tracking-tight transition-all duration-200 spring-tap cursor-pointer {$filterStore.view_mode === 'albums' ? 'text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
          >
            {#if $filterStore.view_mode === 'albums'}
              <div class="liquid-active-pill absolute inset-0 rounded-full -z-10"></div>
            {/if}
            Albums
          </button>
        </nav>
      </div>

      <!-- Liquid Glass Plus Button -->
      <button
        type="button"
        on:click={() => modalStore.openUpload()}
        style="bottom: max(1.5rem, calc(var(--sab) + 0.75rem)); right: max(1.5rem, var(--sar));"
        class="liquid-btn fixed z-30 w-11 h-11 rounded-full text-[var(--text-main)] flex items-center justify-center spring-tap cursor-pointer select-none transition-all duration-500 cubic-bezier(0.16, 1, 0.3, 1) backdrop-blur-2xl {isNavHidden ? 'translate-y-24 opacity-0 scale-90 pointer-events-none' : 'translate-y-0 opacity-100 scale-100'}"
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

    <!-- Modal Overlays via Declarative Store -->
    <UploadModal
      isOpen={$modalStore === 'upload'}
      initialFiles={droppedFiles}
      on:close={() => modalStore.close()}
      on:uploaded={() => {
        modalStore.close();
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
      on:selectPhoto={(e) => {
        modalStore.close();
        window.dispatchEvent(new CustomEvent('vault:open-asset', { detail: { id: e.detail.id } }));
      }}
    />
  </div>
{/if}

<style>
  /* All variables (--dock-bg, --dock-border, etc.) resolve directly from app.css */
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