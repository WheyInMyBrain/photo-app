<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { filterStore } from '$lib/stores/filterStore';
  import { authStore } from '$lib/stores/authStore';
  import { filterOptionsStore } from '$lib/stores/filterOptionsStore';

  export let isOpen = false;

  const dispatch = createEventDispatcher<{
    close: void;
    openPeople: void;
  }>();

  $: filters = $filterOptionsStore;
</script>

<!-- Backdrop overlay on mobile -->
{#if isOpen}
  <button
    type="button"
    class="fixed inset-0 z-40 bg-black/60 backdrop-blur-xs transition-opacity cursor-pointer border-0"
    on:click={() => dispatch('close')}
    aria-label="Close Sidebar Backdrop"
  ></button>
{/if}

<aside
  class="fixed inset-y-0 left-0 z-50 w-72 max-w-[85vw] h-full border-r border-neutral-800/80 bg-neutral-950 flex flex-col justify-between p-3.5 select-none shadow-2xl transition-transform duration-200 ease-in-out {isOpen ? 'translate-x-0' : '-translate-x-full'}"
>
  <div class="space-y-4 overflow-y-auto pr-1">
    <!-- Header -->
    <div class="space-y-1">
      <div class="flex items-center justify-between px-1">
        <div class="flex items-center gap-2">
          <a href="/" class="text-base font-bold tracking-tight text-white flex items-center gap-1.5 hover:opacity-90 transition-opacity">
            Vault
          </a>
          <span class="text-[10px] bg-neutral-900 border border-neutral-800 text-neutral-300 px-2 py-0.5 rounded-full font-medium">
            {$authStore.user?.displayName || $authStore.user?.username}
          </span>
        </div>

        <button
          type="button"
          on:click={() => dispatch('close')}
          class="w-7 h-7 flex items-center justify-center rounded-lg bg-neutral-900 hover:bg-neutral-800 text-neutral-400 hover:text-white border border-neutral-800 transition-colors cursor-pointer text-xs"
          title="Close Sidebar"
          aria-label="Close Sidebar"
        >
          ✕
        </button>
      </div>
    </div>

    <!-- Search Input -->
    <input
      type="text"
      placeholder="Search..."
      value={$filterStore.q}
      on:input={(e) => filterStore.setQ(e.currentTarget.value)}
      class="w-full bg-neutral-900 border border-neutral-800 rounded-lg px-2.5 py-1.5 text-xs text-white placeholder-neutral-500 focus:outline-none focus:border-neutral-700"
    />

    <!-- Media Type Toggle -->
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
        Photos ({filters.photos_count})
      </button>
      <button
        type="button"
        on:click={() => filterStore.setMediaType('videos')}
        class="flex-1 py-1 rounded-md text-center cursor-pointer transition-all {$filterStore.media_type === 'videos' ? 'bg-neutral-800 text-white font-medium shadow-sm' : 'text-neutral-400 hover:text-white'}"
      >
        Videos ({filters.videos_count})
      </button>
    </div>

    <!-- Favorites Only -->
    <button
      type="button"
      on:click={() => filterStore.toggleFavorite()}
      class="w-full py-1 text-xs rounded border transition-all cursor-pointer {$filterStore.is_favorite ? 'bg-amber-400/10 border-amber-400/40 text-amber-300 font-medium' : 'bg-neutral-900 border-neutral-800 text-neutral-400 hover:text-white'}"
    >
      ★ Favorites Only
    </button>

    <!-- Folder/Album Dropdown -->
    {#if filters.albums.length > 0}
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

    <!-- People Filter Chips -->
    {#if filters.people.length > 0}
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

    <!-- Tags Filter Chips -->
    {#if filters.tags.length > 0}
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

    <!-- Location Dropdown -->
    {#if filters.locations.length > 0}
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

    <!-- Camera Dropdown -->
    {#if filters.cameras.length > 0}
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

  <!-- Footer Actions -->
  <div class="pt-3 border-t border-neutral-900 space-y-2">
    <!-- Manage People Modal Trigger -->
    <button
      type="button"
      on:click={() => {
        dispatch('close');
        dispatch('openPeople');
      }}
      class="w-full py-1.5 px-2.5 text-xs rounded-lg flex items-center justify-between border transition-all cursor-pointer bg-neutral-900/80 border-neutral-800 text-neutral-400 hover:text-white"
    >
      <span class="flex items-center gap-1.5">
        <span>👤</span>
        <span>Manage People</span>
      </span>
    </button>

    <!-- Trash Mode -->
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