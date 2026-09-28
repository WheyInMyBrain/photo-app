<!-- photo-app/frontend/src/lib/components/PeopleModal.svelte -->
<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { browser } from '$app/environment';
  import { filterStore } from '$lib/stores/filterStore';
  import { authStore } from '$lib/stores/authStore';
  import {
    fetchPeopleOverview,
    fetchNamesDirectory,
    renamePerson,
    mergePersons,
    deletePerson,
    type PersonCard,
    type NameDirectoryItem
  } from '$lib/api/people';

  export let isOpen = false;

  const dispatch = createEventDispatcher<{
    close: void;
    selectPerson: { personId: string };
  }>();

  let people: PersonCard[] = [];
  let nameDirectory: NameDirectoryItem[] = [];
  let isLoading = true;
  let editingId: string | null = null;
  let editingName = '';

  // Drag & drop merge state
  let source: PersonCard | null = null;
  let target: PersonCard | null = null;
  let isMerging = false;

  // Delete person state
  let personPendingDelete: PersonCard | null = null;
  let isDeleting = false;

  async function loadPeople() {
    if (!browser || !$authStore.isAuthenticated) return;
    isLoading = true;
    try {
      const [peopleData, namesData] = await Promise.all([
        fetchPeopleOverview(),
        fetchNamesDirectory()
      ]);
      people = peopleData;
      nameDirectory = namesData;
    } catch (e) {
      console.error('Failed loading people:', e);
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
        await mergePersons(person.id, matched.id);
        await loadPeople();
      } catch (err) {
        console.error('Auto-merge failed:', err);
      }
    } else {
      try {
        await renamePerson(person.id, clean);
        person.name = clean;
        people = [...people];
        if (!nameDirectory.some((n) => n.id === person.id)) {
          nameDirectory = [...nameDirectory, { id: person.id, name: clean }];
        }
      } catch (err) {
        console.error('Rename failed:', err);
      }
    }
  }

  async function confirmMerge() {
    if (!source || !target) return;
    isMerging = true;
    try {
      await mergePersons(source.id, target.id);
      source = null;
      target = null;
      await loadPeople();
    } catch (err) {
      console.error('Merge failed:', err);
    } finally {
      isMerging = false;
    }
  }

  async function confirmDeletePerson() {
    if (!personPendingDelete) return;
    isDeleting = true;
    try {
      await deletePerson(personPendingDelete.id);
      people = people.filter((p) => p.id !== personPendingDelete?.id);
      nameDirectory = nameDirectory.filter((n) => n.id !== personPendingDelete?.id);
      personPendingDelete = null;
    } catch (err) {
      console.error('Delete person failed:', err);
    } finally {
      isDeleting = false;
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
      if (personPendingDelete) {
        personPendingDelete = null;
      } else if (source || target) {
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
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 bg-black/60 backdrop-blur-md flex items-center justify-center p-3 sm:p-6 select-none"
    on:click|self={() => dispatch('close')}
  >
    <!-- Liquid-Glass Modal Card -->
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Manage People"
      class="liquid-modal rounded-3xl w-full max-w-4xl max-h-[85vh] flex flex-col overflow-hidden"
    >
      <!-- Specular Header -->
      <div class="px-6 py-4.5 border-b border-white/[0.08] flex items-center justify-between">
        <div>
          <h2 class="text-sm sm:text-base font-semibold tracking-tight text-white flex items-center gap-2">
            <span>People & Faces</span>
          </h2>
          <p class="text-[11px] text-white/45 mt-0.5 tracking-tight font-normal">
            Select to filter. Drag a card onto another to merge. Tap delete on unwanted background faces.
          </p>
        </div>

        <div class="flex items-center gap-3">
          <span class="text-[11px] text-white/40 font-mono hidden sm:inline">
            {people.length} {people.length === 1 ? 'identity' : 'identities'}
          </span>
          <button
            type="button"
            on:click={() => dispatch('close')}
            class="liquid-icon-btn w-7 h-7 flex items-center justify-center rounded-full text-white/50 hover:text-white transition-all spring-tap cursor-pointer"
            title="Close"
            aria-label="Close dialog"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
              <line x1="18" y1="6" x2="6" y2="18"></line>
              <line x1="6" y1="6" x2="18" y2="18"></line>
            </svg>
          </button>
        </div>
      </div>

      <!-- Identity Grid Area -->
      <div class="p-4 sm:p-6 overflow-y-auto flex-1 no-scrollbar">
        {#if isLoading}
          <div class="flex flex-col items-center justify-center py-20 gap-3 text-white/40">
            <div class="w-6 h-6 border-2 border-white/20 border-t-white rounded-full animate-spin"></div>
            <span class="text-xs font-mono tracking-tight">Scanning embeddings...</span>
          </div>
        {:else if people.length === 0}
          <div class="text-xs text-white/40 text-center py-20 font-normal">
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
                class="liquid-card group relative rounded-2xl p-3.5 flex flex-col items-center text-center cursor-pointer transition-all spring-tap {isFiltered ? 'liquid-card-selected' : ''}"
              >
                <!-- Card Actions: Dismiss / Delete Button (Top Right) -->
                <button
                  type="button"
                  on:click|stopPropagation={() => (personPendingDelete = p)}
                  class="absolute top-2 right-2 w-5 h-5 rounded-full liquid-delete-btn flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity text-white/40 hover:text-rose-300"
                  title="Remove this identity"
                  aria-label="Remove identity {p.name || 'Unnamed'}"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                    <line x1="18" y1="6" x2="6" y2="18"></line>
                    <line x1="6" y1="6" x2="18" y2="18"></line>
                  </svg>
                </button>

                <!-- Avatar Circle -->
                <div class="w-16 h-16 sm:w-20 sm:h-20 rounded-full overflow-hidden liquid-avatar-frame mb-2.5 flex items-center justify-center">
                  {#if p.avatar_thumb}
                    <img
                      src={p.avatar_thumb.startsWith('/') ? p.avatar_thumb : `/${p.avatar_thumb}`}
                      alt={p.name ?? 'Person'}
                      loading="lazy"
                      class="w-full h-full object-cover pointer-events-none"
                    />
                  {:else}
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-7 h-7 text-white/30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                      <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"></path>
                      <circle cx="12" cy="7" r="4"></circle>
                    </svg>
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
                    class="liquid-input w-full text-xs text-center text-white rounded-lg px-2 py-1 outline-none"
                  />
                {:else}
                  <div class="flex items-center justify-center gap-1.5 w-full px-1">
                    <span class="text-xs font-medium text-white/90 truncate max-w-[100px] sm:max-w-[120px]">
                      {p.name || 'Unnamed'}
                    </span>
                    <button
                      type="button"
                      on:click|stopPropagation={() => { editingId = p.id; editingName = p.name ?? ''; }}
                      class="text-white/40 hover:text-white cursor-pointer p-0.5 transition-colors spring-tap"
                      title="Rename"
                      aria-label="Rename {p.name || 'Unnamed'}"
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M12 20h9"></path>
                        <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"></path>
                      </svg>
                    </button>
                  </div>
                {/if}

                <!-- Face Count Label -->
                <span class="text-[10px] text-white/40 mt-1 font-mono">
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

<!-- Merge Confirmation Overlay -->
{#if source && target}
  <div class="fixed inset-0 bg-black/70 backdrop-blur-md flex items-center justify-center p-4 z-[60]">
    <div class="liquid-modal rounded-3xl p-6 max-w-xs w-full space-y-3.5 text-center">
      <div class="w-10 h-10 rounded-full liquid-icon-plate flex items-center justify-center text-white/80 mx-auto">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="16 3 21 3 21 8"></polyline>
          <line x1="4" y1="20" x2="21" y2="3"></line>
          <polyline points="21 16 21 21 16 21"></polyline>
          <line x1="15" y1="15" x2="21" y2="21"></line>
          <line x1="4" y1="4" x2="9" y2="9"></line>
        </svg>
      </div>

      <h3 class="text-sm font-semibold tracking-tight text-white">Merge Identities?</h3>
      <p class="text-xs text-white/60 leading-relaxed">
        Merge <span class="text-white font-medium">{source.name || 'Unnamed'}</span> into <span class="text-white font-medium">{target.name || 'Unnamed'}</span>?
      </p>
      <p class="text-[10px] text-white/40 font-mono">
        All photo associations will be re-assigned.
      </p>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          on:click={() => { source = null; target = null; }}
          class="liquid-btn-secondary px-3.5 py-1.5 rounded-xl text-xs text-white/60 hover:text-white transition-all spring-tap cursor-pointer"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={confirmMerge}
          disabled={isMerging}
          class="liquid-btn-primary px-4 py-1.5 rounded-xl text-xs font-medium text-white transition-all spring-tap cursor-pointer disabled:opacity-40"
        >
          {isMerging ? 'Merging...' : 'Merge'}
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Delete Confirmation Overlay -->
{#if personPendingDelete}
  <div class="fixed inset-0 bg-black/70 backdrop-blur-md flex items-center justify-center p-4 z-[60]">
    <div class="liquid-modal rounded-3xl p-6 max-w-xs w-full space-y-3.5 text-center">
      <div class="w-10 h-10 rounded-full liquid-icon-plate flex items-center justify-center text-rose-300 mx-auto">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"></polyline>
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
        </svg>
      </div>

      <h3 class="text-sm font-semibold tracking-tight text-white">Remove Identity?</h3>
      <p class="text-xs text-white/60 leading-relaxed">
        Remove <span class="text-white font-medium">{personPendingDelete.name || 'Unnamed Person'}</span>?
      </p>
      <p class="text-[10px] text-white/40 font-mono">
        Original photos will NOT be deleted. Only this identity group and face bindings are removed.
      </p>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          on:click={() => (personPendingDelete = null)}
          class="liquid-btn-secondary px-3.5 py-1.5 rounded-xl text-xs text-white/60 hover:text-white transition-all spring-tap cursor-pointer"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={confirmDeletePerson}
          disabled={isDeleting}
          class="liquid-btn-danger px-4 py-1.5 rounded-xl text-xs font-medium text-rose-200 transition-all spring-tap cursor-pointer disabled:opacity-40"
        >
          {isDeleting ? 'Removing...' : 'Remove'}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Apple Liquid-Glass Framework */
  .liquid-modal {
    background: rgba(18, 18, 22, 0.76);
    border: 1px solid rgba(255, 255, 255, 0.13);
    backdrop-filter: blur(40px) saturate(180%);
    -webkit-backdrop-filter: blur(40px) saturate(180%);
    box-shadow:
      0 30px 70px rgba(0, 0, 0, 0.75),
      inset 0 1px 0 0 rgba(255, 255, 255, 0.22),
      inset 0 -1px 0 0 rgba(0, 0, 0, 0.4);
  }

  .liquid-card {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow: inset 0 1px 0 0 rgba(255, 255, 255, 0.12);
  }

  .liquid-card:hover {
    background: rgba(255, 255, 255, 0.07);
    border-color: rgba(255, 255, 255, 0.16);
  }

  .liquid-card-selected {
    background: rgba(255, 255, 255, 0.12);
    border-color: rgba(255, 255, 255, 0.28);
    box-shadow:
      0 4px 16px rgba(0, 0, 0, 0.4),
      inset 0 1px 0 rgba(255, 255, 255, 0.35);
  }

  .liquid-delete-btn {
    background: rgba(0, 0, 0, 0.5);
    border: 1px solid rgba(255, 255, 255, 0.12);
  }

  .liquid-delete-btn:hover {
    background: rgba(244, 63, 94, 0.25);
    border-color: rgba(244, 63, 94, 0.4);
  }

  .liquid-avatar-frame {
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.15);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.5);
  }

  .liquid-input {
    background: rgba(0, 0, 0, 0.45);
    border: 1px solid rgba(255, 255, 255, 0.22);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.4);
  }

  .liquid-icon-btn {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.12);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.2);
  }

  .liquid-icon-btn:hover {
    background: rgba(255, 255, 255, 0.12);
    border-color: rgba(255, 255, 255, 0.2);
  }

  .liquid-icon-plate {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.14);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.25);
  }

  .liquid-btn-primary {
    background: rgba(255, 255, 255, 0.16);
    border: 1px solid rgba(255, 255, 255, 0.24);
    box-shadow:
      0 4px 14px rgba(0, 0, 0, 0.3),
      inset 0 1px 0 rgba(255, 255, 255, 0.35);
  }

  .liquid-btn-primary:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.22);
    border-color: rgba(255, 255, 255, 0.32);
    box-shadow:
      0 6px 18px rgba(0, 0, 0, 0.4),
      inset 0 1px 0 rgba(255, 255, 255, 0.45);
  }

  .liquid-btn-danger {
    background: rgba(244, 63, 94, 0.2);
    border: 1px solid rgba(244, 63, 94, 0.35);
    box-shadow:
      0 4px 14px rgba(0, 0, 0, 0.3),
      inset 0 1px 0 rgba(255, 255, 255, 0.2);
  }

  .liquid-btn-danger:hover:not(:disabled) {
    background: rgba(244, 63, 94, 0.32);
    border-color: rgba(244, 63, 94, 0.5);
  }

  .liquid-btn-secondary {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .liquid-btn-secondary:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.14);
  }
</style>