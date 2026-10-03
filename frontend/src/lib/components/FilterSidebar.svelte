<!-- photo-app/frontend/src/lib/components/FilterSidebar.svelte -->
<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { filterStore } from '$lib/stores/filterStore';
  import { authStore } from '$lib/stores/authStore';
  import { filterOptionsStore } from '$lib/stores/filterOptionsStore';
  import { modalStore } from '$lib/stores/modalStore';

  export let isOpen = false;

  const dispatch = createEventDispatcher<{
    close: void;
    openPeople: void;
  }>();

  $: filters = $filterOptionsStore;

  // --- Dual-Range Slider State & Helpers ---
  function parseToTimestamp(dateStr?: string | null): number | null {
    if (!dateStr) return null;
    const t = new Date(dateStr).getTime();
    return isNaN(t) ? null : t;
  }

  function formatTimestampToIso(ts: number): string {
    const d = new Date(ts);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }

  function formatDisplayDate(dateStr?: string | null): string {
    if (!dateStr) return '--';
    const d = new Date(dateStr);
    if (isNaN(d.getTime())) return dateStr;
    return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
  }

  $: libMinTs = parseToTimestamp(filters.min_date);
  $: libMaxTs = parseToTimestamp(filters.max_date);

  // If min and max are the same day, add 1 day so range isn't 0
  $: totalSpan = libMinTs && libMaxTs ? Math.max(86_400_000, libMaxTs - libMinTs) : 0;

  $: currentFromTs = parseToTimestamp($filterStore.from) ?? libMinTs ?? 0;
  $: currentToTs = parseToTimestamp($filterStore.to) ?? libMaxTs ?? 0;

  // Percentage positions (0% to 100%)
  $: minPercent =
    totalSpan > 0 && libMinTs !== null
      ? Math.max(0, Math.min(100, ((currentFromTs - libMinTs) / totalSpan) * 100))
      : 0;

  $: maxPercent =
    totalSpan > 0 && libMinTs !== null
      ? Math.max(0, Math.min(100, ((currentToTs - libMinTs) / totalSpan) * 100))
      : 100;

  function handleMinSliderInput(e: Event) {
    if (libMinTs === null || totalSpan === 0) return;
    const val = parseFloat((e.currentTarget as HTMLInputElement).value);
    const newFromTs = Math.min(libMinTs + (val / 100) * totalSpan, currentToTs - 86_400_000);
    filterStore.setFrom(formatTimestampToIso(newFromTs));
  }

  function handleMaxSliderInput(e: Event) {
    if (libMinTs === null || totalSpan === 0) return;
    const val = parseFloat((e.currentTarget as HTMLInputElement).value);
    const newToTs = Math.max(libMinTs + (val / 100) * totalSpan, currentFromTs + 86_400_000);
    filterStore.setTo(formatTimestampToIso(newToTs));
  }

  function resetDateRange() {
    filterStore.setFrom('');
    filterStore.setTo('');
  }
</script>

<!-- Backdrop with blur -->
{#if isOpen}
  <button
    type="button"
    class="fixed inset-0 z-40 bg-black/60 backdrop-blur-md transition-opacity cursor-pointer border-0"
    on:click={() => dispatch('close')}
    aria-label="Close Filters Backdrop"
  ></button>
{/if}

<aside
  style="top: max(0.75rem, var(--sat)); bottom: max(0.75rem, var(--sab)); left: max(0.75rem, var(--sal));"
  class="liquid-sidebar fixed z-50 w-80 max-w-[88vw] rounded-3xl flex flex-col justify-between p-4.5 select-none transition-all duration-300 cubic-bezier(0.16, 1, 0.3, 1) text-[var(--text-main)] {isOpen ? 'translate-x-0 opacity-100' : '-translate-x-[110%] opacity-0 pointer-events-none'}"
>
  <!-- Scrollable Filter Options Body -->
  <div class="space-y-4 overflow-y-auto pr-1 no-scrollbar">
    <!-- Header with Branding & Close Button -->
    <div class="flex items-center justify-between px-0.5 pb-1">
      <div class="flex items-center gap-2">
        <span class="text-sm font-semibold tracking-tight text-[var(--text-main)]">Vault</span>
        <span class="liquid-chip text-[10px] text-[var(--text-muted)] px-2 py-0.5 rounded-full font-mono">
          {$authStore.user?.displayName || $authStore.user?.username}
        </span>
      </div>

      <button
        type="button"
        on:click={() => dispatch('close')}
        class="liquid-icon-btn w-7 h-7 flex items-center justify-center rounded-full text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
        title="Close Sidebar"
        aria-label="Close Sidebar"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>
    </div>

    <!-- Live Search Input -->
    <div class="relative">
      <div class="absolute left-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)] opacity-60 pointer-events-none">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="11" cy="11" r="8"></circle>
          <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
        </svg>
      </div>
      <input
        type="text"
        placeholder="Search..."
        value={$filterStore.q}
        on:input={(e) => filterStore.setQ(e.currentTarget.value)}
        class="liquid-input w-full pl-8 pr-7 py-2 rounded-xl text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] focus:outline-none transition-all"
      />
      {#if $filterStore.q}
        <button
          type="button"
          on:click={() => filterStore.setQ('')}
          class="absolute right-2.5 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer p-0.5"
          aria-label="Clear search"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18"></line>
            <line x1="6" y1="6" x2="18" y2="18"></line>
          </svg>
        </button>
      {/if}
    </div>

    <!-- Media Type Segmented Control -->
    <div class="liquid-segmented p-0.5 rounded-xl flex text-xs">
      <button
        type="button"
        on:click={() => filterStore.setMediaType('all')}
        class="flex-1 py-1.5 rounded-lg text-center cursor-pointer transition-all spring-tap {$filterStore.media_type === 'all' ? 'liquid-seg-active text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      >
        All <span class="text-[10px] opacity-70 font-mono">({filters.total_media})</span>
      </button>
      <button
        type="button"
        on:click={() => filterStore.setMediaType('photos')}
        class="flex-1 py-1.5 rounded-lg text-center cursor-pointer transition-all spring-tap {$filterStore.media_type === 'photos' ? 'liquid-seg-active text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      >
        Photos <span class="text-[10px] opacity-70 font-mono">({filters.photos_count})</span>
      </button>
      <button
        type="button"
        on:click={() => filterStore.setMediaType('videos')}
        class="flex-1 py-1.5 rounded-lg text-center cursor-pointer transition-all spring-tap {$filterStore.media_type === 'videos' ? 'liquid-seg-active text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      >
        Videos <span class="text-[10px] opacity-70 font-mono">({filters.videos_count})</span>
      </button>
    </div>

    <!-- Favorites Only Button -->
    <button
      type="button"
      on:click={() => filterStore.toggleFavorite()}
      class="w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between transition-all spring-tap cursor-pointer {$filterStore.is_favorite ? 'liquid-fav-active text-amber-500 dark:text-amber-300 font-semibold' : 'liquid-card text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
    >
      <span class="flex items-center gap-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 {$filterStore.is_favorite ? 'fill-amber-500 dark:fill-amber-300 stroke-amber-500 dark:stroke-amber-300' : 'fill-none stroke-current'}" viewBox="0 0 24 24" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon>
        </svg>
        <span>Favorites Only</span>
      </span>
      {#if $filterStore.is_favorite}
        <span class="text-[9px] font-mono px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-600 dark:text-amber-300">ON</span>
      {/if}
    </button>

    <!-- Squeezable Period Dual Range Slider -->
    {#if filters.min_date && filters.max_date}
      <div class="space-y-2 pt-1">
        <div class="flex items-center justify-between pl-1 pr-0.5">
          <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)]">Time Period</span>
          {#if $filterStore.from ||$filterStore.to}
            <button
              type="button"
              on:click={resetDateRange}
              class="text-[9px] text-purple-500 hover:text-purple-400 font-medium transition-colors cursor-pointer"
            >
              Reset Span
            </button>
          {/if}
        </div>

        <!-- Range Badges: Displays the active squeezed window -->
        <div class="flex items-center justify-between text-[11px] font-mono text-[var(--text-main)] px-1">
          <span class="bg-[var(--pill-bg)] px-2 py-0.5 rounded-md border border-[var(--border-glass)]">
            {formatDisplayDate($filterStore.from || filters.min_date)}
          </span>
          <span class="text-[10px] text-[var(--text-muted)] font-sans">to</span>
          <span class="bg-[var(--pill-bg)] px-2 py-0.5 rounded-md border border-[var(--border-glass)]">
            {formatDisplayDate($filterStore.to || filters.max_date)}
          </span>
        </div>

        <!-- Squeezable Dual-Thumb Track Container -->
        <div class="relative h-6 flex items-center px-1">
          <!-- Background Inactive Track -->
          <div class="absolute left-1 right-1 h-1.5 rounded-full bg-[var(--pill-bg)] border border-[var(--border-glass)] pointer-events-none"></div>

          <!-- Highlighted Active Squeezed Track -->
          <div
            style="left: calc({minPercent}%); width: calc({Math.max(0, maxPercent - minPercent)}%);"
            class="absolute h-1.5 rounded-full bg-purple-500 shadow-[0_0_8px_rgba(168,85,247,0.5)] pointer-events-none"
          ></div>

          <!-- Left Thumb: Drag forward to contract start period -->
          <input
            type="range"
            min="0"
            max="100"
            step="0.5"
            value={minPercent}
            on:input={handleMinSliderInput}
            aria-label="Filter start date"
            class="dual-range-input z-20"
          />

          <!-- Right Thumb: Drag backward to contract end period -->
          <input
            type="range"
            min="0"
            max="100"
            step="0.5"
            value={maxPercent}
            on:input={handleMaxSliderInput}
            aria-label="Filter end date"
            class="dual-range-input z-30"
          />
        </div>

        <!-- Native Date Inputs for Precision -->
        <div class="grid grid-cols-2 gap-2 pt-0.5">
          <input
            type="date"
            value={$filterStore.from || filters.min_date}
            min={filters.min_date}
            max={$filterStore.to || filters.max_date}
            on:change={(e) => filterStore.setFrom(e.currentTarget.value)}
            class="liquid-input text-[11px] text-[var(--text-main)] rounded-xl px-2.5 py-1.5 outline-none w-full"
          />
          <input
            type="date"
            value={$filterStore.to || filters.max_date}
            min={$filterStore.from || filters.min_date}
            max={filters.max_date}
            on:change={(e) => filterStore.setTo(e.currentTarget.value)}
            class="liquid-input text-[11px] text-[var(--text-main)] rounded-xl px-2.5 py-1.5 outline-none w-full"
          />
        </div>
      </div>
    {/if}

    <!-- Album / Folder Selector -->
    {#if filters.albums.length > 0}
      <div class="space-y-1.5 pt-1">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-1">Album / Directory</span>
        <select
          value={$filterStore.folder_path}
          on:change={(e) => filterStore.setFolderPath(e.currentTarget.value)}
          class="liquid-select w-full rounded-xl p-2 text-xs text-[var(--text-main)] outline-none cursor-pointer"
        >
          <option value="" class="theme-option">All Locations</option>
          {#each filters.albums as alb (alb.value)}
            <option value={alb.value} class="theme-option">{alb.label} ({alb.count})</option>
          {/each}
        </select>
      </div>
    {/if}

    <!-- People Filter Chips -->
    {#if filters.people.length > 0}
      <div class="space-y-1.5 pt-1">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-1">People</span>
        <div class="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto no-scrollbar">
          {#each filters.people as p (p.value)}
            {@const isPersonActive = $filterStore.person_ids.has(p.value)}
            <button
              type="button"
              on:click={() => filterStore.togglePerson(p.value)}
              class="text-[11px] px-2.5 py-1 rounded-full transition-all spring-tap cursor-pointer {isPersonActive ? 'liquid-chip-active text-[var(--text-main)] font-semibold' : 'liquid-chip text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            >
              {p.label} <span class="text-[9px] opacity-70 font-mono">({p.count})</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Tags Filter Chips -->
    {#if filters.tags.length > 0}
      <div class="space-y-1.5 pt-1">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-1">Tags</span>
        <div class="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto no-scrollbar">
          {#each filters.tags as t (t.value)}
            {@const isTagActive = $filterStore.tags.has(t.value)}
            <button
              type="button"
              on:click={() => filterStore.toggleTag(t.value)}
              class="text-[11px] px-2.5 py-1 rounded-full transition-all spring-tap cursor-pointer {isTagActive ? 'liquid-chip-active text-[var(--text-main)] font-semibold' : 'liquid-chip text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            >
              #{t.label} <span class="text-[9px] opacity-70 font-mono">({t.count})</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Location Dropdown -->
    {#if filters.locations.length > 0}
      <div class="space-y-1.5 pt-1">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-1">Location</span>
        <select
          value={$filterStore.city}
          on:change={(e) => filterStore.setCity(e.currentTarget.value)}
          class="liquid-select w-full rounded-xl p-2 text-xs text-[var(--text-main)] outline-none cursor-pointer"
        >
          <option value="" class="theme-option">All Locations</option>
          {#each filters.locations as loc (loc.value)}
            <option value={loc.value} class="theme-option">{loc.label} ({loc.count})</option>
          {/each}
        </select>
      </div>
    {/if}

    <!-- Camera Dropdown -->
    {#if filters.cameras.length > 0}
      <div class="space-y-1.5 pt-1">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-1">Camera</span>
        <select
          value={$filterStore.camera_model}
          on:change={(e) => filterStore.setCamera(e.currentTarget.value)}
          class="liquid-select w-full rounded-xl p-2 text-xs text-[var(--text-main)] outline-none cursor-pointer"
        >
          <option value="" class="theme-option">All Models</option>
          {#each filters.cameras as cam (cam.value)}
            <option value={cam.value} class="theme-option">{cam.label} ({cam.count})</option>
          {/each}
        </select>
      </div>
    {/if}
  </div>

  <!-- Drawer Footer Actions -->
  <div class="pt-3 space-y-2 border-t border-[var(--border-glass)]">
    <!-- Manage People Modal Trigger -->
    <button
      type="button"
      on:click={() => {
        dispatch('close');
        dispatch('openPeople');
      }}
      class="liquid-card w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
    >
      <span class="flex items-center gap-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 opacity-80" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"></path>
          <circle cx="9" cy="7" r="4"></circle>
          <path d="M23 21v-2a4 4 0 0 0-3-3.87"></path>
          <path d="M16 3.13a4 4 0 0 1 0 7.75"></path>
        </svg>
        <span class="font-medium text-[var(--text-main)]">Manage People</span>
      </span>
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 opacity-50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="9 18 15 12 9 6"></polyline>
      </svg>
    </button>

    <!-- Places Map Modal Trigger -->
    <button
      type="button"
      on:click={() => {
        dispatch('close');
        modalStore.openMap();
      }}
      class="liquid-card w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
    >
      <span class="flex items-center gap-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 opacity-80" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polygon points="1 6 1 22 8 18 16 22 23 18 23 2 16 6 8 2 1 6"></polygon>
          <line x1="8" y1="2" x2="8" y2="18"></line>
          <line x1="16" y1="6" x2="16" y2="22"></line>
        </svg>
        <span class="font-medium text-[var(--text-main)]">Places Map</span>
      </span>
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 opacity-50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="9 18 15 12 9 6"></polyline>
      </svg>
    </button>

    <!-- Trash Mode Toggle -->
    <button
      type="button"
      on:click={() => filterStore.toggleTrash()}
      class="w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between transition-all spring-tap cursor-pointer {$filterStore.show_trash ? 'liquid-trash-active text-rose-600 dark:text-rose-300 font-semibold' : 'liquid-card text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
    >
      <span class="flex items-center gap-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 {$filterStore.show_trash ? 'stroke-rose-600 dark:stroke-rose-300' : 'opacity-80'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"></polyline>
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
        </svg>
        <span>{$filterStore.show_trash ? 'Viewing Trash' : 'Trash'}</span>
      </span>
      {#if $filterStore.show_trash}
        <span class="text-[9px] bg-rose-500/20 text-rose-600 dark:text-rose-300 px-1.5 py-0.5 rounded font-mono">ACTIVE</span>
      {:else}
        <span class="text-[9px] opacity-60 font-mono">30d purge</span>
      {/if}
    </button>

    <!-- Reset / Sign Out Actions -->
    <div class="grid grid-cols-2 gap-2 pt-1">
      <button
        type="button"
        on:click={() => filterStore.reset()}
        class="liquid-card py-1.5 text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] rounded-xl transition-colors spring-tap cursor-pointer font-medium"
      >
        Reset
      </button>

      <button
        type="button"
        on:click={() => authStore.logout()}
        class="liquid-card py-1.5 text-xs text-rose-500 dark:text-rose-400 hover:text-rose-600 dark:hover:text-rose-300 rounded-xl transition-colors spring-tap cursor-pointer font-medium"
      >
        Sign Out
      </button>
    </div>
  </div>
</aside>

<style>
  .liquid-sidebar {
    background: var(--bg-surface-elevated);
    border: 1px solid var(--border-glass);
    backdrop-filter: blur(40px) saturate(190%);
    -webkit-backdrop-filter: blur(40px) saturate(190%);
    box-shadow:
      0 24px 60px var(--dock-shadow),
      inset 0 1px 0 0 var(--border-specular);
  }

  .liquid-card {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    box-shadow: inset 0 1px 0 0 var(--border-specular);
  }

  .liquid-card:hover {
    background: var(--dock-bg-hover);
    border-color: var(--border-subtle);
  }

  .liquid-input,
  .liquid-select {
    background: var(--pill-bg);
    border: 1px solid var(--border-glass);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.1);
  }

  .liquid-input:focus,
  .liquid-select:focus {
    border-color: var(--border-subtle);
    box-shadow: 0 0 0 1px var(--border-subtle);
  }

  .theme-option {
    background: var(--bg-primary);
    color: var(--text-main);
  }

  .liquid-icon-btn {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    box-shadow: inset 0 1px 0 0 var(--border-specular);
  }

  .liquid-icon-btn:hover {
    background: var(--dock-bg-hover);
    border-color: var(--border-subtle);
  }

  .liquid-segmented {
    background: var(--pill-bg);
    border: 1px solid var(--border-glass);
  }

  .liquid-seg-active {
    background: var(--dock-bg);
    border: 1px solid var(--dock-border);
    box-shadow:
      0 2px 6px var(--dock-shadow),
      inset 0 1px 0 var(--dock-highlight);
  }

  .liquid-chip {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
  }

  .liquid-chip:hover {
    background: var(--dock-bg-hover);
    border-color: var(--border-subtle);
  }

  .liquid-chip-active {
    background: var(--pill-bg);
    border: 1px solid var(--border-subtle);
    box-shadow: 0 1px 4px var(--dock-shadow);
  }

  .liquid-fav-active {
    background: rgba(245, 158, 11, 0.12);
    border: 1px solid rgba(245, 158, 11, 0.28);
    box-shadow: 0 2px 10px rgba(245, 158, 11, 0.12);
  }

  .liquid-trash-active {
    background: rgba(244, 63, 94, 0.12);
    border: 1px solid rgba(244, 63, 94, 0.28);
    box-shadow: 0 2px 10px rgba(244, 63, 94, 0.12);
  }

  /* Dual Range Overlapping Sliders */
  .dual-range-input {
    position: absolute;
    left: 0;
    right: 0;
    width: 100%;
    margin: 0;
    pointer-events: none;
    -webkit-appearance: none;
    appearance: none;
    background: transparent;
  }

  .dual-range-input::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    pointer-events: auto;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #a855f7;
    border: 2px solid #ffffff;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.35);
    cursor: ew-resize;
    transition: transform 0.15s ease;
  }

  .dual-range-input::-webkit-slider-thumb:hover {
    transform: scale(1.18);
  }

  .dual-range-input::-moz-range-thumb {
    pointer-events: auto;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #a855f7;
    border: 2px solid #ffffff;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.35);
    cursor: ew-resize;
    transition: transform 0.15s ease;
  }

  .dual-range-input::-moz-range-thumb:hover {
    transform: scale(1.18);
  }
</style>