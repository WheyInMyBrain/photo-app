<script lang="ts">
  import '../app.css';
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { authStore } from '$lib/stores/authStore';
  import { filterQueryString } from '$lib/stores/filterStore';
  import { filterOptionsStore } from '$lib/stores/filterOptionsStore';
  import { createWindowFileDrop } from '$lib/utils/dragDrop';

  import AuthScreen from '$lib/components/AuthScreen.svelte';
  import FilterSidebar from '$lib/components/FilterSidebar.svelte';
  import UploadModal from '$lib/components/UploadModal.svelte';
  import PeopleModal from '$lib/components/PeopleModal.svelte';

  let isSidebarOpen = false;
  let showUploadModal = false;
  let showPeopleModal = false;

  let isDraggingOverWindow = false;
  let droppedFiles: File[] = [];

  const dragDropHandler = createWindowFileDrop((files) => {
    droppedFiles = files;
    showUploadModal = true;
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
  <div class="h-screen w-screen bg-neutral-950 flex items-center justify-center text-xs text-neutral-500">
    Loading library...
  </div>
{:else if !$authStore.isAuthenticated}
  <AuthScreen />
{:else}
  <div class="h-screen w-screen flex bg-neutral-950 text-neutral-100 overflow-hidden font-sans relative">
    <!-- Floating Sidebar Toggle Button -->
    {#if !isSidebarOpen}
      <button
        type="button"
        on:click={() => (isSidebarOpen = true)}
        class="fixed top-3 left-3 z-30 p-2 rounded-lg bg-neutral-900/90 hover:bg-neutral-800 text-neutral-300 hover:text-white border border-neutral-800 shadow-lg backdrop-blur-sm transition-all cursor-pointer flex items-center justify-center gap-1.5 text-xs select-none"
        title="Open Filters"
        aria-label="Open Filters"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 6h16M4 12h16M4 18h16" />
        </svg>
        <span class="text-[11px] font-medium hidden sm:inline">Filters</span>
      </button>
    {/if}

    <!-- Isolated Filter Drawer -->
    <FilterSidebar
      isOpen={isSidebarOpen}
      on:close={() => (isSidebarOpen = false)}
      on:openPeople={() => (showPeopleModal = true)}
    />

    <!-- Main Viewport -->
    <main class="flex-1 w-full h-full overflow-y-auto bg-neutral-950 relative">
      <slot />

      <!-- Bottom-Right Floating '+' Upload Button -->
      <button
        type="button"
        on:click={() => (showUploadModal = true)}
        class="fixed bottom-6 right-6 z-40 w-12 h-12 rounded-full bg-white hover:bg-neutral-200 text-neutral-950 shadow-2xl flex items-center justify-center text-2xl font-light transition-transform hover:scale-105 active:scale-95 cursor-pointer select-none"
        title="Upload Media"
      >
        +
      </button>
    </main>

    <!-- Window Drag Overlay -->
    {#if isDraggingOverWindow}
      <div class="fixed inset-0 z-50 bg-black/70 border-2 border-dashed border-neutral-400 flex items-center justify-center pointer-events-none backdrop-blur-xs">
        <div class="bg-neutral-900 px-6 py-3 rounded-xl border border-neutral-800 text-sm font-medium text-white shadow-2xl">
          Drop files to upload
        </div>
      </div>
    {/if}

    <!-- Modals -->
    <UploadModal
      isOpen={showUploadModal}
      initialFiles={droppedFiles}
      on:close={() => (showUploadModal = false)}
      on:uploaded={() => {
        showUploadModal = false;
        window.dispatchEvent(new CustomEvent('vault:refresh-timeline'));
        filterOptionsStore.scheduleRefresh($filterQueryString, 0);
      }}
    />

    <PeopleModal
      isOpen={showPeopleModal}
      on:close={() => (showPeopleModal = false)}
    />
  </div>
{/if}