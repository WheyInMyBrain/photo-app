<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { browser } from '$app/environment';
  import { filterStore } from '$lib/stores/filterStore';

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
    if (!browser) return;
    isLoading = true;
    try {
      const isPrivate = $filterStore.is_private;
      const [pRes, nRes] = await Promise.all([
        fetch(`/api/smart-albums/people?is_private=${isPrivate}`),
        fetch('/api/persons/names')
      ]);
      if (pRes.ok) people = await pRes.json();
      if (nRes.ok) nameDirectory = await nRes.json();
    } catch (e) {
      console.error('Failed loading people', e);
    } finally {
      isLoading = false;
    }
  }

  $: if (browser && $filterStore.is_private !== undefined) {
    loadPeople();
  }

  async function saveName(person: PersonCard) {
    const clean = editingName.trim();
    editingId = null;
    if (!clean || clean === person.name) return;

    // Check if the typed name already matches an existing person in the directory
    const matched = nameDirectory.find(
      (n) => n.name?.toLowerCase() === clean.toLowerCase() && n.id !== person.id
    );

    if (matched) {
      // Auto-merge: User chose an existing identity
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
      // Fresh new name for this cluster
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

  function selectPerson(person: PersonCard) {
    filterStore.togglePerson(person.id);
    goto('/');
  }

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  onMount(loadPeople);
</script>

<!-- Datalist Autocomplete Source for Inline Renaming -->
<datalist id="people-name-suggestions">
  {#each nameDirectory as item (item.id)}
    {#if item.name}
      <option value={item.name}>{item.name}</option>
    {/if}
  {/each}
</datalist>

<div class="p-6 max-w-7xl mx-auto">
  <div class="flex items-center justify-between pb-4 mb-6 border-b border-neutral-900">
    <div>
      <div class="flex items-center gap-2">
        <h1 class="text-xl font-bold text-white">People</h1>
        {#if $filterStore.is_private}
          <span class="text-[10px] bg-purple-950/80 text-purple-300 border border-purple-800/60 px-1.5 py-0.2 rounded font-mono font-medium">
            PRIVATE VAULT
          </span>
        {/if}
      </div>
      <p class="text-xs text-neutral-400">Click to filter timeline. Drag onto another person to merge.</p>
    </div>
    <span class="text-xs text-neutral-500">{people.length} identities</span>
  </div>

  {#if isLoading}
    <div class="text-xs text-neutral-500 text-center py-12">Loading people...</div>
  {:else if people.length === 0}
    <div class="text-xs text-neutral-500 text-center py-12">
      {$filterStore.is_private ? 'No private faces found.' : 'No public faces found.'}
    </div>
  {:else}
    <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-3">
      {#each people as p}
        <div
          role="button"
          tabindex="0"
          draggable="true"
          on:dragstart={(e) => { source = p; e.dataTransfer?.setData('text/plain', p.id); }}
          on:dragover|preventDefault
          on:drop={() => { if (source && source.id !== p.id) target = p; }}
          on:click={() => selectPerson(p)}
          on:keydown={(e) => e.key === 'Enter' && selectPerson(p)}
          class="bg-neutral-900/70 hover:bg-neutral-900 border {$filterStore.person_ids.has(p.id) ? 'border-purple-500' : 'border-neutral-800 hover:border-neutral-700'} rounded-xl p-3 flex flex-col items-center text-center cursor-pointer transition-all select-none"
        >
          <div class="w-20 h-20 rounded-full overflow-hidden bg-neutral-800 border border-neutral-700 mb-2">
            {#if p.avatar_thumb}
              <img src="/{p.avatar_thumb}" alt={p.name ?? ''} class="w-full h-full object-cover" />
            {:else}
              <div class="w-full h-full flex items-center justify-center text-xl text-neutral-500">👤</div>
            {/if}
          </div>

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
              class="w-full bg-black border border-purple-500 text-xs text-center text-white rounded px-1 outline-none"
            />
          {:else}
            <div class="flex items-center justify-center gap-1 w-full px-1">
              <span class="text-xs font-medium text-neutral-200 truncate max-w-[100px]">{p.name || 'Unnamed'}</span>
              <button
                type="button"
                on:click|stopPropagation={() => { editingId = p.id; editingName = p.name ?? ''; }}
                class="text-[10px] text-neutral-500 hover:text-white cursor-pointer"
                title="Rename person"
              >
                ✎
              </button>
            </div>
          {/if}
          <span class="text-[10px] text-neutral-500 mt-0.5">{p.face_count} photos</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<!-- Merge Confirmation Modal -->
{#if source && target}
  <div class="fixed inset-0 bg-black/80 flex items-center justify-center p-4 z-50">
    <div class="bg-neutral-900 border border-neutral-800 rounded-xl p-5 max-w-xs w-full space-y-3 text-center">
      <h3 class="text-sm font-bold text-white">Merge Identities?</h3>
      <p class="text-xs text-neutral-400">
        Merge <strong>{source.name || 'Unnamed'}</strong> into <strong>{target.name || 'Unnamed'}</strong>?
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