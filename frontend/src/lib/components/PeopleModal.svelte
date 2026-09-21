<script lang="ts">
  import { onMount, createEventDispatcher } from 'svelte';
  import { browser } from '$app/environment';
  import { filterStore } from '$lib/stores/filterStore';
  import { authStore } from '$lib/stores/authStore';

  export let isOpen = false;

  const dispatch = createEventDispatcher<{
    close: void;
    selectPerson: { personId: string };
  }>();

  interface PersonCard {
    id: string;
    name: string | null;
    face_count: number;
    avatar_thumb: string | null;
  }

  interface NameDirectoryItem {
    id: string;
    name: string | null;
  }

  let people: PersonCard[] = [];
  let nameDirectory: NameDirectoryItem[] = [];
  let isLoading = true;
  let editingId: string | null = null;
  let editingName = '';

  // Merge modal state
  let source: PersonCard | null = null;
  let target: PersonCard | null = null;
  let isMerging = false;

  async function loadPeople() {
    if (!browser || !$authStore.isAuthenticated) return;
    isLoading = true;
    try {
      const [pRes, nRes] = await Promise.all([
        fetch('/api/smart-albums/people'),
        fetch('/api/persons/names')
      ]);

      if (pRes.status === 401 || nRes.status === 401) {
        authStore.checkStatus();
        return;
      }

      if (pRes.ok) people = await pRes.json();
      if (nRes.ok) nameDirectory = await nRes.json();
    } catch (e) {
      console.error('Failed loading people', e);
    } finally {
      isLoading = false;
    }
  }

  $: if (isOpen && browser &&$authStore.isAuthenticated) {
    loadPeople();
  }

  async function saveName(person: PersonCard) {
    const clean = editingName.trim();
    editingId = null;
    if (!clean || clean === person.name) return;

    const matched = nameDirectory.find(
      (n) => n.name?.toLowerCase() === clean.toLowerCase() && n.id !== person.id
    );

    if (matched) {
      try {
        const res = await fetch('/api/persons/merge', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ source_person_id: person.id, target_person_id: matched.id })
        });
        if (res.ok) {
          await loadPeople();
        }
      } catch (err) {
        console.error('Auto-merge failed:', err);
      }
    } else {
      const res = await fetch(`/api/persons/${person.id}/name`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: clean })
      });
      if (res.ok) {
        person.name = clean;
        people = [...people];
        if (!nameDirectory.some((n) => n.id === person.id)) {
          nameDirectory = [...nameDirectory, { id: person.id, name: clean }];
        }
      }
    }
  }

  async function confirmMerge() {
    if (!source || !target) return;
    isMerging = true;
    try {
      const res = await fetch('/api/persons/merge', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ source_person_id: source.id, target_person_id: target.id })
      });
      if (res.ok) {
        source = null;
        target = null;
        await loadPeople();
      }
    } finally {
      isMerging = false;
    }
  }

  function handleSelect(person: PersonCard) {
    filterStore.togglePerson(person.id);
    dispatch('selectPerson', { personId: person.id });
    dispatch('close');
  }

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!isOpen) return;
    if (e.key === 'Escape') {
      if (source || target) {
        source = null;
        target = null;
      } else if (editingId) {
        editingId = null;
      } else {
        dispatch('close');
      }
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<datalist id="people-name-suggestions">
  {#each nameDirectory as item (item.id)}
    {#if item.name}
      <option value={item.name}>{item.name}</option>
    {/if}
  {/each}
</datalist>

{#if isOpen}
  <!-- Dim Backdrop -->
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div
    class="fixed inset-0 z-50 bg-black/80 backdrop-blur-sm flex items-center justify-center p-3 sm:p-6 select-none"
    on:click|self={() => dispatch('close')}
  >
    <!-- Modal Dialog Window -->
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Manage People"
      class="bg-neutral-950 border border-neutral-800 rounded-2xl w-full max-w-4xl max-h-[85vh] flex flex-col shadow-2xl overflow-hidden"
    >
      <!-- Header -->
      <div class="px-5 py-4 border-b border-neutral-800/80 flex items-center justify-between">
        <div>
          <h2 class="text-base sm:text-lg font-bold text-white flex items-center gap-2">
            <span>👤</span>
            <span>People & Faces</span>
          </h2>
          <p class="text-[11px] text-neutral-400 mt-0.5">
            Select to filter timeline. Drag onto another card to merge identities.
          </p>
        </div>

        <div class="flex items-center gap-3">
          <span class="text-xs text-neutral-500 font-mono hidden sm:inline">
            {people.length} identities
          </span>
          <button
            type="button"
            on:click={() => dispatch('close')}
            class="p-1.5 rounded-lg bg-neutral-900 hover:bg-neutral-800 text-neutral-400 hover:text-white border border-neutral-800 transition-colors cursor-pointer text-xs"
            title="Close (Esc)"
            aria-label="Close dialog"
          >
            ✕
          </button>
        </div>
      </div>

      <!-- Content Area -->
      <div class="p-4 sm:p-6 overflow-y-auto flex-1">
        {#if isLoading}
          <div class="text-xs text-neutral-500 text-center py-16">Reading faces from library...</div>
        {:else if people.length === 0}
          <div class="text-xs text-neutral-500 text-center py-16">
            No identified faces found in your library yet.
          </div>
        {:else}
          <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
            {#each people as p (p.id)}
              {@const isFiltered = $filterStore.person_ids.has(p.id)}
              <div
                role="button"
                tabindex="0"
                draggable="true"
                on:dragstart={(e) => { source = p; e.dataTransfer?.setData('text/plain', p.id); }}
                on:dragover|preventDefault
                on:drop={() => { if (source && source.id !== p.id) target = p; }}
                on:click={() => handleSelect(p)}
                on:keydown={(e) => e.key === 'Enter' && handleSelect(p)}
                class="bg-neutral-900/60 hover:bg-neutral-900 border {isFiltered ? 'border-purple-500 ring-2 ring-purple-500/30' : 'border-neutral-800 hover:border-neutral-700'} rounded-xl p-3 flex flex-col items-center text-center cursor-pointer transition-all"
              >
                <!-- Avatar -->
                <div class="w-16 h-16 sm:w-20 sm:h-20 rounded-full overflow-hidden bg-neutral-800 border border-neutral-700 mb-2">
                  {#if p.avatar_thumb}
                    <img
                      src={p.avatar_thumb.startsWith('/') ? p.avatar_thumb : `/${p.avatar_thumb}`}
                      alt={p.name ?? 'Person'}
                      loading="lazy"
                      class="w-full h-full object-cover pointer-events-none"
                    />
                  {:else}
                    <div class="w-full h-full flex items-center justify-center text-xl text-neutral-500">👤</div>
                  {/if}
                </div>

                <!-- Editable Name -->
                {#if editingId === p.id}
                  <input
                    type="text"
                    list="people-name-suggestions"
                    use:focusOnMount
                    bind:value={editingName}
                    on:click|stopPropagation
                    on:blur={() => saveName(p)}
                    on:keydown={(e) => {
                      if (e.key === 'Enter') saveName(p);
                      if (e.key === 'Escape') editingId = null;
                    }}
                    class="w-full bg-black border border-purple-500 text-xs text-center text-white rounded px-1 py-0.5 outline-none"
                  />
                {:else}
                  <div class="flex items-center justify-center gap-1 w-full px-1">
                    <span class="text-xs font-medium text-neutral-200 truncate max-w-[90px] sm:max-w-[110px]">
                      {p.name || 'Unnamed'}
                    </span>
                    <button
                      type="button"
                      on:click|stopPropagation={() => { editingId = p.id; editingName = p.name ?? ''; }}
                      class="text-[10px] text-neutral-500 hover:text-white cursor-pointer p-0.5"
                      title="Rename person"
                    >
                      ✎
                    </button>
                  </div>
                {/if}

                <span class="text-[10px] text-neutral-500 mt-0.5 font-mono">
                  {p.face_count} {p.face_count === 1 ? 'photo' : 'photos'}
                </span>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<!-- Nested Merge Confirmation Dialog -->
{#if source && target}
  <div class="fixed inset-0 bg-black/85 flex items-center justify-center p-4 z-[60]">
    <div class="bg-neutral-900 border border-neutral-800 rounded-xl p-5 max-w-xs w-full space-y-3 text-center shadow-2xl">
      <h3 class="text-sm font-bold text-white">Merge Identities?</h3>
      <p class="text-xs text-neutral-300">
        Merge <strong>{source.name || 'Unnamed'}</strong> into <strong>{target.name || 'Unnamed'}</strong>?
      </p>
      <p class="text-[10px] text-neutral-500">
        All photo associations will point to {target.name || 'this identity'}.
      </p>
      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          on:click={() => { source = null; target = null; }}
          class="px-3 py-1 rounded text-xs text-neutral-400 hover:text-white cursor-pointer"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={confirmMerge}
          disabled={isMerging}
          class="px-3 py-1 bg-purple-600 hover:bg-purple-500 text-white rounded text-xs font-semibold cursor-pointer"
        >
          {isMerging ? 'Merging...' : 'Merge'}
        </button>
      </div>
    </div>
  </div>
{/if}