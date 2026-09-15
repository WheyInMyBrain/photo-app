<script lang="ts">
  import { onMount, onDestroy, createEventDispatcher } from 'svelte';
  import { authStore } from '$lib/stores/authStore';

  export let asset: {
    id: string;
    file_name: string;
    thumb_path: string;
    preview_path: string;
    mime_type: string;
    captured_at: string | null;
    is_favorite?: number;
  } | null = null;

  export let prevAsset: { id: string; mime_type: string } | null = null;
  export let nextAsset: { id: string; mime_type: string } | null = null;
  export let hasPrev = false;
  export let hasNext = false;

  const dispatch = createEventDispatcher<{
    close: void;
    prev: void;
    next: void;
    toggleFavorite: { id: string; is_favorite: boolean };
    selectAsset: { id: string };
  }>();

  interface FaceDetail {
    face_id: string;
    person_id: string | null;
    person_name: string | null;
    face_thumb_path: string;
    bbox_x: number;
    bbox_y: number;
    bbox_w: number;
    bbox_h: number;
    score: number;
  }

  interface TagItem {
    name: string;
    confidence: number;
  }

  interface PersonCandidate {
    id: string;
    name: string | null;
  }

  interface SimilarItem {
    id: string;
    thumb_path: string;
    mime_type: string;
    similarity: number;
  }

  let faces: FaceDetail[] = [];
  let tags: TagItem[] = [];
  let similarItems: SimilarItem[] = [];
  let knownPeople: PersonCandidate[] = [];
  let loadingDetails = true;
  let loadingSimilar = true;
  let showBoxes = true;
  let isFavorite = false;

  let editingFaceId: string | null = null;
  let editingName = '';

  let detailAbortCtrl: AbortController | null = null;

  function focusInput(node: HTMLElement) {
    node.focus();
  }

  $: isMotionMedia =
    Boolean(asset?.mime_type?.startsWith('video/')) || asset?.mime_type === 'image/gif';

  function isImage(item: { mime_type?: string } | null): boolean {
    if (!item?.mime_type) return false;
    return !item.mime_type.startsWith('video/') && item.mime_type !== 'image/gif';
  }

  function resolveThumbUrl(path: string): string {
    if (!path) return '';
    if (path.startsWith('http')) return path;

    const clean = path.startsWith('/') ? path.slice(1) : path;
    if (clean.startsWith('users/')) {
      return `/${clean}`;
    }

    return `/thumbs/${clean}`;
  }

  $: if (nextAsset && isImage(nextAsset)) {
    const img = new Image();
    img.src = `/api/assets/${nextAsset.id}/stream`;
  }

  $: if (prevAsset && isImage(prevAsset)) {
    const img = new Image();
    img.src = `/api/assets/${prevAsset.id}/stream`;
  }

  $: if (asset?.id) {
    isFavorite = Boolean(asset.is_favorite);
    editingFaceId = null;
    loadDetails(asset.id);
  }

  onMount(() => {
    const originalOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';

    fetch('/api/persons/names')
      .then((res) => {
        if (res.status === 401) {
          authStore.checkStatus();
          return [];
        }
        return res.ok ? res.json() : [];
      })
      .then((data) => {
        knownPeople = data;
      })
      .catch((e) => console.error('Failed to load names directory', e));

    return () => {
      document.body.style.overflow = originalOverflow;
    };
  });

  onDestroy(() => {
    if (detailAbortCtrl) detailAbortCtrl.abort();
  });

  async function loadDetails(id: string) {
    if (detailAbortCtrl) {
      detailAbortCtrl.abort();
    }
    detailAbortCtrl = new AbortController();
    loadingDetails = true;
    loadingSimilar = true;

    try {
      const [fRes, tRes, sRes] = await Promise.all([
        fetch(`/api/assets/${id}/faces`, { signal: detailAbortCtrl.signal }),
        fetch(`/api/assets/${id}/tags`, { signal: detailAbortCtrl.signal }),
        fetch(`/api/assets/${id}/similar`, { signal: detailAbortCtrl.signal })
      ]);

      if (fRes.status === 401 || tRes.status === 401 || sRes.status === 401) {
        authStore.checkStatus();
        return;
      }

      faces = fRes.ok ? await fRes.json() : [];
      tags = tRes.ok ? await tRes.json() : [];
      similarItems = sRes.ok ? await sRes.json() : [];
    } catch (e: any) {
      if (e?.name !== 'AbortError') {
        faces = [];
        tags = [];
        similarItems = [];
      }
    } finally {
      loadingDetails = false;
      loadingSimilar = false;
    }
  }

  async function toggleFavorite() {
    if (!asset) return;
    isFavorite = !isFavorite;
    try {
      const res = await fetch(`/api/assets/${asset.id}/favorite`, { method: 'POST' });
      if (res.status === 401) {
        authStore.checkStatus();
        return;
      }
      if (res.ok) {
        const data = await res.json();
        isFavorite = data.is_favorite;
        dispatch('toggleFavorite', data);
      }
    } catch {
      isFavorite = !isFavorite;
    }
  }

  async function saveFaceName(face: FaceDetail) {
    const clean = editingName.trim();
    editingFaceId = null;
    if (!clean || clean === face.person_name) return;

    const matched = knownPeople.find(
      (p) => p.name?.toLowerCase() === clean.toLowerCase()
    );

    if (matched && matched.id !== face.person_id) {
      const res = await fetch(`/api/faces/${face.face_id}/reassign`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ target_person_id: matched.id })
      });
      if (res.ok) {
        face.person_id = matched.id;
        face.person_name = matched.name;
        faces = [...faces];
      }
    } else if (face.person_id) {
      const res = await fetch(`/api/persons/${face.person_id}/name`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: clean })
      });
      if (res.ok) {
        face.person_name = clean;
        faces = [...faces];
        if (!knownPeople.some((p) => p.id === face.person_id)) {
          knownPeople = [...knownPeople, { id: face.person_id, name: clean }];
        }
      }
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    // Ignore global keyboard triggers if user is actively naming a person
    if (editingFaceId || (e.target as HTMLElement)?.tagName === 'INPUT') return;

    if (e.key === 'Escape') dispatch('close');
    else if (e.key === 'ArrowLeft' && hasPrev) dispatch('prev');
    else if (e.key === 'ArrowRight' && hasNext) dispatch('next');
    else if (e.key.toLowerCase() === 'f') toggleFavorite();
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<datalist id="known-people-list">
  {#each knownPeople as person (person.id)}
    {#if person.name}
      <option value={person.name}>{person.name}</option>
    {/if}
  {/each}
</datalist>

{#if asset}
  <div class="fixed inset-0 z-50 flex bg-black/95 backdrop-blur-md select-none">
    <!-- Top-Left Close Button -->
    <button
      type="button"
      on:click={() => dispatch('close')}
      class="absolute top-4 left-4 z-30 text-neutral-400 hover:text-white bg-neutral-900/80 hover:bg-neutral-800 p-2.5 rounded-full transition-colors cursor-pointer"
      title="Close (Esc)"
    >
      ✕
    </button>

    <!-- Main Viewport -->
    <div class="flex-1 relative flex items-center justify-center p-4 overflow-hidden">
      {#if hasPrev}
        <button
          type="button"
          on:click={() => dispatch('prev')}
          class="absolute left-6 z-20 text-white/70 hover:text-white bg-black/40 hover:bg-black/80 border border-neutral-800/80 p-3 rounded-full transition-all cursor-pointer backdrop-blur-xs"
          title="Previous (←)"
        >
          ‹
        </button>
      {/if}

      {#if hasNext}
        <button
          type="button"
          on:click={() => dispatch('next')}
          class="absolute right-6 z-20 text-white/70 hover:text-white bg-black/40 hover:bg-black/80 border border-neutral-800/80 p-3 rounded-full transition-all cursor-pointer backdrop-blur-xs"
          title="Next (→)"
        >
          ›
        </button>
      {/if}

      <div class="relative max-h-full max-w-full flex items-center justify-center">
        {#key asset.id}
          {#if isMotionMedia}
            <!-- Uses generated 720p H.264 FastStart preview MP4 with WebP poster -->
            <video
              src={resolveThumbUrl(asset.preview_path)}
              poster={resolveThumbUrl(asset.thumb_path)}
              controls={asset.mime_type !== 'image/gif'}
              autoplay
              loop={asset.mime_type === 'image/gif'}
              muted={asset.mime_type === 'image/gif'}
              playsinline
              preload="metadata"
              class="max-h-[90vh] max-w-[75vw] rounded-lg shadow-2xl object-contain bg-black"
            >
              <track kind="captions" />
            </video>
          {:else}
            <div class="relative inline-block">
              <img
                src="/api/assets/{asset.id}/stream"
                alt={asset.file_name}
                decoding="async"
                class="max-h-[90vh] max-w-[75vw] object-contain rounded-lg shadow-2xl block select-none pointer-events-none"
              />

              {#if showBoxes}
                {#each faces as f (f.face_id)}
                  <div
                    class="absolute border border-emerald-400/80 bg-emerald-400/10 rounded cursor-pointer group z-10 hover:border-emerald-300 hover:bg-emerald-400/20 transition-colors"
                    style="left: {f.bbox_x * 100}%; top: {f.bbox_y * 100}%; width: {f.bbox_w * 100}%; height: {f.bbox_h * 100}%;"
                    on:click={() => { editingFaceId = f.face_id; editingName = f.person_name ?? ''; }}
                    role="button"
                    tabindex="0"
                    on:keydown={(e) => e.key === 'Enter' && (editingFaceId = f.face_id)}
                  >
                    <span class="absolute -bottom-6 left-1/2 -translate-x-1/2 bg-neutral-900/90 text-[10px] text-emerald-300 px-1.5 py-0.5 rounded shadow whitespace-nowrap opacity-80 group-hover:opacity-100 border border-neutral-700 pointer-events-none">
                      {f.person_name || 'Unnamed'}
                    </span>
                  </div>
                {/each}
              {/if}
            </div>
          {/if}
        {/key}
      </div>
    </div>

    <!-- Metadata Drawer -->
    <aside class="w-80 border-l border-neutral-800 bg-neutral-950 p-5 flex flex-col justify-between overflow-y-auto space-y-6 flex-shrink-0">
      <div class="space-y-5">
        <div class="flex items-center justify-between border-b border-neutral-900 pb-3">
          <div class="truncate mr-2">
            <h3 class="font-medium text-xs text-white truncate" title={asset.file_name}>{asset.file_name}</h3>
            <p class="text-[10px] text-neutral-500 mt-0.5 font-mono">
              {asset.captured_at ? new Date(asset.captured_at).toLocaleDateString() : 'Undated'}
            </p>
          </div>
          <button
            type="button"
            on:click={toggleFavorite}
            class="text-sm p-1.5 rounded hover:bg-neutral-900 transition-colors cursor-pointer {isFavorite ? 'text-amber-400' : 'text-neutral-600 hover:text-white'}"
            title="Toggle Favorite (F)"
          >
            ★
          </button>
        </div>

        <!-- People Section -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <span class="text-[10px] uppercase font-semibold text-neutral-500 tracking-wider">People</span>
            {#if faces.length > 0}
              <button
                type="button"
                on:click={() => (showBoxes = !showBoxes)}
                class="text-[10px] text-blue-400 hover:text-blue-300 cursor-pointer"
              >
                {showBoxes ? 'Hide Boxes' : 'Show Boxes'}
              </button>
            {/if}
          </div>

          {#if loadingDetails}
            <div class="text-xs text-neutral-600">Reading faces...</div>
          {:else if faces.length === 0}
            <div class="text-xs text-neutral-600 italic">No faces detected</div>
          {:else}
            <div class="space-y-2">
              {#each faces as f (f.face_id)}
                <div class="flex items-center gap-2.5 bg-neutral-900/60 border border-neutral-800/60 p-2 rounded-lg">
                  <img
                    src={resolveThumbUrl(f.face_thumb_path)}
                    alt=""
                    class="w-8 h-8 rounded-full object-cover border border-neutral-700 bg-neutral-800"
                  />
                  <div class="flex-1 min-w-0">
                    {#if editingFaceId === f.face_id}
                      <input
                        type="text"
                        list="known-people-list"
                        bind:value={editingName}
                        placeholder="Search or enter name..."
                        on:blur={() => saveFaceName(f)}
                        on:keydown={(e) => {
                          if (e.key === 'Enter') saveFaceName(f);
                          if (e.key === 'Escape') editingFaceId = null;
                        }}
                        use:focusInput
                        class="w-full bg-black border border-blue-500 text-xs text-white rounded px-1.5 py-0.5 outline-none"
                      />
                    {:else}
                      <div class="flex items-center justify-between">
                        <span class="text-xs text-neutral-300 truncate">{f.person_name || 'Unnamed'}</span>
                        <button
                          type="button"
                          on:click={() => { editingFaceId = f.face_id; editingName = f.person_name ?? ''; }}
                          class="text-[10px] text-neutral-500 hover:text-white ml-1 cursor-pointer"
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

        <!-- Tags Section -->
        <div>
          <span class="text-[10px] uppercase font-semibold text-neutral-500 tracking-wider block mb-2">Tags</span>
          {#if loadingDetails}
            <div class="text-xs text-neutral-600">Reading tags...</div>
          {:else if tags.length === 0}
            <div class="text-xs text-neutral-600 italic">No tags</div>
          {:else}
            <div class="flex flex-wrap gap-1">
              {#each tags as t (t.name)}
                <span class="px-2 py-0.5 rounded text-[10px] bg-neutral-900 border border-neutral-800 text-neutral-300">
                  #{t.name}
                </span>
              {/each}
            </div>
          {/if}
        </div>

        <!-- Visual Similarity Section -->
        <div class="border-t border-neutral-900 pt-4">
          <div class="flex items-center justify-between mb-2.5">
            <span class="text-[10px] uppercase font-semibold text-neutral-500 tracking-wider">Similar Media</span>
            {#if similarItems.length > 0}
              <span class="text-[10px] text-neutral-500 font-mono">{similarItems.length} found</span>
            {/if}
          </div>

          {#if loadingSimilar}
            <div class="text-xs text-neutral-600">Finding visually similar...</div>
          {:else if similarItems.length === 0}
            <div class="text-xs text-neutral-600 italic">No visually similar items</div>
          {:else}
            <div class="grid grid-cols-3 gap-2">
              {#each similarItems as s (s.id)}
                <button
                  type="button"
                  on:click={() => dispatch('selectAsset', { id: s.id })}
                  class="group relative aspect-square rounded-md overflow-hidden bg-neutral-900 border border-neutral-800/80 hover:border-blue-500/80 transition-all cursor-pointer text-left"
                  title="Similarity: {Math.round(s.similarity * 100)}%"
                >
                  <img
                    src={resolveThumbUrl(s.thumb_path)}
                    alt=""
                    loading="lazy"
                    class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-200"
                  />
                  {#if s.mime_type.startsWith('video/') || s.mime_type === 'image/gif'}
                    <div class="absolute top-1 left-1 bg-black/60 backdrop-blur-xs px-1 py-0.5 rounded text-[8px] text-white">
                      ▶
                    </div>
                  {/if}
                  <div class="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/80 to-transparent p-1 opacity-0 group-hover:opacity-100 transition-opacity flex justify-end">
                    <span class="text-[9px] font-mono text-blue-300">
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
  </div>
{/if}