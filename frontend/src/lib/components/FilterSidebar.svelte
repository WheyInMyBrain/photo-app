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

<!-- Mobile Dim Backdrop Overlay with High-Index Blur -->
{#if isOpen}
  <button
    type="button"
    class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm transition-opacity cursor-pointer border-0"
    on:click={() => dispatch('close')}
    aria-label="Close Filters Backdrop"
  ></button>
{/if}

<aside
  style="padding-top: max(1rem, var(--sat)); padding-bottom: max(1rem, var(--sab));"
  class="fixed inset-y-0 left-0 z-50 w-76 max-w-[85vw] h-full glass-pill border-y-0 border-l-0 flex flex-col justify-between p-4 select-none shadow-2xl transition-transform duration-300 ease-out {isOpen ? 'translate-x-0' : '-translate-x-full'}"
>
  <!-- Scrollable Filter Options Body -->
  <div class="space-y-4 overflow-y-auto pr-1 no-scrollbar">
    <!-- Header with Branding & Close Button -->
    <div class="space-y-1">
      <div class="flex items-center justify-between px-0.5">
        <div class="flex items-center gap-2">
          <a href="/" class="text-base font-bold tracking-tight text-[var(--text-main)] flex items-center gap-1.5 hover:opacity-80 transition-opacity">
            Vault
          </a>
          <span class="text-[10px] glass-panel text-[var(--text-muted)] px-2 py-0.5 rounded-full font-mono font-medium">
            {$authStore.user?.displayName || $authStore.user?.username}
          </span>
        </div>

        <button
          type="button"
          on:click={() => dispatch('close')}
          class="w-7 h-7 flex items-center justify-center rounded-full glass-panel text-[var(--text-muted)] hover:text-[var(--text-main)] transition-colors spring-tap cursor-pointer text-xs"
          title="Close Sidebar"
          aria-label="Close Sidebar"
        >
          ✕
        </button>
      </div>
    </div>

    <!-- Live Search Input -->
    <div class="relative">
      <input
        type="text"
        placeholder="Search library..."
        value={$filterStore.q}
        on:input={(e) => filterStore.setQ(e.currentTarget.value)}
        class="w-full bg-[var(--bg-surface-elevated)] border border-[var(--border-glass)] rounded-xl px-3 py-2 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-purple-500/50 transition-all"
      />
      {#if $filterStore.q}
        <button
          type="button"
          on:click={() => filterStore.setQ('')}
          class="absolute right-2.5 top-1/2 -translate-y-1/2 text-[10px] text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer"
        >
          ✕
        </button>
      {/if}
    </div>

    <!-- Media Type Segmented Control -->
    <div class="flex bg-[var(--bg-surface-elevated)] p-1 rounded-xl border border-[var(--border-glass)] text-xs">
      <button
        type="button"
        on:click={() => filterStore.setMediaType('all')}
        class="flex-1 py-1 rounded-lg text-center cursor-pointer transition-all spring-tap {$filterStore.media_type === 'all' ? 'bg-purple-600 text-white font-medium shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      >
        All ({filters.total_media})
      </button>
      <button
        type="button"
        on:click={() => filterStore.setMediaType('photos')}
        class="flex-1 py-1 rounded-lg text-center cursor-pointer transition-all spring-tap {$filterStore.media_type === 'photos' ? 'bg-purple-600 text-white font-medium shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      >
        Photos ({filters.photos_count})
      </button>
      <button
        type="button"
        on:click={() => filterStore.setMediaType('videos')}
        class="flex-1 py-1 rounded-lg text-center cursor-pointer transition-all spring-tap {$filterStore.media_type === 'videos' ? 'bg-purple-600 text-white font-medium shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      >
        Videos ({filters.videos_count})
      </button>
    </div>

    <!-- Favorites Only Switch -->
    <button
      type="button"
      on:click={() => filterStore.toggleFavorite()}
      class="w-full py-1.5 text-xs rounded-xl border transition-all spring-tap cursor-pointer {$filterStore.is_favorite ? 'bg-amber-400/15 border-amber-400/50 text-amber-300 font-medium' : 'glass-panel text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
    >
      ★ Favorites Only
    </button>

    <!-- Folder / Album Selector -->
    {#if filters.albums.length > 0}
      <div class="space-y-1.5 pt-2 border-t border-[var(--border-glass)]">
        <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider block pl-0.5">Album / Folder</span>
        <select
          value={$filterStore.folder_path}
          on:change={(e) => filterStore.setFolderPath(e.currentTarget.value)}
          class="w-full bg-[var(--bg-surface-elevated)] border border-[var(--border-glass)] text-xs text-[var(--text-main)] rounded-xl p-2 outline-none cursor-pointer focus:ring-2 focus:ring-purple-500/50"
        >
          <option value="">All Folders</option>
          {#each filters.albums as alb (alb.value)}
            <option value={alb.value}>{alb.label} ({alb.count})</option>
          {/each}
        </select>
      </div>
    {/if}

    <!-- Date Range Bounds -->
    <div class="space-y-1.5 pt-2 border-t border-[var(--border-glass)]">
      <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider block pl-0.5">Date Range</span>
      <div class="grid grid-cols-2 gap-2">
        <input
          type="date"
          value={$filterStore.from}
          min={filters.min_date ?? ''}
          max={filters.max_date ?? ''}
          on:change={(e) => filterStore.setFrom(e.currentTarget.value)}
          class="bg-[var(--bg-surface-elevated)] border border-[var(--border-glass)] text-[11px] text-[var(--text-main)] rounded-xl px-2 py-1.5 outline-none w-full"
        />
        <input
          type="date"
          value={$filterStore.to}
          min={filters.min_date ?? ''}
          max={filters.max_date ?? ''}
          on:change={(e) => filterStore.setTo(e.currentTarget.value)}
          class="bg-[var(--bg-surface-elevated)] border border-[var(--border-glass)] text-[11px] text-[var(--text-main)] rounded-xl px-2 py-1.5 outline-none w-full"
        />
      </div>
    </div>

    <!-- People Filter Chips -->
    {#if filters.people.length > 0}
      <div class="space-y-1.5 pt-2 border-t border-[var(--border-glass)]">
        <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider block pl-0.5">People</span>
        <div class="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto no-scrollbar">
          {#each filters.people as p (p.value)}
            {@const isPersonActive = $filterStore.person_ids.has(p.value)}
            <button
              type="button"
              on:click={() => filterStore.togglePerson(p.value)}
              class="text-[11px] px-2.5 py-1 rounded-full border transition-all spring-tap cursor-pointer {isPersonActive ? 'bg-purple-600 border-purple-500 text-white font-medium shadow-sm' : 'glass-panel text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            >
              {p.label} <span class="text-[9px] opacity-70">({p.count})</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Tags Filter Chips -->
    {#if filters.tags.length > 0}
      <div class="space-y-1.5 pt-2 border-t border-[var(--border-glass)]">
        <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider block pl-0.5">Tags</span>
        <div class="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto no-scrollbar">
          {#each filters.tags as t (t.value)}
            {@const isTagActive = $filterStore.tags.has(t.value)}
            <button
              type="button"
              on:click={() => filterStore.toggleTag(t.value)}
              class="text-[11px] px-2.5 py-1 rounded-full border transition-all spring-tap cursor-pointer {isTagActive ? 'bg-purple-600 border-purple-500 text-white font-medium shadow-sm' : 'glass-panel text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            >
              #{t.label} <span class="text-[9px] opacity-70">({t.count})</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Location Dropdown -->
    {#if filters.locations.length > 0}
      <div class="space-y-1.5 pt-2 border-t border-[var(--border-glass)]">
        <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider block pl-0.5">Location</span>
        <select
          value={$filterStore.city}
          on:change={(e) => filterStore.setCity(e.currentTarget.value)}
          class="w-full bg-[var(--bg-surface-elevated)] border border-[var(--border-glass)] text-xs text-[var(--text-main)] rounded-xl p-2 outline-none cursor-pointer focus:ring-2 focus:ring-purple-500/50"
        >
          <option value="">All Places</option>
          {#each filters.locations as loc (loc.value)}
            <option value={loc.value}>{loc.label} ({loc.count})</option>
          {/each}
        </select>
      </div>
    {/if}

    <!-- Camera Dropdown -->
    {#if filters.cameras.length > 0}
      <div class="space-y-1.5 pt-2 border-t border-[var(--border-glass)]">
        <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider block pl-0.5">Camera</span>
        <select
          value={$filterStore.camera_model}
          on:change={(e) => filterStore.setCamera(e.currentTarget.value)}
          class="w-full bg-[var(--bg-surface-elevated)] border border-[var(--border-glass)] text-xs text-[var(--text-main)] rounded-xl p-2 outline-none cursor-pointer focus:ring-2 focus:ring-purple-500/50"
        >
          <option value="">All Cameras</option>
          {#each filters.cameras as cam (cam.value)}
            <option value={cam.value}>{cam.label} ({cam.count})</option>
          {/each}
        </select>
      </div>
    {/if}
  </div>

  <!-- Drawer Footer Actions -->
  <div class="pt-3 border-t border-[var(--border-glass)] space-y-2">
    <!-- Manage People Modal Trigger -->
    <button
      type="button"
      on:click={() => {
        dispatch('close');
        dispatch('openPeople');
      }}
      class="w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between glass-panel text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
    >
      <span class="flex items-center gap-2">
        <span>👤</span>
        <span class="font-medium">Manage People</span>
      </span>
      <span class="text-[10px] text-[var(--text-muted)]">›</span>
    </button>

    <!-- Places Map Modal Trigger -->
    <button
      type="button"
      on:click={() => {
        dispatch('close');
        modalStore.openMap();
      }}
      class="w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between glass-panel text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
    >
      <span class="flex items-center gap-2">
        <span>🗺️</span>
        <span class="font-medium">Places Map</span>
      </span>
      <span class="text-[10px] text-[var(--text-muted)]">›</span>
    </button>

    <!-- Trash Mode Toggle -->
    <button
      type="button"
      on:click={() => filterStore.toggleTrash()}
      class="w-full py-2 px-3 text-xs rounded-xl flex items-center justify-between border transition-all spring-tap cursor-pointer {$filterStore.show_trash ? 'bg-red-500/15 border-red-500/50 text-red-300 font-medium' : 'glass-panel text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
    >
      <span class="flex items-center gap-2">
        <span>🗑️</span>
        <span>{$filterStore.show_trash ? 'Viewing Trash' : 'Trash'}</span>
      </span>
      {#if $filterStore.show_trash}
        <span class="text-[9px] bg-red-500/20 text-red-300 px-1.5 py-0.5 rounded font-mono">ACTIVE</span>
      {:else}
        <span class="text-[9px] opacity-60 font-mono">30d purge</span>
      {/if}
    </button>

    <div class="grid grid-cols-2 gap-2 pt-1">
      <button
        type="button"
        on:click={() => filterStore.reset()}
        class="py-1.5 text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] glass-panel rounded-xl transition-colors spring-tap cursor-pointer"
      >
        Reset
      </button>

      <button
        type="button"
        on:click={() => authStore.logout()}
        class="py-1.5 text-xs text-red-400/80 hover:text-red-400 glass-panel rounded-xl transition-colors spring-tap cursor-pointer"
      >
        Sign Out
      </button>
    </div>
  </div>
</aside>