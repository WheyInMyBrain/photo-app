<!-- photo-app/frontend/src/lib/components/PhotoDetailsSheet.svelte -->
<script lang="ts">
  import { createEventDispatcher, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { fade } from 'svelte/transition';
  import { modalStore } from '$lib/stores/modalStore';
  import type { FaceDetail, TagItem, PersonCandidate } from '$lib/types/modal';
  import type { AssetPoseDetail, SimilarMediaItem } from '$lib/api/assets';

  export let asset: {
    id: string;
    file_name: string;
    captured_at: string | null;
    latitude?: number | null;
    longitude?: number | null;
  };
  export let isFavorite = false;
  export let faces: FaceDetail[] = [];
  export let poses: AssetPoseDetail[] = [];
  export let tags: TagItem[] = [];
  export let similarItems: SimilarMediaItem[] = [];
  export let knownPeople: PersonCandidate[] = [];
  export let loadingDetails = false;
  export let loadingSimilar = false;

  export let showFaces = false;
  export let showPoses = false;
  export let showMobileInfo = false;

  const dispatch = createEventDispatcher<{
    toggleFavorite: void;
    toggleFaces: void;
    togglePoses: void;
    closeMobile: void;
    selectAsset: { id: string };
    saveFaceName: { face: FaceDetail; cleanName: string };
    reassignFace: { faceId: string; targetPersonId: string };
    splitFace: { faceId: string };
    unlinkFace: { faceId: string };
    deleteFace: { faceId: string };
  }>();

  let editingFaceId: string | null = null;
  let reassigningFaceId: string | null = null;
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
        <div class="w-3.5 h-3.5 rounded-full bg-purple-600 border-2 border-white shadow-md relative -translate-x-1/2 -translate-y-1/2">
          <div class="absolute inset-0 rounded-full bg-purple-400 animate-ping opacity-75"></div>
        </div>
      `,
      className: 'mini-map-pin',
      iconSize: [14, 14],
      iconAnchor: [7, 7]
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
    class="fixed inset-0 z-40 bg-black/60 backdrop-blur-md md:hidden cursor-pointer border-0"
    on:click={() => dispatch('closeMobile')}
    aria-label="Close details sheet"
  ></button>
{/if}

<aside
  class="
    fixed md:static inset-x-0 bottom-0 z-50 md:z-auto
    w-full md:w-88 h-[75vh] md:h-full
    liquid-sidebar
    p-5 pb-[max(1.5rem,var(--sab))]
    flex flex-col justify-between overflow-y-auto space-y-5 flex-shrink-0
    rounded-t-3xl md:rounded-none
    transition-transform duration-300 ease-out no-scrollbar
    text-[var(--text-main)]
    {showMobileInfo ? 'translate-y-0' : 'translate-y-full md:translate-y-0'}
  "
>
  <div class="space-y-5">
    <!-- Mobile Grab Handle -->
    <div class="flex flex-col items-center -mt-1.5 mb-1 md:hidden">
      <div class="w-9 h-1 rounded-full bg-[var(--text-muted)] opacity-30"></div>
    </div>

    <!-- Title & Favorite Header -->
    <div class="flex items-center justify-between border-b border-[var(--border-glass)] pb-3.5">
      <div class="truncate mr-3">
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
        class="liquid-icon-btn w-8 h-8 rounded-full flex items-center justify-center transition-all spring-tap cursor-pointer {isFavorite ? 'text-amber-500 dark:text-amber-300 liquid-fav-active' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
        title="Toggle Favorite"
        aria-label="Toggle Favorite"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 {isFavorite ? 'fill-amber-500 dark:fill-amber-300 stroke-amber-500 dark:stroke-amber-300' : 'fill-none stroke-current'}" viewBox="0 0 24 24" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon>
        </svg>
      </button>
    </div>

    <!-- Geolocation & Mini Map -->
    {#if asset.latitude != null && asset.longitude != null}
      <div class="space-y-1.5">
        <div class="flex items-center justify-between px-0.5">
          <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)]">Location</span>
          <span class="text-[10px] font-mono text-[var(--text-muted)]">
            {asset.latitude.toFixed(4)}°, {asset.longitude.toFixed(4)}°
          </span>
        </div>

        <button
          type="button"
          on:click={() => modalStore.openMap()}
          class="liquid-map-frame relative w-full h-32 rounded-2xl overflow-hidden group cursor-pointer block text-left spring-tap"
          title="Open in Places Map"
          aria-label="View photo location on Places Map"
        >
          <div bind:this={miniMapContainer} class="w-full h-full pointer-events-none"></div>

          <div class="liquid-hud absolute bottom-2 right-2 z-[400] px-2.5 py-1 rounded-full text-[10px] text-[var(--text-main)] flex items-center gap-1.5 transition-transform group-hover:scale-102">
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 text-[var(--text-muted)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polygon points="1 6 1 22 8 18 16 22 23 18 23 2 16 6 8 2 1 6"></polygon>
              <line x1="8" y1="2" x2="8" y2="18"></line>
              <line x1="16" y1="6" x2="16" y2="22"></line>
            </svg>
            <span class="font-medium tracking-tight">Open Map</span>
          </div>
        </button>
      </div>
    {/if}

    <!-- 1. Detected People & Faces -->
    <div class="space-y-2">
      <div class="flex items-center justify-between px-0.5">
        <div class="flex items-center gap-1.5">
          <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)]">People & Faces</span>
          {#if faces.length > 0}
            <span class="text-[9px] font-mono px-1.5 py-0.2 rounded-full bg-purple-500/15 text-purple-600 dark:text-purple-300 border border-purple-500/25">
              {faces.length}
            </span>
          {/if}
        </div>

        {#if faces.length > 0}
          <button
            type="button"
            role="switch"
            aria-checked={showFaces}
            on:click={() => dispatch('toggleFaces')}
            class="switch-track {showFaces ? 'switch-active-purple' : ''}"
            title={showFaces ? 'Hide face boxes' : 'Show face boxes'}
            aria-label="Toggle face bounding boxes"
          >
            <span class="switch-thumb {showFaces ? 'translate-x-3.5 bg-purple-600 dark:bg-purple-200' : 'translate-x-0.5 bg-[var(--text-muted)] opacity-50'}"></span>
          </button>
        {/if}
      </div>

      {#if loadingDetails}
        <div class="text-[11px] text-[var(--text-muted)] font-mono">Scanning faces...</div>
      {:else if faces.length === 0}
        <div class="text-[11px] text-[var(--text-muted)] italic opacity-60">No faces detected</div>
      {:else}
        <div class="space-y-1.5">
          {#each faces as f (f.face_id)}
            <div class="liquid-card group flex flex-col gap-2 p-2.5 rounded-xl">
              <div class="flex items-center gap-2.5">
                <img
                  src={resolveUrl(f.face_thumb_path)}
                  alt=""
                  class="w-7 h-7 rounded-full object-cover liquid-avatar-frame flex-shrink-0"
                />
                <div class="flex-1 min-w-0">
                  {#if editingFaceId === f.face_id}
                    <input
                      type="text"
                      list="known-people-list"
                      bind:value={editingName}
                      placeholder="Name person..."
                      on:blur={() => submitName(f)}
                      on:keydown={(e) => {
                        if (e.key === 'Enter') submitName(f);
                        if (e.key === 'Escape') editingFaceId = null;
                      }}
                      use:focusInput
                      class="liquid-input w-full text-xs text-[var(--text-main)] rounded-lg px-2 py-1 outline-none"
                    />
                  {:else}
                    <div class="flex items-center justify-between">
                      <span class="text-xs font-medium text-[var(--text-main)] truncate">{f.person_name || 'Unnamed'}</span>
                      <div class="flex items-center gap-1">
                        <!-- Rename -->
                        <button
                          type="button"
                          on:click={() => {
                            editingFaceId = f.face_id;
                            reassigningFaceId = null;
                            editingName = f.person_name ?? '';
                          }}
                          class="text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer p-0.5 transition-colors spring-tap"
                          title="Rename person"
                          aria-label="Rename face"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <path d="M12 20h9"></path>
                            <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"></path>
                          </svg>
                        </button>

                        <!-- Reassign / Move to another person -->
                        {#if knownPeople.length > 0}
                          <button
                            type="button"
                            on:click={() => {
                              reassigningFaceId = reassigningFaceId === f.face_id ? null : f.face_id;
                              editingFaceId = null;
                            }}
                            class="text-[var(--text-muted)] hover:text-purple-500 cursor-pointer p-0.5 transition-colors spring-tap"
                            title="Reassign to another person"
                            aria-label="Reassign face"
                          >
                            <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                              <path d="M17 3l4 4-4 4"></path>
                              <path d="M3 7h18"></path>
                              <path d="M7 21l-4-4 4-4"></path>
                              <path d="M21 17H3"></path>
                            </svg>
                          </button>
                        {/if}

                        <!-- Split to New Person -->
                        <button
                          type="button"
                          on:click={() => dispatch('splitFace', { faceId: f.face_id })}
                          class="text-[var(--text-muted)] hover:text-emerald-500 cursor-pointer p-0.5 transition-colors spring-tap"
                          title="Split into a new person identity"
                          aria-label="Split face to new person"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M16 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"></path>
                            <circle cx="8.5" cy="7" r="4"></circle>
                            <line x1="20" y1="8" x2="20" y2="14"></line>
                            <line x1="23" y1="11" x2="17" y2="11"></line>
                          </svg>
                        </button>

                        <!-- Unlink Face (Detach to unassigned) -->
                        <button
                          type="button"
                          on:click={() => dispatch('unlinkFace', { faceId: f.face_id })}
                          class="text-[var(--text-muted)] hover:text-amber-500 cursor-pointer p-0.5 transition-colors spring-tap"
                          title="Unlink face from person"
                          aria-label="Unlink face"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"></path>
                            <polyline points="15 3 21 3 21 9"></polyline>
                            <line x1="10" y1="14" x2="21" y2="3"></line>
                          </svg>
                        </button>

                        <!-- Delete Face Entirely -->
                        <button
                          type="button"
                          on:click={() => dispatch('deleteFace', { faceId: f.face_id })}
                          class="text-[var(--text-muted)] hover:text-rose-500 cursor-pointer p-0.5 transition-colors spring-tap opacity-0 group-hover:opacity-100"
                          title="Dismiss face detection"
                          aria-label="Remove face detection"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                            <line x1="18" y1="6" x2="6" y2="18"></line>
                            <line x1="6" y1="6" x2="18" y2="18"></line>
                          </svg>
                        </button>
                      </div>
                    </div>
                  {/if}
                </div>
              </div>

              <!-- Quick Reassignment Drawer -->
              {#if reassigningFaceId === f.face_id}
                <div class="pt-2 border-t border-[var(--border-glass)] flex flex-col gap-1.5" in:fade={{ duration: 120 }}>
                  <span class="text-[9px] text-[var(--text-muted)] font-mono uppercase tracking-wider">Move face to:</span>
                  <div class="max-h-28 overflow-y-auto space-y-1 no-scrollbar">
                    {#each knownPeople as person (person.id)}
                      {#if person.name && person.id !== f.person_id}
                        <button
                          type="button"
                          on:click={() => {
                            dispatch('reassignFace', { faceId: f.face_id, targetPersonId: person.id });
                            reassigningFaceId = null;
                          }}
                          class="w-full text-left px-2 py-1 rounded-lg text-xs text-[var(--text-main)] hover:bg-[var(--dock-bg-hover)] transition-colors flex items-center justify-between cursor-pointer"
                        >
                          <span class="truncate">{person.name}</span>
                          <span class="text-[9px] text-[var(--text-muted)] font-mono">{person.face_count}</span>
                        </button>
                      {/if}
                    {/each}
                  </div>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <!-- 2. Human Pose Skeletons -->
    {#if poses.length > 0}
      <div class="space-y-1.5">
        <div class="flex items-center justify-between px-0.5">
          <div class="flex items-center gap-1.5">
            <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)]">Human Pose</span>
            <span class="text-[9px] font-mono px-1.5 py-0.2 rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border border-emerald-500/25">
              {poses.length}
            </span>
          </div>

          <button
            type="button"
            role="switch"
            aria-checked={showPoses}
            on:click={() => dispatch('togglePoses')}
            class="switch-track {showPoses ? 'switch-active-emerald' : ''}"
            title={showPoses ? 'Hide pose skeleton' : 'Show pose skeleton'}
            aria-label="Toggle pose skeleton"
          >
            <span class="switch-thumb {showPoses ? 'translate-x-3.5 bg-emerald-600 dark:bg-emerald-200' : 'translate-x-0.5 bg-[var(--text-muted)] opacity-50'}"></span>
          </button>
        </div>

        <p class="text-[10px] text-[var(--text-muted)] font-mono px-0.5">
          {poses.length} {poses.length === 1 ? 'person skeleton' : 'skeletons'} detected (17 keypoints)
        </p>
      </div>
    {/if}

    <!-- 3. Tags -->
    <div class="space-y-1.5">
      <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-0.5">Tags</span>
      {#if loadingDetails}
        <div class="text-[11px] text-[var(--text-muted)] font-mono">Loading tags...</div>
      {:else if tags.length === 0}
        <div class="text-[11px] text-[var(--text-muted)] italic opacity-60">No tags</div>
      {:else}
        <div class="flex flex-wrap gap-1.5">
          {#each tags as t (t.name)}
            <span class="liquid-chip px-2.5 py-1 rounded-full text-[10px] text-[var(--text-main)] font-mono">
              #{t.name}
            </span>
          {/each}
        </div>
      {/if}
    </div>

    <!-- 4. Visually Similar Media -->
    <div class="border-t border-[var(--border-glass)] pt-3.5 space-y-2">
      <div class="flex items-center justify-between px-0.5">
        <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)]">Similar Media</span>
        {#if similarItems.length > 0}
          <span class="text-[10px] text-[var(--text-muted)] font-mono">{similarItems.length} found</span>
        {/if}
      </div>

      {#if loadingSimilar}
        <div class="text-[11px] text-[var(--text-muted)] font-mono">Finding similar...</div>
      {:else if similarItems.length === 0}
        <div class="text-[11px] text-[var(--text-muted)] italic opacity-60">No visually similar items</div>
      {:else}
        <div class="grid grid-cols-3 gap-2">
          {#each similarItems as s (s.id)}
            <button
              type="button"
              on:click={() => dispatch('selectAsset', { id: s.id })}
              class="liquid-card group relative aspect-square rounded-xl overflow-hidden cursor-pointer text-left spring-tap"
              title="Similarity: {Math.round(s.similarity * 100)}%"
              aria-label="Select similar media item"
            >
              <img
                src={resolveUrl(s.thumb_path)}
                alt=""
                loading="lazy"
                class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-200 pointer-events-none"
              />

              {#if s.mime_type.startsWith('video/') || s.mime_type === 'image/gif'}
                <div class="absolute top-1 left-1 bg-black/60 dark:bg-black/80 backdrop-blur-md w-4 h-4 rounded-full flex items-center justify-center text-white shadow-sm">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-2 h-2 fill-current ml-0.5" viewBox="0 0 24 24">
                    <polygon points="5 3 19 12 5 21 5 3"></polygon>
                  </svg>
                </div>
              {/if}

              <div class="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent p-1 opacity-0 group-hover:opacity-100 transition-opacity flex justify-end">
                <span class="text-[8px] font-mono text-white font-medium">
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

<style>
  /* ========================================================================= */
  /* Liquid Micro Toggle Switches                                              */
  /* ========================================================================= */
  .switch-track {
    position: relative;
    display: inline-flex;
    align-items: center;
    width: 28px;
    height: 16px;
    border-radius: 9999px;
    background: var(--pill-bg);
    border: 1px solid var(--border-glass);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.1);
    cursor: pointer;
    transition: all 0.22s cubic-bezier(0.34, 1.56, 0.64, 1);
    outline: none;
    flex-shrink: 0;
  }

  .switch-track:hover {
    border-color: var(--border-subtle);
  }

  .switch-thumb {
    width: 11px;
    height: 11px;
    border-radius: 9999px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
    transition: transform 0.22s cubic-bezier(0.34, 1.56, 0.64, 1), background-color 0.22s ease;
  }

  .switch-active-purple {
    background: rgba(168, 85, 247, 0.2);
    border-color: rgba(168, 85, 247, 0.5);
    box-shadow: 0 0 8px rgba(168, 85, 247, 0.25);
  }

  .switch-active-emerald {
    background: rgba(16, 185, 129, 0.2);
    border-color: rgba(16, 185, 129, 0.5);
    box-shadow: 0 0 8px rgba(16, 185, 129, 0.25);
  }

  /* ========================================================================= */
  /* Sidebar and Container Components                                          */
  /* ========================================================================= */
  .liquid-sidebar {
    background: var(--bg-surface-elevated);
    border-color: var(--border-glass);
    backdrop-filter: blur(40px) saturate(190%);
    -webkit-backdrop-filter: blur(40px) saturate(190%);
    box-shadow:
      0 20px 50px var(--dock-shadow),
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

  .liquid-icon-btn {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    box-shadow: inset 0 1px 0 var(--border-specular);
  }

  .liquid-icon-btn:hover {
    background: var(--dock-bg-hover);
    border-color: var(--border-subtle);
  }

  .liquid-fav-active {
    background: rgba(245, 158, 11, 0.12);
    border-color: rgba(245, 158, 11, 0.28);
    box-shadow: 0 2px 10px rgba(245, 158, 11, 0.15);
  }

  .liquid-map-frame {
    border: 1px solid var(--border-glass);
    box-shadow: 0 4px 16px var(--dock-shadow);
  }

  .liquid-hud {
    background: var(--dock-bg);
    border: 1px solid var(--dock-border);
    backdrop-filter: blur(24px) saturate(180%);
    -webkit-backdrop-filter: blur(24px) saturate(180%);
    box-shadow: 0 4px 12px var(--dock-shadow), inset 0 1px 0 var(--dock-highlight);
  }

  .liquid-avatar-frame {
    background: var(--card-bg);
    border: 1px solid var(--border-glass);
  }

  .liquid-input {
    background: var(--pill-bg);
    border: 1px solid var(--border-subtle);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.08);
  }

  .liquid-chip {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    box-shadow: inset 0 1px 0 var(--border-specular);
  }
</style>