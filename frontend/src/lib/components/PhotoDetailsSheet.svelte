<script lang="ts">
  import { createEventDispatcher, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { modalStore } from '$lib/stores/modalStore';
  import type { FaceDetail, TagItem, SimilarItem, PersonCandidate } from '$lib/types/modal';

  export let asset: {
    id: string;
    file_name: string;
    captured_at: string | null;
    latitude?: number | null;
    longitude?: number | null;
  };
  export let isFavorite = false;
  export let faces: FaceDetail[] = [];
  export let tags: TagItem[] = [];
  export let similarItems: SimilarItem[] = [];
  export let knownPeople: PersonCandidate[] = [];
  export let loadingDetails = false;
  export let loadingSimilar = false;
  export let showBoxes = true;
  export let showMobileInfo = false;

  const dispatch = createEventDispatcher<{
    toggleFavorite: void;
    toggleBoxes: void;
    closeMobile: void;
    selectAsset: { id: string };
    saveFaceName: { face: FaceDetail; cleanName: string };
  }>();

  let editingFaceId: string | null = null;
  let editingName = '';

  let miniMapContainer: HTMLDivElement | null = null;
  let miniMap: any = null;

  function focusInput(node: HTMLElement) {
    node.focus();
  }

  function resolveUrl(path: string | undefined | null): string {
    if (!path) return '';
    if (path.startsWith('http')) return path;
    const clean = path.startsWith('/') ? path.slice(1) : path;
    return clean.startsWith('users/') ? `/${clean}` : `/thumbs/${clean}`;
  }

  function submitName(face: FaceDetail) {
    const clean = editingName.trim();
    editingFaceId = null;
    if (!clean || clean === face.person_name) return;
    dispatch('saveFaceName', { face, cleanName: clean });
  }

  async function initMiniMap(lat: number, lng: number) {
    if (!browser || !miniMapContainer) return;
    const L = (await import('leaflet')).default;

    if (miniMap) {
      miniMap.remove();
      miniMap = null;
    }

    miniMap = L.map(miniMapContainer, {
      zoomControl: false,
      attributionControl: false,
      dragging: false,
      scrollWheelZoom: false,
      doubleClickZoom: false,
      boxZoom: false,
      touchZoom: false,
      keyboard: false
    }).setView([lat, lng], 13);

    L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
      maxZoom: 18
    }).addTo(miniMap);

    const miniPinIcon = L.divIcon({
      html: `
        <div class="w-4 h-4 rounded-full bg-purple-600 border-2 border-white shadow-lg relative -translate-x-1/2 -translate-y-1/2">
          <div class="absolute inset-0 rounded-full bg-purple-400 animate-ping opacity-60"></div>
        </div>
      `,
      className: 'mini-map-pin',
      iconSize: [16, 16],
      iconAnchor: [8, 8]
    });

    L.marker([lat, lng], { icon: miniPinIcon }).addTo(miniMap);
  }

  $: if (browser && miniMapContainer && asset.latitude != null && asset.longitude != null) {
    const lat = asset.latitude;
    const lng = asset.longitude;
    setTimeout(() => initMiniMap(lat, lng), 60);
  }

  onDestroy(() => {
    if (miniMap) {
      miniMap.remove();
      miniMap = null;
    }
  });
</script>

<datalist id="known-people-list">
  {#each knownPeople as person (person.id)}
    {#if person.name}
      <option value={person.name}>{person.name}</option>
    {/if}
  {/each}
</datalist>

{#if showMobileInfo}
  <button
    type="button"
    class="fixed inset-0 z-40 bg-black/40 backdrop-blur-xs md:hidden cursor-pointer border-0"
    on:click={() => dispatch('closeMobile')}
    aria-label="Close details sheet"
  ></button>
{/if}

<aside
  class="
    fixed md:static inset-x-0 bottom-0 z-50 md:z-auto
    w-full md:w-88 h-[74vh] md:h-full
    border-t md:border-t-0 md:border-l border-[var(--border-glass)]
    glass-panel p-5 pb-[max(1.25rem,var(--sab))]
    flex flex-col justify-between overflow-y-auto space-y-6 flex-shrink-0
    rounded-t-3xl md:rounded-none shadow-2xl md:shadow-none
    transition-transform duration-300 ease-out no-scrollbar
    text-[var(--text-main)]
    {showMobileInfo ? 'translate-y-0' : 'translate-y-full md:translate-y-0'}
  "
>
  <div class="space-y-5">
    <div class="flex flex-col items-center -mt-2 mb-1 md:hidden">
      <div class="w-10 h-1 rounded-full bg-[var(--text-muted)]/30"></div>
    </div>

    <div class="flex items-center justify-between border-b border-[var(--border-glass)] pb-3">
      <div class="truncate mr-2">
        <h3 class="font-semibold text-xs tracking-tight text-[var(--text-main)] truncate" title={asset.file_name}>
          {asset.file_name}
        </h3>
        <p class="text-[10px] text-[var(--text-muted)] mt-0.5 font-mono">
          {asset.captured_at ? new Date(asset.captured_at).toLocaleDateString() : 'Undated'}
        </p>
      </div>
      <button
        type="button"
        on:click={() => dispatch('toggleFavorite')}
        class="text-sm p-2 rounded-full glass-panel hover:bg-[var(--border-glass)] transition-colors spring-tap cursor-pointer {isFavorite ? 'text-amber-400' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
        title="Toggle Favorite"
        aria-label="Toggle Favorite"
      >
        ★
      </button>
    </div>

    {#if asset.latitude != null && asset.longitude != null}
      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider">Location</span>
          <span class="text-[10px] font-mono text-[var(--text-muted)]">
            {asset.latitude.toFixed(4)}°, {asset.longitude.toFixed(4)}°
          </span>
        </div>

        <button
          type="button"
          on:click={() => modalStore.openMap()}
          class="relative w-full h-32 rounded-2xl overflow-hidden border border-[var(--border-glass)] glass-panel group cursor-pointer block text-left shadow-md spring-tap"
          title="Open in Places Map"
          aria-label="View photo location on Places Map"
        >
          <div bind:this={miniMapContainer} class="w-full h-full pointer-events-none"></div>

          <div class="absolute bottom-2 right-2 z-[400] px-2 py-1 rounded-lg glass-pill text-[10px] text-[var(--text-main)] flex items-center gap-1 shadow-md opacity-90 group-hover:opacity-100 transition-opacity">
            <span>🗺️</span>
            <span class="font-medium">Open in Places</span>
          </div>
        </button>
      </div>
    {/if}

    <div>
      <div class="flex items-center justify-between mb-2">
        <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider">People</span>
        {#if faces.length > 0}
          <button
            type="button"
            on:click={() => dispatch('toggleBoxes')}
            class="text-[10px] text-purple-500 hover:text-purple-400 font-medium cursor-pointer"
          >
            {showBoxes ? 'Hide Markers' : 'Show Markers'}
          </button>
        {/if}
      </div>

      {#if loadingDetails}
        <div class="text-xs text-[var(--text-muted)] font-mono">Scanning faces...</div>
      {:else if faces.length === 0}
        <div class="text-xs text-[var(--text-muted)] italic">No faces detected</div>
      {:else}
        <div class="space-y-2">
          {#each faces as f (f.face_id)}
            <div class="flex items-center gap-2.5 glass-panel p-2 rounded-xl">
              <img
                src={resolveUrl(f.face_thumb_path)}
                alt=""
                class="w-8 h-8 rounded-full object-cover border border-[var(--border-glass)] bg-[var(--bg-surface-elevated)]"
              />
              <div class="flex-1 min-w-0">
                {#if editingFaceId === f.face_id}
                  <input
                    type="text"
                    list="known-people-list"
                    bind:value={editingName}
                    placeholder="Search or name..."
                    on:blur={() => submitName(f)}
                    on:keydown={(e) => {
                      if (e.key === 'Enter') submitName(f);
                      if (e.key === 'Escape') editingFaceId = null;
                    }}
                    use:focusInput
                    class="w-full bg-[var(--bg-surface-elevated)] border border-purple-500 text-xs text-[var(--text-main)] rounded-lg px-2 py-1 outline-none"
                  />
                {:else}
                  <div class="flex items-center justify-between">
                    <span class="text-xs font-medium text-[var(--text-main)] truncate">{f.person_name || 'Unnamed'}</span>
                    <button
                      type="button"
                      on:click={() => {
                        editingFaceId = f.face_id;
                        editingName = f.person_name ?? '';
                      }}
                      class="text-[10px] text-[var(--text-muted)] hover:text-[var(--text-main)] ml-1 cursor-pointer p-1"
                      title="Rename"
                      aria-label="Rename face"
                    >
                      ✎
                    </button>
                  </div>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <div>
      <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider block mb-2">Tags</span>
      {#if loadingDetails}
        <div class="text-xs text-[var(--text-muted)] font-mono">Loading tags...</div>
      {:else if tags.length === 0}
        <div class="text-xs text-[var(--text-muted)] italic">No tags</div>
      {:else}
        <div class="flex flex-wrap gap-1.5">
          {#each tags as t (t.name)}
            <span class="px-2.5 py-1 rounded-full text-[10px] glass-panel font-medium text-[var(--text-main)]">
              #{t.name}
            </span>
          {/each}
        </div>
      {/if}
    </div>

    <div class="border-t border-[var(--border-glass)] pt-4">
      <div class="flex items-center justify-between mb-2.5">
        <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider">Similar Media</span>
        {#if similarItems.length > 0}
          <span class="text-[10px] text-[var(--text-muted)] font-mono">{similarItems.length} found</span>
        {/if}
      </div>

      {#if loadingSimilar}
        <div class="text-xs text-[var(--text-muted)] font-mono">Finding visually similar...</div>
      {:else if similarItems.length === 0}
        <div class="text-xs text-[var(--text-muted)] italic">No visually similar items</div>
      {:else}
        <div class="grid grid-cols-3 gap-2">
          {#each similarItems as s (s.id)}
            <button
              type="button"
              on:click={() => dispatch('selectAsset', { id: s.id })}
              class="group relative aspect-square rounded-xl overflow-hidden glass-panel hover:border-purple-500/80 transition-all cursor-pointer text-left shadow-md"
              title="Similarity: {Math.round(s.similarity * 100)}%"
              aria-label="Select similar media item"
            >
              <img
                src={resolveUrl(s.thumb_path)}
                alt=""
                loading="lazy"
                class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-200"
              />
              {#if s.mime_type.startsWith('video/') || s.mime_type === 'image/gif'}
                <div class="absolute top-1 left-1 bg-black/60 backdrop-blur-xs px-1.5 py-0.5 rounded-full text-[8px] text-white">
                  ▶
                </div>
              {/if}
              <div class="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/80 to-transparent p-1 opacity-0 group-hover:opacity-100 transition-opacity flex justify-end">
                <span class="text-[9px] font-mono text-purple-300 font-bold">
                  {Math.round(s.similarity * 100)}%
                </span>
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</aside>