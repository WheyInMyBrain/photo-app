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
</script>

<!-- Backdrop with pure dark blur -->
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
  class="liquid-sidebar fixed z-50 w-80 max-w-[88vw] rounded-3xl flex flex-col justify-between p-4.5 select-none transition-all duration-300 cubic-bezier(0.16, 1, 0.3, 1) {isOpen ? 'translate-x-0 opacity-100' : '-translate-x-[110%] opacity-0 pointer-events-none'}"
>
  <!-- Scrollable Filter Options Body -->
  <div class="space-y-4 overflow-y-auto pr-1 no-scrollbar">
    <!-- Header with Branding & Close Button -->
    <div class="flex items-center justify-between px-0.5 pb-1">
      <div class="flex items-center gap-2">
        <span class="text-sm font-semibold tracking-tight text-white">Vault</span>
        <span class="liquid-chip text-[10px] text-white/60 px-2 py-0.5 rounded-full font-mono">
          {$authStore.user?.displayName || $authStore.user?.username}
        </span>
      </div>

      <button
        type="button"
        on:click={() => dispatch('close')}
        class="liquid-icon-btn w-7 h-7 flex items-center justify-center rounded-full text-white/60 hover:text-white transition-all spring-tap cursor-pointer text-xs"
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
      <div class="absolute left-3 top-1/2 -translate-y-1/2 text-white/30 pointer-events-none">
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
        class="liquid-input w-full pl-8 pr-7 py-2 rounded-xl text-xs text-white placeholder-white/30 focus:outline-none transition-all"
      />
      {#if $filterStore.q}
        <button
          type="button"
          on:click={() => filterStore.setQ('')}
          class="absolute right-2.5 top-1/2 -translate-y-1/2 text-white/40 hover:text-white cursor-pointer text-[11px]"
        >
          ✕
        </button>
      {/if}
    </div>

    <!-- Media Type Segmented Control -->
    <div class="liquid-segmented p-0.5 rounded-xl flex text-xs">
      <button
        type="button"
        on:click={() => filterStore.setMediaType('all')}
        class="flex-1 py-1.5 rounded-lg text-center cursor-pointer transition-all spring-tap {$filterStore.media_type === 'all' ? 'liquid-seg-active text-white font-medium' : 'text-white/50 hover:text-white'}"
      >
        All <span class="text-[10px] opacity-60 font-mono">({filters.total_media})</span>
      </button>
      <button
        type="button"
        on:click={() => filterStore.setMediaType('photos')}
        class="flex-1 py-1.5 rounded-lg text-center cursor-pointer transition-all spring-tap {$filterStore.media_type === 'photos' ? 'liquid-seg-active text-white font-medium' : 'text-white/50 hover:text-white'}"
      >
        Photos <span class="text-[10px] opacity-60 font-mono">({filters.photos_count})</span>
      </button>
      <button
        type="button"
        on:click={() => filterStore.setMediaType('videos')}
        class="flex-1 py-1.5 rounded-lg text-center cursor-pointer transition-all spring-tap {$filterStore.media_type === 'videos' ? 'liquid-seg-active text-white font-medium' : 'text-white/50 hover:text-white'}"
      >
        Videos <span class="text-[10px] opacity-60 font-mono">({filters.videos_count})</span>
      </button>
    </div>

    <!-- Favorites Only Button -->
    <button
      type="button"
      on:click={() => filterStore.toggleFavorite()}
      class="w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between transition-all spring-tap cursor-pointer {$filterStore.is_favorite ? 'liquid-fav-active text-amber-300 font-medium' : 'liquid-card text-white/70 hover:text-white'}"
    >
      <span class="flex items-center gap-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 {$filterStore.is_favorite ? 'fill-amber-300 stroke-amber-300' : 'fill-none stroke-current'}" viewBox="0 0 24 24" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon>
        </svg>
        <span>Favorites Only</span>
      </span>
      {#if $filterStore.is_favorite}
        <span class="text-[9px] font-mono px-1.5 py-0.5 rounded bg-amber-400/20 text-amber-300">ON</span>
      {/if}
    </button>

    <!-- Album / Folder Selector -->
    {#if filters.albums.length > 0}
      <div class="space-y-1.5 pt-1">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40 block pl-1">Album / Directory</span>
        <select
          value={$filterStore.folder_path}
          on:change={(e) => filterStore.setFolderPath(e.currentTarget.value)}
          class="liquid-select w-full rounded-xl p-2 text-xs text-white outline-none cursor-pointer"
        >
          <option value="" class="bg-neutral-900 text-white">All Locations</option>
          {#each filters.albums as alb (alb.value)}
            <option value={alb.value} class="bg-neutral-900 text-white">{alb.label} ({alb.count})</option>
          {/each}
        </select>
      </div>
    {/if}

    <!-- Date Range Bounds -->
    <div class="space-y-1.5 pt-1">
      <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40 block pl-1">Date Range</span>
      <div class="grid grid-cols-2 gap-2">
        <input
          type="date"
          value={$filterStore.from}
          min={filters.min_date ?? ''}
          max={filters.max_date ?? ''}
          on:change={(e) => filterStore.setFrom(e.currentTarget.value)}
          class="liquid-input text-[11px] text-white rounded-xl px-2.5 py-1.5 outline-none w-full"
        />
        <input
          type="date"
          value={$filterStore.to}
          min={filters.min_date ?? ''}
          max={filters.max_date ?? ''}
          on:change={(e) => filterStore.setTo(e.currentTarget.value)}
          class="liquid-input text-[11px] text-white rounded-xl px-2.5 py-1.5 outline-none w-full"
        />
      </div>
    </div>

    <!-- People Filter Chips -->
    {#if filters.people.length > 0}
      <div class="space-y-1.5 pt-1">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40 block pl-1">People</span>
        <div class="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto no-scrollbar">
          {#each filters.people as p (p.value)}
            {@const isPersonActive = $filterStore.person_ids.has(p.value)}
            <button
              type="button"
              on:click={() => filterStore.togglePerson(p.value)}
              class="text-[11px] px-2.5 py-1 rounded-full transition-all spring-tap cursor-pointer {isPersonActive ? 'liquid-chip-active text-white font-medium' : 'liquid-chip text-white/60 hover:text-white'}"
            >
              {p.label} <span class="text-[9px] opacity-60 font-mono">({p.count})</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Tags Filter Chips -->
    {#if filters.tags.length > 0}
      <div class="space-y-1.5 pt-1">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40 block pl-1">Tags</span>
        <div class="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto no-scrollbar">
          {#each filters.tags as t (t.value)}
            {@const isTagActive = $filterStore.tags.has(t.value)}
            <button
              type="button"
              on:click={() => filterStore.toggleTag(t.value)}
              class="text-[11px] px-2.5 py-1 rounded-full transition-all spring-tap cursor-pointer {isTagActive ? 'liquid-chip-active text-white font-medium' : 'liquid-chip text-white/60 hover:text-white'}"
            >
              #{t.label} <span class="text-[9px] opacity-60 font-mono">({t.count})</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Location Dropdown -->
    {#if filters.locations.length > 0}
      <div class="space-y-1.5 pt-1">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40 block pl-1">Location</span>
        <select
          value={$filterStore.city}
          on:change={(e) => filterStore.setCity(e.currentTarget.value)}
          class="liquid-select w-full rounded-xl p-2 text-xs text-white outline-none cursor-pointer"
        >
          <option value="" class="bg-neutral-900 text-white">All Locations</option>
          {#each filters.locations as loc (loc.value)}
            <option value={loc.value} class="bg-neutral-900 text-white">{loc.label} ({loc.count})</option>
          {/each}
        </select>
      </div>
    {/if}

    <!-- Camera Dropdown -->
    {#if filters.cameras.length > 0}
      <div class="space-y-1.5 pt-1">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40 block pl-1">Camera</span>
        <select
          value={$filterStore.camera_model}
          on:change={(e) => filterStore.setCamera(e.currentTarget.value)}
          class="liquid-select w-full rounded-xl p-2 text-xs text-white outline-none cursor-pointer"
        >
          <option value="" class="bg-neutral-900 text-white">All Models</option>
          {#each filters.cameras as cam (cam.value)}
            <option value={cam.value} class="bg-neutral-900 text-white">{cam.label} ({cam.count})</option>
          {/each}
        </select>
      </div>
    {/if}
  </div>

  <!-- Drawer Footer Actions -->
  <div class="pt-3 space-y-2 border-t border-white/[0.08]">
    <!-- Manage People Modal Trigger -->
    <button
      type="button"
      on:click={() => {
        dispatch('close');
        dispatch('openPeople');
      }}
      class="liquid-card w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between text-white/70 hover:text-white transition-all spring-tap cursor-pointer"
    >
      <span class="flex items-center gap-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 opacity-70" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"></path>
          <circle cx="9" cy="7" r="4"></circle>
          <path d="M23 21v-2a4 4 0 0 0-3-3.87"></path>
          <path d="M16 3.13a4 4 0 0 1 0 7.75"></path>
        </svg>
        <span class="font-medium">Manage People</span>
      </span>
      <span class="text-white/40 text-xs">›</span>
    </button>

    <!-- Places Map Modal Trigger -->
    <button
      type="button"
      on:click={() => {
        dispatch('close');
        modalStore.openMap();
      }}
      class="liquid-card w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between text-white/70 hover:text-white transition-all spring-tap cursor-pointer"
    >
      <span class="flex items-center gap-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 opacity-70" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polygon points="1 6 1 22 8 18 16 22 23 18 23 2 16 6 8 2 1 6"></polygon>
          <line x1="8" y1="2" x2="8" y2="18"></line>
          <line x1="16" y1="6" x2="16" y2="22"></line>
        </svg>
        <span class="font-medium">Places Map</span>
      </span>
      <span class="text-white/40 text-xs">›</span>
    </button>

    <!-- Trash Mode Toggle -->
    <button
      type="button"
      on:click={() => filterStore.toggleTrash()}
      class="w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between transition-all spring-tap cursor-pointer {$filterStore.show_trash ? 'liquid-trash-active text-rose-300 font-medium' : 'liquid-card text-white/70 hover:text-white'}"
    >
      <span class="flex items-center gap-2">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 {$filterStore.show_trash ? 'stroke-rose-300' : 'opacity-70'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"></polyline>
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
        </svg>
        <span>{$filterStore.show_trash ? 'Viewing Trash' : 'Trash'}</span>
      </span>
      {#if $filterStore.show_trash}
        <span class="text-[9px] bg-rose-500/20 text-rose-300 px-1.5 py-0.5 rounded font-mono">ACTIVE</span>
      {:else}
        <span class="text-[9px] opacity-40 font-mono">30d purge</span>
      {/if}
    </button>

    <!-- Reset / Sign Out Actions -->
    <div class="grid grid-cols-2 gap-2 pt-1">
      <button
        type="button"
        on:click={() => filterStore.reset()}
        class="liquid-card py-1.5 text-xs text-white/60 hover:text-white rounded-xl transition-colors spring-tap cursor-pointer font-medium"
      >
        Reset
      </button>

      <button
        type="button"
        on:click={() => authStore.logout()}
        class="liquid-card py-1.5 text-xs text-rose-400/80 hover:text-rose-400 rounded-xl transition-colors spring-tap cursor-pointer font-medium"
      >
        Sign Out
      </button>
    </div>
  </div>
</aside>

<style>
  /* Apple Liquid-Glass Architecture */
  .liquid-sidebar {
    background: rgba(18, 18, 22, 0.72);
    border: 1px solid rgba(255, 255, 255, 0.12);
    backdrop-filter: blur(40px) saturate(180%);
    -webkit-backdrop-filter: blur(40px) saturate(180%);
    box-shadow:
      0 24px 60px rgba(0, 0, 0, 0.75),
      inset 0 1px 0 0 rgba(255, 255, 255, 0.22),
      inset 0 -1px 0 0 rgba(0, 0, 0, 0.4);
  }

  .liquid-card {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow:
      0 2px 8px rgba(0, 0, 0, 0.25),
      inset 0 1px 0 0 rgba(255, 255, 255, 0.12);
  }

  .liquid-card:hover {
    background: rgba(255, 255, 255, 0.07);
    border-color: rgba(255, 255, 255, 0.14);
  }

  .liquid-input,
  .liquid-select {
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.4);
  }

  .liquid-input:focus,
  .liquid-select:focus {
    border-color: rgba(255, 255, 255, 0.25);
    box-shadow: 
      inset 0 1px 2px rgba(0, 0, 0, 0.4),
      0 0 0 1px rgba(255, 255, 255, 0.15);
  }

  .liquid-icon-btn {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.12);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.2);
  }

  .liquid-icon-btn:hover {
    background: rgba(255, 255, 255, 0.12);
    border-color: rgba(255, 255, 255, 0.2);
  }

  .liquid-segmented {
    background: rgba(0, 0, 0, 0.4);
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.4);
  }

  .liquid-seg-active {
    background: rgba(255, 255, 255, 0.14);
    border: 1px solid rgba(255, 255, 255, 0.18);
    box-shadow:
      0 2px 6px rgba(0, 0, 0, 0.25),
      inset 0 1px 0 rgba(255, 255, 255, 0.3);
  }

  .liquid-chip {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.09);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.1);
  }

  .liquid-chip:hover {
    background: rgba(255, 255, 255, 0.09);
    border-color: rgba(255, 255, 255, 0.15);
  }

  .liquid-chip-active {
    background: rgba(255, 255, 255, 0.16);
    border: 1px solid rgba(255, 255, 255, 0.25);
    box-shadow:
      0 2px 8px rgba(0, 0, 0, 0.3),
      inset 0 1px 0 rgba(255, 255, 255, 0.35);
  }

  .liquid-fav-active {
    background: rgba(245, 158, 11, 0.12);
    border: 1px solid rgba(245, 158, 11, 0.28);
    box-shadow:
      0 2px 10px rgba(245, 158, 11, 0.15),
      inset 0 1px 0 rgba(255, 255, 255, 0.2);
  }

  .liquid-trash-active {
    background: rgba(244, 63, 94, 0.12);
    border: 1px solid rgba(244, 63, 94, 0.28);
    box-shadow:
      0 2px 10px rgba(244, 63, 94, 0.15),
      inset 0 1px 0 rgba(255, 255, 255, 0.2);
  }
</style>