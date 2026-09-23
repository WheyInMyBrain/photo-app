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
  <!-- Backdrop -->
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div
    class="fixed inset-0 z-50 bg-black/70 backdrop-blur-md flex items-center justify-center p-3 sm:p-6 select-none"
    on:click|self={() => dispatch('close')}
  >
    <!-- Modal Window Container -->
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Manage People"
      class="glass-panel rounded-3xl w-full max-w-4xl max-h-[85vh] flex flex-col shadow-2xl overflow-hidden border border-[var(--border-glass)] bg-[var(--bg-surface-elevated)]"
    >
      <!-- Header -->
      <div class="px-6 py-4.5 border-b border-[var(--border-glass)] flex items-center justify-between">
        <div>
          <h2 class="text-sm sm:text-base font-bold text-[var(--text-main)] flex items-center gap-2">
            <span>👤</span>
            <span>People & Faces</span>
          </h2>
          <p class="text-[11px] text-[var(--text-muted)] mt-0.5">
            Click to filter timeline. Drag a card onto another to merge duplicate identities.
          </p>
        </div>

        <div class="flex items-center gap-3">
          <span class="text-xs text-[var(--text-muted)] font-mono hidden sm:inline">
            {people.length} identities
          </span>
          <button
            type="button"
            on:click={() => dispatch('close')}
            class="w-7 h-7 flex items-center justify-center rounded-full glass-panel text-[var(--text-muted)] hover:text-[var(--text-main)] transition-colors spring-tap cursor-pointer text-xs"
            title="Close (Esc)"
            aria-label="Close dialog"
          >
            ✕
          </button>
        </div>
      </div>

      <!-- Identity Grid Area -->
      <div class="p-4 sm:p-6 overflow-y-auto flex-1 no-scrollbar">
        {#if isLoading}
          <div class="flex flex-col items-center justify-center py-20 gap-2.5 text-[var(--text-muted)]">
            <div class="w-6 h-6 border-2 border-purple-500/20 border-t-purple-500 rounded-full animate-spin"></div>
            <span class="text-xs font-medium">Scanning face embeddings...</span>
          </div>
        {:else if people.length === 0}
          <div class="text-xs text-[var(--text-muted)] text-center py-20 font-medium">
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
                class="glass-panel rounded-2xl p-3 flex flex-col items-center text-center cursor-pointer transition-all spring-tap border border-[var(--border-glass)] hover:border-purple-500/50 hover:shadow-lg {isFiltered ? 'ring-2 ring-purple-500 shadow-md bg-purple-500/10' : ''}"
              >
                <!-- Avatar Circular Frame -->
                <div class="w-16 h-16 sm:w-20 sm:h-20 rounded-full overflow-hidden bg-[var(--bg-surface)] border-2 border-[var(--border-glass)] mb-2.5 shadow-sm">
                  {#if p.avatar_thumb}
                    <img
                      src={p.avatar_thumb.startsWith('/') ? p.avatar_thumb : `/${p.avatar_thumb}`}
                      alt={p.name ?? 'Person'}
                      loading="lazy"
                      class="w-full h-full object-cover pointer-events-none"
                    />
                  {:else}
                    <div class="w-full h-full flex items-center justify-center text-xl text-[var(--text-muted)]">👤</div>
                  {/if}
                </div>

                <!-- Editable Name Input -->
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
                    class="w-full bg-[var(--bg-surface-elevated)] border border-purple-500 text-xs text-center text-[var(--text-main)] rounded-lg px-2 py-1 outline-none shadow-sm"
                  />
                {:else}
                  <div class="flex items-center justify-center gap-1.5 w-full px-1">
                    <span class="text-xs font-semibold text-[var(--text-main)] truncate max-w-[95px] sm:max-w-[115px]">
                      {p.name || 'Unnamed'}
                    </span>
                    <button
                      type="button"
                      on:click|stopPropagation={() => { editingId = p.id; editingName = p.name ?? ''; }}
                      class="text-[11px] text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer p-0.5 spring-tap"
                      title="Rename person"
                      aria-label="Rename {p.name || 'Unnamed'}"
                    >
                      ✎
                    </button>
                  </div>
                {/if}

                <!-- Face Count Micro Pill -->
                <span class="text-[10px] text-[var(--text-muted)] mt-1 font-mono font-medium">
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

<!-- Merge Confirmation Dialog Overlay -->
{#if source && target}
  <div class="fixed inset-0 bg-black/80 backdrop-blur-md flex items-center justify-center p-4 z-[60]">
    <div class="glass-panel rounded-3xl p-6 max-w-xs w-full space-y-3.5 text-center shadow-2xl border border-[var(--border-glass)] bg-[var(--bg-surface-elevated)]">
      <div class="w-10 h-10 rounded-2xl bg-purple-600/10 text-purple-400 flex items-center justify-center text-lg mx-auto shadow-inner">
        🔀
      </div>

      <h3 class="text-sm font-bold text-[var(--text-main)]">Merge Identities?</h3>
      <p class="text-xs text-[var(--text-muted)] leading-relaxed">
        Merge <strong class="text-[var(--text-main)]">{source.name || 'Unnamed'}</strong> into <strong class="text-[var(--text-main)]">{target.name || 'Unnamed'}</strong>?
      </p>
      <p class="text-[10px] text-[var(--text-muted)] opacity-80">
        All photo associations will be re-assigned.
      </p>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          on:click={() => { source = null; target = null; }}
          class="px-3.5 py-1.5 rounded-xl text-xs font-medium text-[var(--text-muted)] hover:text-[var(--text-main)] glass-panel transition-all spring-tap cursor-pointer"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={confirmMerge}
          disabled={isMerging}
          class="px-4 py-1.5 bg-purple-600 hover:bg-purple-500 text-white rounded-xl text-xs font-semibold shadow-md shadow-purple-600/25 transition-all spring-tap cursor-pointer disabled:opacity-50"
        >
          {isMerging ? 'Merging...' : 'Merge'}
        </button>
      </div>
    </div>
  </div>
{/if}