<script lang="ts">
  import { onMount } from 'svelte';
  import PhotoModal from '$lib/components/PhotoModal.svelte';

  interface LocationCard {
    city: string;
    country: string | null;
    country_code: string | null;
    media_count: number;
    cover_thumb: string | null;
  }

  interface MediaItem {
    id: string;
    file_name: string;
    thumb_path: string;
    preview_path: string;
    aspect_ratio: number | null;
    duration_seconds: number | null;
    mime_type: string;
    captured_at: string | null;
  }

  interface SmartAlbumsOverviewResponse {
    locations: LocationCard[];
  }

  interface SmartAlbumItemsResponse {
    items: MediaItem[];
    total: number;
  }

  let locations: LocationCard[] = [];
  let isLoading = true;

  // Detail / Drilldown state
  let selectedLocation: LocationCard | null = null;
  let locationMedia: MediaItem[] = [];
  let isLoadingMedia = false;

  // Modal inspection & navigation state
  let selectedIndex: number | null = null;
  $: selectedAsset = selectedIndex !== null ? locationMedia[selectedIndex] : null;

  async function loadLocations() {
    isLoading = true;
    try {
      const res = await fetch('/api/smart-albums?is_private=false');
      if (res.ok) {
        const data: SmartAlbumsOverviewResponse = await res.json();
        locations = data.locations || [];
      }
    } catch (err) {
      console.error('Failed to load locations:', err);
    } finally {
      isLoading = false;
    }
  }

  async function openLocation(loc: LocationCard) {
    selectedLocation = loc;
    selectedIndex = null;
    isLoadingMedia = true;
    locationMedia = [];

    try {
      const params = new URLSearchParams({
        city: loc.city,
        limit: '150',
        offset: '0',
        is_private: 'false'
      });

      const res = await fetch(`/api/smart-albums/items?${params.toString()}`);
      if (res.ok) {
        const data: SmartAlbumItemsResponse = await res.json();
        locationMedia = data.items;
      }
    } catch (err) {
      console.error('Failed to load photos for location:', err);
    } finally {
      isLoadingMedia = false;
    }
  }

  function openAsset(index: number) {
    selectedIndex = index;
  }

  function handlePrev() {
    if (selectedIndex !== null && selectedIndex > 0) {
      selectedIndex -= 1;
    }
  }

  function handleNext() {
    if (selectedIndex !== null && selectedIndex < locationMedia.length - 1) {
      selectedIndex += 1;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && selectedLocation && selectedIndex === null) {
      selectedLocation = null;
    }
  }

  onMount(() => {
    loadLocations();
  });
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="p-6 max-w-7xl mx-auto min-h-full flex flex-col">
  <!-- Top Header -->
  <div class="flex items-center justify-between pb-6 mb-6 border-b border-neutral-900">
    <div>
      <h1 class="text-xl font-bold tracking-tight text-white">Places</h1>
      <p class="text-xs text-neutral-400">
        Photos and videos grouped by captured GPS city and country
      </p>
    </div>
    <div class="text-xs text-neutral-500 font-medium">
      {locations.length} {locations.length === 1 ? 'place' : 'places'} discovered
    </div>
  </div>

  {#if isLoading}
    <div class="flex items-center justify-center p-16 text-xs text-neutral-500">
      Loading locations...
    </div>
  {:else if locations.length === 0}
    <div class="flex-1 flex flex-col items-center justify-center text-center p-12 text-neutral-500">
      <div class="text-3xl mb-2">📍</div>
      <p class="text-sm">No geotagged media found</p>
      <p class="text-xs text-neutral-600 mt-1">Upload photos containing GPS EXIF coordinates to automatically populate places.</p>
    </div>
  {:else}
    <!-- Grid of Location Cards -->
    <div class="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
      {#each locations as loc}
        <button
          type="button"
          on:click={() => openLocation(loc)}
          class="group relative h-48 rounded-xl overflow-hidden border border-neutral-800 bg-neutral-900 text-left transition-all hover:border-neutral-700 focus:outline-none cursor-pointer"
        >
          <!-- Cover Thumbnail -->
          {#if loc.cover_thumb}
            <img
              src="/{loc.cover_thumb}"
              alt={loc.city}
              loading="lazy"
              class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300 brightness-75 group-hover:brightness-90"
            />
          {:else}
            <div class="w-full h-full bg-neutral-900 flex items-center justify-center text-neutral-700">
              <svg class="w-10 h-10" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0" />
                <circle cx="12" cy="10" r="3" />
              </svg>
            </div>
          {/if}

          <!-- Gradient Overlay -->
          <div class="absolute inset-0 bg-gradient-to-t from-black/90 via-black/30 to-transparent"></div>

          <!-- Card Content -->
          <div class="absolute bottom-0 inset-x-0 p-4">
            <h2 class="text-sm font-bold text-white truncate group-hover:text-blue-400 transition-colors">
              {loc.city}
            </h2>
            <div class="flex items-center justify-between text-[11px] text-neutral-400 mt-0.5">
              <span>{loc.country || loc.country_code || 'Unknown Country'}</span>
              <span class="font-mono text-[10px] text-neutral-500 bg-neutral-900/80 px-1.5 py-0.5 rounded border border-neutral-800">
                {loc.media_count} {loc.media_count === 1 ? 'item' : 'items'}
              </span>
            </div>
          </div>
        </button>
      {/each}
    </div>
  {/if}
</div>

<!-- Location Drilldown Modal / Photo Grid -->
{#if selectedLocation}
  <div class="fixed inset-0 z-40 flex bg-neutral-950/95 backdrop-blur-md overflow-y-auto p-6">
    <div class="max-w-7xl w-full mx-auto space-y-6">
      <!-- Modal Navigation Header -->
      <div class="flex items-center justify-between pb-4 border-b border-neutral-800">
        <div class="flex items-center gap-3">
          <button
            on:click={() => {
              selectedLocation = null;
              selectedIndex = null;
            }}
            class="p-1.5 bg-neutral-900 border border-neutral-800 hover:bg-neutral-800 rounded-lg text-neutral-400 hover:text-white transition-colors cursor-pointer"
            title="Back to Places"
          >
            <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
            </svg>
          </button>
          <div>
            <h2 class="text-lg font-bold text-white flex items-center gap-2">
              <span>{selectedLocation.city}</span>
              {#if selectedLocation.country_code}
                <span class="text-xs text-neutral-500 font-mono font-normal">({selectedLocation.country_code})</span>
              {/if}
            </h2>
            <p class="text-xs text-neutral-400">{selectedLocation.country || 'Geotagged Media'}</p>
          </div>
        </div>

        <div class="text-xs text-neutral-500 font-mono">
          {locationMedia.length} {locationMedia.length === 1 ? 'item' : 'items'}
        </div>
      </div>

      <!-- Photo Grid -->
      {#if isLoadingMedia}
        <div class="text-xs text-neutral-500 text-center py-24">Fetching photos from {selectedLocation.city}...</div>
      {:else if locationMedia.length === 0}
        <div class="text-xs text-neutral-500 text-center py-24">No media found for this location.</div>
      {:else}
        <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
          {#each locationMedia as asset, idx}
            <button
              type="button"
              on:click={() => openAsset(idx)}
              class="group relative aspect-square bg-neutral-900 rounded-lg overflow-hidden border border-neutral-800 hover:border-neutral-700 transition-all focus:outline-none cursor-pointer"
            >
              <img
                src="/{asset.thumb_path}"
                alt={asset.file_name}
                loading="lazy"
                class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-200"
              />

              {#if asset.duration_seconds}
                <div class="absolute bottom-1.5 right-1.5 bg-black/75 px-1.5 py-0.5 rounded text-[10px] text-white font-mono flex items-center gap-1">
                  <svg class="w-2.5 h-2.5 fill-current" viewBox="0 0 16 16">
                    <path d="m11.596 8.697-6.363 3.692c-.54.313-1.233-.066-1.233-.697V4.308c0-.63.692-1.01 1.233-.696l6.363 3.692a.802.802 0 0 1 0 1.393z"/>
                  </svg>
                  {Math.floor(asset.duration_seconds / 60)}:{Math.floor(asset.duration_seconds % 60).toString().padStart(2, '0')}
                </div>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

<!-- Asset Inspector Modal with Previous/Next Navigation -->
{#if selectedAsset && selectedIndex !== null}
  <PhotoModal
    asset={selectedAsset}
    hasPrev={selectedIndex > 0}
    hasNext={selectedIndex < locationMedia.length - 1}
    on:close={() => (selectedIndex = null)}
    on:prev={handlePrev}
    on:next={handleNext}
  />
{/if}