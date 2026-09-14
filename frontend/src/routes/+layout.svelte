<script lang="ts">
  import '../app.css';
  import { browser } from '$app/environment';
  import { page } from '$app/stores';
  import { onMount, onDestroy } from 'svelte';
  import { filterStore, filterQueryString } from '$lib/stores/filterStore';
  import { authStore } from '$lib/stores/authStore';
  import UploadModal from '$lib/components/UploadModal.svelte';
  import AuthScreen from '$lib/components/AuthScreen.svelte';

  interface FilterOption {
    value: string;
    label: string;
    count: number;
  }

  interface FilterData {
    total_media: number;
    photos_count: number;
    videos_count: number;
    min_date: string | null;
    max_date: string | null;
    times_of_day: FilterOption[];
    people: FilterOption[];
    tags: FilterOption[];
    locations: FilterOption[];
    cameras: FilterOption[];
    albums: FilterOption[];
  }

  let filters: FilterData = {
    total_media: 0,
    photos_count: 0,
    videos_count: 0,
    min_date: null,
    max_date: null,
    times_of_day: [],
    people: [],
    tags: [],
    locations: [],
    cameras: [],
    albums: []
  };

  let showUploadModal = false;
  let droppedFiles: File[] = [];
  let isDraggingOverWindow = false;
  let dragCounter = 0;

  let filterAbortCtrl: AbortController | null = null;
  let filterDebounce: ReturnType<typeof setTimeout> | null = null;

  $: isOnPeoplePage = $page.url.pathname.startsWith('/people');

  onMount(() => {
    authStore.checkStatus();
  });

  async function refreshFilters(qs: string) {
    if (!browser || !$authStore.isAuthenticated) return;

    if (filterAbortCtrl) filterAbortCtrl.abort();
    filterAbortCtrl = new AbortController();

    try {
      const res = await fetch(`/api/media/filters${qs}`, {
        signal: filterAbortCtrl.signal
      });
      if (res.ok) {
        filters = await res.json();
      }
    } catch (e: any) {
      if (e?.name !== 'AbortError') {
        console.error('Failed to load dynamic filters', e);
      }
    }
  }

  $: if (browser && $authStore.isAuthenticated && $filterQueryString !== undefined) {
    if (filterDebounce) clearTimeout(filterDebounce);
    filterDebounce = setTimeout(() => {
      refreshFilters($filterQueryString);
    }, 150);
  }

  onDestroy(() => {
    if (filterDebounce) clearTimeout(filterDebounce);
    if (filterAbortCtrl) filterAbortCtrl.abort();
  });

  function isValidFileDrag(e: DragEvent): boolean {
    const types = e.dataTransfer?.types;
    return !!(types && types.includes('Files') && !types.includes('text/plain') && !types.includes('application/x-face-id'));
  }

  function handleDragEnter(e: DragEvent) {
    if (!isValidFileDrag(e)) return;
    e.preventDefault();
    dragCounter++;
    isDraggingOverWindow = true;
  }

  function handleDragOver(e: DragEvent) {
    if (!isValidFileDrag(e)) return;
    e.preventDefault();
  }

  function handleDragLeave(e: DragEvent) {
    if (!isValidFileDrag(e)) return;
    e.preventDefault();
    dragCounter--;
    if (dragCounter <= 0) {
      dragCounter = 0;
      isDraggingOverWindow = false;
    }
  }

  function handleDrop(e: DragEvent) {
    dragCounter = 0;
    isDraggingOverWindow = false;
    if (!isValidFileDrag(e)) return;

    e.preventDefault();
    if (e.dataTransfer?.files?.length) {
      droppedFiles = Array.from(e.dataTransfer.files);
      showUploadModal = true;
    }
  }
</script>

<svelte:window
  on:dragenter={handleDragEnter}
  on:dragover={handleDragOver}
  on:dragleave={handleDragLeave}
  on:drop={handleDrop}
/>

{#if $authStore.isLoading}
  <div class="h-screen w-screen bg-neutral-950 flex items-center justify-center text-xs text-neutral-500">
    Loading library...
  </div>
{:else if !$authStore.isAuthenticated}
  <AuthScreen />
{:else}
  <div class="h-screen w-screen flex bg-neutral-950 text-neutral-100 overflow-hidden font-sans">
    <!-- Dynamic Left Filter Sidebar -->
    <aside class="w-64 flex-shrink-0 h-full border-r border-neutral-800/80 bg-neutral-950 flex flex-col justify-between p-3.5 select-none">
      <div class="space-y-4 overflow-y-auto pr-1">
        <!-- Title & Profile Banner -->
        <div class="space-y-1">
          <div class="flex items-center justify-between px-1">
            <a href="/" class="text-base font-bold tracking-tight text-white flex items-center gap-1.5 hover:opacity-90 transition-opacity">
              Vault
            </a>
            <span class="text-[10px] bg-neutral-900 border border-neutral-800 text-neutral-300 px-2 py-0.5 rounded-full font-medium">
              {$authStore.user?.displayName || $authStore.user?.username}
            </span>
          </div>
        </div>

        <!-- Search -->
        <input
          type="text"
          placeholder="Search (⌘K)..."
          value={$filterStore.q}
          on:input={(e) => filterStore.setQ(e.currentTarget.value)}
          class="w-full bg-neutral-900 border border-neutral-800 rounded-lg px-2.5 py-1.5 text-xs text-white placeholder-neutral-500 focus:outline-none focus:border-neutral-700"
        />

        <!-- Photos / Videos Segmented Control -->
        <div class="flex bg-neutral-900 p-0.5 rounded-lg border border-neutral-800 text-xs">
          <button
            type="button"
            on:click={() => filterStore.setMediaType('all')}
            class="flex-1 py-1 rounded-md text-center cursor-pointer transition-all {$filterStore.media_type === 'all' ? 'bg-neutral-800 text-white font-medium shadow-sm' : 'text-neutral-400 hover:text-white'}"
          >
            All ({filters.total_media})
          </button>
          <button
            type="button"
            on:click={() => filterStore.setMediaType('photos')}
            class="flex-1 py-1 rounded-md text-center cursor-pointer transition-all {$filterStore.media_type === 'photos' ? 'bg-neutral-800 text-white font-medium shadow-sm' : 'text-neutral-400 hover:text-white'}"
          >
            Photos ({filters.photos_count ?? 0})
          </button>
          <button
            type="button"
            on:click={() => filterStore.setMediaType('videos')}
            class="flex-1 py-1 rounded-md text-center cursor-pointer transition-all {$filterStore.media_type === 'videos' ? 'bg-neutral-800 text-white font-medium shadow-sm' : 'text-neutral-400 hover:text-white'}"
          >
            Videos ({filters.videos_count ?? 0})
          </button>
        </div>

        <!-- Favorites Toggle -->
        <button
          type="button"
          on:click={() => filterStore.toggleFavorite()}
          class="w-full py-1 text-xs rounded border transition-all cursor-pointer {$filterStore.is_favorite ? 'bg-amber-400/10 border-amber-400/40 text-amber-300 font-medium' : 'bg-neutral-900 border-neutral-800 text-neutral-400 hover:text-white'}"
        >
          ★ Favorites Only
        </button>

        <!-- Folder / Album -->
        {#if filters.albums?.length > 0}
          <div class="space-y-1 pt-1 border-t border-neutral-900">
            <span class="text-[10px] uppercase font-semibold text-neutral-500 tracking-wider">Album / Folder</span>
            <select
              value={$filterStore.folder_path}
              on:change={(e) => filterStore.setFolderPath(e.currentTarget.value)}
              class="w-full bg-neutral-900 border border-neutral-800 text-xs text-neutral-300 rounded p-1.5 outline-none cursor-pointer"
            >
              <option value="">All Folders</option>
              {#each filters.albums as alb (alb.value)}
                <option value={alb.value}>{alb.label} ({alb.count})</option>
              {/each}
            </select>
          </div>
        {/if}

        <!-- Date Range -->
        <div class="space-y-1 pt-1 border-t border-neutral-900">
          <span class="text-[10px] uppercase font-semibold text-neutral-500 tracking-wider">Date Range</span>
          <div class="grid grid-cols-2 gap-1.5">
            <input
              type="date"
              value={$filterStore.from}
              min={filters.min_date ?? ''}
              max={filters.max_date ?? ''}
              on:change={(e) => filterStore.setFrom(e.currentTarget.value)}
              class="bg-neutral-900 border border-neutral-800 text-[10px] text-neutral-300 rounded px-1.5 py-1 outline-none w-full"
            />
            <input
              type="date"
              value={$filterStore.to}
              min={filters.min_date ?? ''}
              max={filters.max_date ?? ''}
              on:change={(e) => filterStore.setTo(e.currentTarget.value)}
              class="bg-neutral-900 border border-neutral-800 text-[10px] text-neutral-300 rounded px-1.5 py-1 outline-none w-full"
            />
          </div>
        </div>

        <!-- People Multi-Select -->
        {#if filters.people?.length > 0}
          <div class="space-y-1.5 pt-1 border-t border-neutral-900">
            <span class="text-[10px] uppercase font-semibold text-neutral-500 tracking-wider">People</span>
            <div class="flex flex-wrap gap-1 max-h-24 overflow-y-auto">
              {#each filters.people as p (p.value)}
                <button
                  type="button"
                  on:click={() => filterStore.togglePerson(p.value)}
                  class="text-[11px] px-2 py-0.5 rounded-md border transition-all cursor-pointer {$filterStore.person_ids.has(p.value) ? 'bg-purple-600 border-purple-500 text-white font-medium shadow-sm' : 'bg-neutral-900 border-neutral-800 text-neutral-400 hover:text-white'}"
                >
                  {p.label} <span class="text-[9px] opacity-60">({p.count})</span>
                </button>
              {/each}
            </div>
          </div>
        {/if}

        <!-- Tags Multi-Select -->
        {#if filters.tags?.length > 0}
          <div class="space-y-1.5 pt-1 border-t border-neutral-900">
            <span class="text-[10px] uppercase font-semibold text-neutral-500 tracking-wider">Tags</span>
            <div class="flex flex-wrap gap-1 max-h-28 overflow-y-auto">
              {#each filters.tags as t (t.value)}
                <button
                  type="button"
                  on:click={() => filterStore.toggleTag(t.value)}
                  class="text-[11px] px-1.5 py-0.5 rounded border transition-all cursor-pointer {$filterStore.tags.has(t.value) ? 'bg-purple-600 border-purple-500 text-white font-medium shadow-sm' : 'bg-neutral-900 border-neutral-800 text-neutral-400 hover:text-white'}"
                >
                  #{t.label} <span class="text-[9px] opacity-50">({t.count})</span>
                </button>
              {/each}
            </div>
          </div>
        {/if}

        <!-- Location -->
        {#if filters.locations?.length > 0}
          <div class="space-y-1 pt-1 border-t border-neutral-900">
            <span class="text-[10px] uppercase font-semibold text-neutral-500 tracking-wider">Location</span>
            <select
              value={$filterStore.city}
              on:change={(e) => filterStore.setCity(e.currentTarget.value)}
              class="w-full bg-neutral-900 border border-neutral-800 text-xs text-neutral-300 rounded p-1.5 outline-none cursor-pointer"
            >
              <option value="">All Places</option>
              {#each filters.locations as loc (loc.value)}
                <option value={loc.value}>{loc.label} ({loc.count})</option>
              {/each}
            </select>
          </div>
        {/if}

        <!-- Camera -->
        {#if filters.cameras?.length > 0}
          <div class="space-y-1 pt-1 border-t border-neutral-900">
            <span class="text-[10px] uppercase font-semibold text-neutral-500 tracking-wider">Camera</span>
            <select
              value={$filterStore.camera_model}
              on:change={(e) => filterStore.setCamera(e.currentTarget.value)}
              class="w-full bg-neutral-900 border border-neutral-800 text-xs text-neutral-300 rounded p-1.5 outline-none cursor-pointer"
            >
              <option value="">All Cameras</option>
              {#each filters.cameras as cam (cam.value)}
                <option value={cam.value}>{cam.label} ({cam.count})</option>
              {/each}
            </select>
          </div>
        {/if}
      </div>

      <!-- Bottom Actions -->
      <div class="pt-3 border-t border-neutral-900 space-y-2">
        <a
          href={isOnPeoplePage ? '/' : '/people'}
          class="w-full py-1.5 px-2.5 text-xs rounded-lg flex items-center justify-between border transition-all cursor-pointer {isOnPeoplePage ? 'bg-purple-950/60 border-purple-800 text-purple-200 font-medium' : 'bg-neutral-900/80 border-neutral-800 text-neutral-400 hover:text-white'}"
        >
          <span class="flex items-center gap-1.5">
            <span>{isOnPeoplePage ? '←' : '👤'}</span>
            <span>{isOnPeoplePage ? 'Exit Manage People' : 'Manage People'}</span>
          </span>
          {#if isOnPeoplePage}
            <span class="text-[9px] bg-purple-900/80 text-purple-200 px-1 py-0.5 rounded font-mono">BACK</span>
          {/if}
        </a>

        <button
          type="button"
          on:click={() => filterStore.toggleTrash()}
          class="w-full py-1.5 px-2.5 text-xs rounded-lg flex items-center justify-between border transition-all cursor-pointer {$filterStore.show_trash ? 'bg-red-950/60 border-red-800 text-red-200 font-medium' : 'bg-neutral-900/80 border-neutral-800 text-neutral-400 hover:text-white'}"
        >
          <span class="flex items-center gap-1.5">
            🗑️ <span>{$filterStore.show_trash ? 'Viewing Trash' : 'Trash (30d Auto-Purge)'}</span>
          </span>
          {#if $filterStore.show_trash}
            <span class="text-[9px] bg-red-900/80 text-red-200 px-1 py-0.5 rounded font-mono">ACTIVE</span>
          {/if}
        </button>

        <button
          type="button"
          on:click={() => filterStore.reset()}
          class="w-full py-1.5 text-xs text-neutral-400 hover:text-white border border-neutral-800 hover:border-neutral-700 bg-neutral-900 rounded-lg transition-colors cursor-pointer"
        >
          Reset Filters
        </button>

        <button
          type="button"
          on:click={() => authStore.logout()}
          class="w-full py-1.5 text-xs text-neutral-500 hover:text-red-400 border border-transparent hover:border-red-900/50 rounded-lg transition-colors cursor-pointer"
        >
          Sign Out
        </button>
      </div>
    </aside>

    <!-- Main View Area -->
    <main class="flex-1 h-full overflow-y-auto bg-neutral-950 relative">
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

    <!-- Drag Overlay -->
    {#if isDraggingOverWindow}
      <div class="fixed inset-0 z-50 bg-black/70 border-2 border-dashed border-neutral-400 flex items-center justify-center pointer-events-none backdrop-blur-xs">
        <div class="bg-neutral-900 px-6 py-3 rounded-xl border border-neutral-800 text-sm font-medium text-white shadow-2xl">
          Drop files to upload
        </div>
      </div>
    {/if}

    <UploadModal
      isOpen={showUploadModal}
      initialFiles={droppedFiles}
      on:close={() => (showUploadModal = false)}
      on:uploaded={() => {
        showUploadModal = false;
        window.dispatchEvent(new CustomEvent('vault:refresh-timeline'));
        refreshFilters($filterQueryString);
      }}
    />
  </div>
{/if}