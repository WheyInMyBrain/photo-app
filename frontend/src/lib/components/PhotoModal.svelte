<script lang="ts">
  import { onMount, createEventDispatcher } from 'svelte';

  export let asset: {
    id: string;
    file_name: string;
    thumb_path: string;
    preview_path: string;
    mime_type: string;
    captured_at: string | null;
    is_favorite?: number;
  } | null = null;

  export let hasPrev = false;
  export let hasNext = false;

  const dispatch = createEventDispatcher<{
    close: void;
    prev: void;
    next: void;
    toggleFavorite: { id: string; is_favorite: boolean };
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

  let faces: FaceDetail[] = [];
  let tags: TagItem[] = [];
  let knownPeople: PersonCandidate[] = [];
  let loadingDetails = true;
  let showBoxes = true;
  let isFavorite = false;

  let editingFaceId: string | null = null;
  let editingName = '';

  // Svelte action to replace HTML autofocus and eliminate a11y warnings
  function focusInput(node: HTMLElement) {
    node.focus();
  }

  $: if (asset?.id) {
    isFavorite = Boolean(asset.is_favorite);
    editingFaceId = null;
    loadDetails(asset.id);
  }

  onMount(async () => {
    try {
      const res = await fetch('/api/persons/names');
      if (res.ok) knownPeople = await res.json();
    } catch (e) {
      console.error('Failed to load names directory', e);
    }
  });

  async function loadDetails(id: string) {
    loadingDetails = true;
    try {
      const [fRes, tRes] = await Promise.all([
        fetch(`/api/assets/${id}/faces`),
        fetch(`/api/assets/${id}/tags`)
      ]);
      faces = fRes.ok ? await fRes.json() : [];
      tags = tRes.ok ? await tRes.json() : [];
    } finally {
      loadingDetails = false;
    }
  }

  async function toggleFavorite() {
    if (!asset) return;
    isFavorite = !isFavorite;
    try {
      const res = await fetch(`/api/assets/${asset.id}/favorite`, { method: 'POST' });
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

    // Check if entered name already belongs to an existing person cluster
    const matched = knownPeople.find(
      (p) => p.name?.toLowerCase() === clean.toLowerCase()
    );

    if (matched && matched.id !== face.person_id) {
      // 1. Move face to existing person cluster
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
      // 2. Assign brand new name to this cluster
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
    if (editingFaceId) return;
    if (e.key === 'Escape') dispatch('close');
    else if (e.key === 'ArrowLeft' && hasPrev) dispatch('prev');
    else if (e.key === 'ArrowRight' && hasNext) dispatch('next');
    else if (e.key.toLowerCase() === 'f') toggleFavorite();
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- Explicitly closed <option> tag prevents Vite compiler warning -->
<datalist id="known-people-list">
  {#each knownPeople as person}
    {#if person.name}
      <option value={person.name}>{person.name}</option>
    {/if}
  {/each}
</datalist>

{#if asset}
  <div class="fixed inset-0 z-50 flex bg-black/90 backdrop-blur-md select-none">
    <button
      type="button"
      on:click={() => dispatch('close')}
      class="absolute top-4 left-4 z-20 text-neutral-400 hover:text-white bg-neutral-900/80 p-2 rounded-full cursor-pointer"
      title="Close (Esc)"
    >
      ✕
    </button>

    <!-- Main Canvas -->
    <div class="flex-1 relative flex items-center justify-center p-4">
      <div class="relative max-h-full max-w-full flex items-center justify-center">
        {#if asset.mime_type.startsWith('video/')}
          <video
            src="/api/assets/{asset.id}/stream"
            controls
            autoplay
            class="max-h-[90vh] max-w-[75vw] rounded shadow-2xl"
          >
            <track kind="captions" />
          </video>
        {:else}
          <div class="relative inline-block">
            <img
              src="/api/assets/{asset.id}/stream"
              alt={asset.file_name}
              class="max-h-[90vh] max-w-[75vw] object-contain rounded shadow-2xl block"
            />

            {#if showBoxes}
              {#each faces as f}
                <div
                  class="absolute border border-emerald-400/80 bg-emerald-400/10 rounded cursor-pointer group z-10"
                  style="left: {f.bbox_x * 100}%; top: {f.bbox_y * 100}%; width: {f.bbox_w * 100}%; height: {f.bbox_h * 100}%;"
                  on:click={() => { editingFaceId = f.face_id; editingName = f.person_name ?? ''; }}
                  role="button"
                  tabindex="0"
                  on:keydown={(e) => e.key === 'Enter' && (editingFaceId = f.face_id)}
                >
                  <span class="absolute -bottom-6 left-1/2 -translate-x-1/2 bg-neutral-900/90 text-[10px] text-emerald-300 px-1.5 py-0.5 rounded shadow whitespace-nowrap opacity-80 group-hover:opacity-100 border border-neutral-700">
                    {f.person_name || 'Unnamed'}
                  </span>
                </div>
              {/each}
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <!-- Metadata Drawer -->
    <aside class="w-80 border-l border-neutral-800 bg-neutral-950 p-5 flex flex-col justify-between overflow-y-auto space-y-6">
      <div class="space-y-5">
        <div class="flex items-center justify-between border-b border-neutral-900 pb-3">
          <div class="truncate mr-2">
            <h3 class="font-medium text-xs text-white truncate" title={asset.file_name}>{asset.file_name}</h3>
            <p class="text-[10px] text-neutral-500 mt-0.5">
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

        <!-- People Section with Datalist Input -->
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
              {#each faces as f}
                <div class="flex items-center gap-2.5 bg-neutral-900/60 border border-neutral-800/60 p-2 rounded-lg">
                  <img src="/{f.face_thumb_path}" alt="" class="w-8 h-8 rounded-full object-cover border border-neutral-700" />
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
              {#each tags as t}
                <span class="px-2 py-0.5 rounded text-[10px] bg-neutral-900 border border-neutral-800 text-neutral-300">
                  #{t.name}
                </span>
              {/each}
            </div>
          {/if}
        </div>
      </div>
    </aside>
  </div>
{/if}