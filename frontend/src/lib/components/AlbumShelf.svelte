<!-- photo-app/frontend/src/lib/components/AlbumShelf.svelte -->
<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { filterStore } from '$lib/stores/filterStore';
  import { albumNav, type BreadcrumbSegment, type ChildAlbumItem } from '$lib/stores/albumNavStore';

  export let isScrolled = false;

  const dispatch = createEventDispatcher<{
    manageAlbum: void;
  }>();

  $: ({ currentAlbumPath, breadcrumbSegments, rootAlbums, childAlbums } = $albumNav);

  function selectRoot(id: string) {
    filterStore.setAlbumId(id);
    window.dispatchEvent(new CustomEvent('vault:scroll-top'));
  }

  function navigateSegment(seg: BreadcrumbSegment) {
    window.dispatchEvent(new CustomEvent('vault:scroll-top'));
    if (seg.albumId) filterStore.setAlbumId(seg.albumId);
    else filterStore.setFolderPath(seg.path);
  }

  function openChild(sub: ChildAlbumItem) {
    window.dispatchEvent(new CustomEvent('vault:scroll-top'));
    if (sub.id) filterStore.setAlbumId(sub.id);
    else filterStore.setFolderPath(`${currentAlbumPath}/${sub.directSubName}`);
  }
</script>

<!-- Floating Centered Root Albums Track -->
{#if $filterStore.view_mode === 'albums' && !currentAlbumPath && rootAlbums.length > 0}
  <div class="sticky top-2 z-30 mb-4 flex justify-center pointer-events-none transition-all duration-300">
    <div
      class="pointer-events-auto flex items-center gap-2 overflow-x-auto no-scrollbar max-w-full px-2 py-1.5 rounded-full transition-all duration-300 {isScrolled ? 'album-floating-capsule' : 'album-floating-row'}"
    >
      {#each rootAlbums as album (album.id)}
        <button
          type="button"
          on:click={() => selectRoot(album.id)}
          class="group flex-shrink-0 flex items-center rounded-full transition-all duration-200 spring-tap cursor-pointer border border-[var(--border-glass)] shadow-md {isScrolled ? 'bg-[#18181f]/90 hover:bg-[#23232c] gap-1.5 pl-1 pr-2.5 py-1' : 'bg-[#16161c]/80 hover:bg-[#1e1e26] gap-2 pl-1.5 pr-3 py-1.5'}"
        >
          <div class="rounded-full overflow-hidden flex-shrink-0 bg-black/50 {isScrolled ? 'w-5 h-5' : 'w-7 h-7 sm:w-8 sm:h-8'}">
            {#if album.cover_thumb}
              <img
                src={album.cover_thumb.startsWith('/') ? album.cover_thumb : `/${album.cover_thumb}`}
                alt={album.rootName}
                loading="lazy"
                class="w-full h-full object-cover group-hover:scale-110 transition-transform duration-300"
              />
            {:else}
              <div class="w-full h-full flex items-center justify-center opacity-40 text-[var(--text-muted)]">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                </svg>
              </div>
            {/if}
          </div>

          <span class="font-medium text-[var(--text-main)] truncate max-w-[100px] sm:max-w-[130px] {isScrolled ? 'text-[11px]' : 'text-xs'}">
            {album.rootName}
          </span>

          {#if !isScrolled}
            <span class="text-[10px] font-mono text-[var(--text-muted)] opacity-60">
              {album.media_count}
            </span>
          {/if}
        </button>
      {/each}
    </div>
  </div>
{/if}

<!-- Sub-Album Breadcrumbs Strip -->
{#if currentAlbumPath || $filterStore.show_trash}
  <div class="sticky top-2 z-30 flex items-center justify-between gap-3 px-1 py-1 min-h-[38px] mb-3">
    <div class="flex items-center gap-1.5 text-xs text-[var(--text-muted)] overflow-x-auto no-scrollbar py-0.5">
      {#if currentAlbumPath}
        <button
          type="button"
          on:click={() => {
            filterStore.clearAlbum();
            window.dispatchEvent(new CustomEvent('vault:scroll-top'));
          }}
          class="w-7 h-7 rounded-full flex items-center justify-center transition-all spring-tap cursor-pointer bg-[var(--card-bg)] text-[var(--text-muted)] hover:text-[var(--text-main)] border border-[var(--border-glass)]"
          title="Back to all albums"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2">
            <polyline points="15 18 9 12 15 6"></polyline>
          </svg>
        </button>
      {/if}

      {#if breadcrumbSegments.length > 0}
        {#each breadcrumbSegments as seg, idx (seg.path)}
          {#if idx > 0}<span class="opacity-25 text-[10px]">/</span>{/if}
          {@const isLast = idx === breadcrumbSegments.length - 1}

          <div class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full liquid-breadcrumb text-[var(--text-main)] text-xs shadow-sm">
            <button
              type="button"
              on:click={() => navigateSegment(seg)}
              class="font-medium truncate max-w-[140px] sm:max-w-[220px] transition-colors cursor-pointer {isLast ? 'font-semibold text-purple-400' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            >
              {seg.name}
            </button>

            {#if isLast}
              <button
                type="button"
                on:click={() => dispatch('manageAlbum')}
                class="text-[var(--text-muted)] hover:text-[var(--text-main)] p-0.5 cursor-pointer"
                title="Album Options"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2">
                  <circle cx="12" cy="12" r="1"></circle>
                  <circle cx="19" cy="12" r="1"></circle>
                  <circle cx="5" cy="12" r="1"></circle>
                </svg>
              </button>
            {/if}
          </div>
        {/each}
      {/if}
    </div>

    {#if $filterStore.show_trash}
      <div class="px-2.5 py-1 rounded-full bg-rose-500/15 border border-rose-500/30 text-rose-500 text-[10px] font-mono flex items-center gap-1.5 shadow-sm">
        <span class="w-1.5 h-1.5 rounded-full bg-rose-500 animate-pulse"></span>
        <span>TRASH</span>
      </div>
    {/if}
  </div>
{/if}

<!-- Sub-Albums Pills within opened directory -->
{#if currentAlbumPath && childAlbums.length > 0}
  <div class="flex items-center gap-2.5 overflow-x-auto no-scrollbar mb-4 py-1">
    {#each childAlbums as sub (sub.id || sub.directSubName)}
      <button
        type="button"
        on:click={() => openChild(sub)}
        class="flex items-center gap-2 px-3 py-1.5 rounded-xl bg-[var(--card-bg)] border border-[var(--border-glass)] hover:border-purple-400/50 cursor-pointer spring-tap transition-all flex-shrink-0"
      >
        <div class="w-7 h-7 rounded-lg overflow-hidden bg-black/30 flex-shrink-0">
          {#if sub.cover_thumb}
            <img
              src={sub.cover_thumb.startsWith('/') ? sub.cover_thumb : `/${sub.cover_thumb}`}
              alt={sub.displayTitle}
              class="w-full h-full object-cover"
            />
          {/if}
        </div>
        <div class="flex flex-col text-left">
          <span class="text-xs font-semibold text-[var(--text-main)] truncate max-w-[120px]">{sub.displayTitle}</span>
          <span class="text-[10px] font-mono text-[var(--text-muted)]">{sub.media_count} items</span>
        </div>
      </button>
    {/each}
  </div>
{/if}

<style>
  .album-floating-row {
    background: transparent;
  }

  .album-floating-capsule {
    background: rgba(18, 18, 24, 0.78);
    border: 1px solid var(--border-glass, rgba(255, 255, 255, 0.1));
    backdrop-filter: blur(20px) saturate(180%);
    -webkit-backdrop-filter: blur(20px) saturate(180%);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  .liquid-breadcrumb {
    background: var(--dock-bg, rgba(20, 20, 25, 0.75));
    border: 1px solid var(--dock-border, rgba(255, 255, 255, 0.1));
    backdrop-filter: blur(20px) saturate(180%);
    box-shadow: 0 2px 8px var(--dock-shadow, rgba(0, 0, 0, 0.25));
  }
</style>