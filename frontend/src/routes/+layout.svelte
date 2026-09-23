<script lang="ts">
  import '../app.css';
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { authStore } from '$lib/stores/authStore';
  import { modalStore } from '$lib/stores/modalStore';
  import { filterQueryString } from '$lib/stores/filterStore';
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

  const dragDropHandler = createWindowFileDrop((files) => {
    droppedFiles = files;
    modalStore.openUpload();
  });

  onMount(() => {
    authStore.checkStatus();
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
  <!-- Smooth Loading State with System Accent Spinner -->
  <div class="h-screen w-screen flex flex-col items-center justify-center gap-3 bg-[var(--bg-primary)] text-[var(--text-main)]">
    <div class="w-7 h-7 border-2 border-purple-500/20 border-t-purple-500 rounded-full animate-spin"></div>
    <span class="text-xs font-medium tracking-tight text-[var(--text-muted)] font-mono">Opening Vault...</span>
  </div>
{:else if !$authStore.isAuthenticated}
  <AuthScreen />
{:else}
  <div class="h-screen w-screen flex overflow-hidden relative font-sans bg-[var(--bg-primary)] text-[var(--text-main)]">
    <!-- Floating Glass Sidebar Toggle Button -->
    {#if !isSidebarOpen}
      <button
        type="button"
        on:click={() => (isSidebarOpen = true)}
        style="top: max(0.85rem, var(--sat)); left: max(0.85rem, var(--sal));"
        class="fixed z-30 w-10 h-10 rounded-full glass-pill text-[var(--text-main)] transition-all spring-tap cursor-pointer flex items-center justify-center select-none shadow-lg"
        title="Open menu"
        aria-label="Open menu"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 opacity-80" fill="none" viewBox="0 0 24 24" stroke="currentColor">
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

    <!-- Main Dynamic Viewport -->
    <main class="flex-1 w-full h-full overflow-y-auto relative overscroll-none">
      <slot />

      <!-- Floating '+' Upload Button -->
      <button
        type="button"
        on:click={() => modalStore.openUpload()}
        style="bottom: max(1.5rem, var(--sab)); right: max(1.5rem, var(--sar));"
        class="fixed z-30 w-13 h-13 rounded-full bg-purple-600 hover:bg-purple-500 text-white shadow-2xl flex items-center justify-center text-2xl font-light spring-tap cursor-pointer select-none border border-white/20"
        title="Upload Media"
        aria-label="Upload Media"
      >
        <span class="-mt-0.5 pointer-events-none">+</span>
      </button>
    </main>

    <!-- Window Drag-and-Drop Overlay -->
    {#if isDraggingOverWindow}
      <div class="fixed inset-0 z-50 bg-black/60 backdrop-blur-md border-2 border-dashed border-purple-400/80 flex items-center justify-center pointer-events-none">
        <div class="glass-pill px-8 py-4 rounded-2xl text-sm font-semibold tracking-wide text-white shadow-2xl animate-pulse">
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