<!-- photo-app/frontend/src/lib/components/PeopleModal.svelte -->
<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { browser } from '$app/environment';
  import { fade, scale } from 'svelte/transition';
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

  // Multi-selection state
  let selectedIds: Set<string> = new Set();

  // Drag & drop merge state
  let source: PersonCard | null = null;
  let target: PersonCard | null = null;
  let isMerging = false;

  // Multi-merge modal state
  let showMultiMergeModal = false;
  let multiMergeTargetId = '';

  // Delete modal state
  let showBatchDeleteConfirm = false;
  let isDeleting = false;

  $: selectedPeople = people.filter((p) => selectedIds.has(p.id));$: canMergeSelected = selectedIds.size >= 2;
  $: canDeleteSelected = selectedIds.size > 0;

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

  function toggleSelection(id: string) {
    if (selectedIds.has(id)) {
      selectedIds.delete(id);
    } else {
      selectedIds.add(id);
    }
    selectedIds = new Set(selectedIds);
  }

  function clearSelection() {
    selectedIds = new Set();
  }

  function selectAll() {
    selectedIds = new Set(people.map((p) => p.id));
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

  async function confirmSingleMerge() {
    if (!source || !target) return;
    isMerging = true;
    try {
      await mergePersons(source.id, target.id);
      source = null;
      target = null;
      clearSelection();
      await loadPeople();
    } catch (err) {
      console.error('Merge failed:', err);
    } finally {
      isMerging = false;
    }
  }

  function openMultiMergeModal() {
    if (selectedPeople.length < 2) return;
    const namedPerson = selectedPeople.find((p) => Boolean(p.name));
    multiMergeTargetId = namedPerson ? namedPerson.id : selectedPeople[0].id;
    showMultiMergeModal = true;
  }

  async function executeMultiMerge() {
    if (!multiMergeTargetId || selectedPeople.length < 2) return;
    isMerging = true;
    try {
      const sourcesToMerge = selectedPeople.filter((p) => p.id !== multiMergeTargetId);
      for (const s of sourcesToMerge) {
        await mergePersons(s.id, multiMergeTargetId);
      }
      showMultiMergeModal = false;
      clearSelection();
      await loadPeople();
    } catch (err) {
      console.error('Multi-merge failed:', err);
    } finally {
      isMerging = false;
    }
  }

  async function executeBatchDelete() {
    if (selectedIds.size === 0) return;
    isDeleting = true;
    try {
      const idsToDelete = Array.from(selectedIds);
      for (const id of idsToDelete) {
        await deletePerson(id);
      }
      people = people.filter((p) => !selectedIds.has(p.id));
      nameDirectory = nameDirectory.filter((n) => !selectedIds.has(n.id));
      showBatchDeleteConfirm = false;
      clearSelection();
    } catch (err) {
      console.error('Batch delete failed:', err);
    } finally {
      isDeleting = false;
    }
  }

  function handleCardClick(person: PersonCard) {
    if (selectedIds.size > 0) {
      toggleSelection(person.id);
      return;
    }
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
      if (showBatchDeleteConfirm) {
        showBatchDeleteConfirm = false;
      } else if (showMultiMergeModal) {
        showMultiMergeModal = false;
      } else if (source || target) {
        source = null;
        target = null;
      } else if (selectedIds.size > 0) {
        clearSelection();
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
  <div
    transition:fade={{ duration: 150 }}
    class="fixed inset-0 z-50 bg-black/60 backdrop-blur-md flex items-center justify-center p-3 sm:p-6 select-none"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    on:click|self={() => dispatch('close')}
  >
    <!-- Liquid-Glass Modal Card -->
    <div
      transition:scale={{ start: 0.96, duration: 150 }}
      class="liquid-modal rounded-3xl w-full max-w-4xl max-h-[85vh] flex flex-col overflow-hidden text-[var(--text-main)]"
    >
      <!-- Specular Header -->
      <div class="px-6 py-4 border-b border-[var(--border-glass)] flex items-center justify-between gap-3">
        <div>
          <h2 class="text-sm sm:text-base font-semibold tracking-tight text-[var(--text-main)] flex items-center gap-2">
            <span>People & Faces</span>
          </h2>
          <p class="text-[11px] text-[var(--text-muted)] mt-0.5 tracking-tight">
            Click to filter or select. Drag onto another person or use checkboxes to bulk merge and delete.
          </p>
        </div>

        <div class="flex items-center gap-2.5">
          <span class="text-[11px] text-[var(--text-muted)] font-mono hidden sm:inline">
            {people.length} {people.length === 1 ? 'person' : 'people'}
          </span>
          <button
            type="button"
            on:click={() => dispatch('close')}
            class="liquid-icon-btn w-7 h-7 flex items-center justify-center rounded-full text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
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

      <!-- Bulk Action Bar -->
      {#if selectedIds.size > 0}
        <div class="px-6 py-2.5 bg-purple-500/10 border-b border-purple-500/20 flex flex-wrap items-center justify-between gap-2.5 transition-all">
          <div class="flex items-center gap-2.5">
            <span class="w-2 h-2 rounded-full bg-purple-500 animate-pulse"></span>
            <span class="text-xs font-semibold text-[var(--text-main)]">
              {selectedIds.size} selected
            </span>
            <button
              type="button"
              on:click={selectAll}
              class="text-[11px] text-purple-600 dark:text-purple-400 hover:underline cursor-pointer font-medium"
            >
              Select All ({people.length})
            </button>
            <span class="text-[var(--border-glass)]">•</span>
            <button
              type="button"
              on:click={clearSelection}
              class="text-[11px] text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer"
            >
              Deselect
            </button>
          </div>

          <div class="flex items-center gap-2">
            <!-- Merge Button -->
            <button
              type="button"
              disabled={!canMergeSelected}
              on:click={openMultiMergeModal}
              class="px-3 py-1.5 rounded-xl text-xs font-semibold flex items-center gap-1.5 transition-all spring-tap cursor-pointer disabled:opacity-40 disabled:pointer-events-none bg-purple-600 text-white shadow-sm hover:bg-purple-500"
              title={canMergeSelected ? 'Merge selected identities' : 'Select at least 2 people to merge'}
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                <polyline points="16 3 21 3 21 8"></polyline>
                <line x1="4" y1="20" x2="21" y2="3"></line>
                <polyline points="21 16 21 21 16 21"></polyline>
                <line x1="15" y1="15" x2="21" y2="21"></line>
                <line x1="4" y1="4" x2="9" y2="9"></line>
              </svg>
              <span>Merge ({selectedIds.size})</span>
            </button>

            <!-- Delete Button -->
            <button
              type="button"
              disabled={!canDeleteSelected}
              on:click={() => (showBatchDeleteConfirm = true)}
              class="px-3 py-1.5 rounded-xl text-xs font-semibold flex items-center gap-1.5 transition-all spring-tap cursor-pointer disabled:opacity-40 disabled:pointer-events-none bg-rose-500/15 border border-rose-500/30 text-rose-600 dark:text-rose-300 hover:bg-rose-500/25"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                <polyline points="3 6 5 6 21 6"></polyline>
                <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
              </svg>
              <span>Delete</span>
            </button>
          </div>
        </div>
      {/if}

      <!-- Identity Grid Area -->
      <div class="p-4 sm:p-6 overflow-y-auto flex-1 no-scrollbar">
        {#if isLoading}
          <div class="flex flex-col items-center justify-center py-20 gap-3 text-[var(--text-muted)]">
            <div class="w-6 h-6 border-2 border-purple-500/20 border-t-purple-500 rounded-full animate-spin"></div>
            <span class="text-xs font-mono tracking-tight">Scanning embeddings...</span>
          </div>
        {:else if people.length === 0}
          <div class="text-xs text-[var(--text-muted)] text-center py-20 font-normal">
            No identified faces found in your library yet.
          </div>
        {:else}
          <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
            {#each people as p (p.id)}
              {@const isSelected = selectedIds.has(p.id)}
              {@const isFiltered = $filterStore.person_ids.has(p.id)}

              <div
                role="button"
                tabindex="0"
                draggable="true"
                on:dragstart={(e) => { source = p; e.dataTransfer?.setData('text/plain', p.id); }}
                on:dragover|preventDefault
                on:drop={() => { if (source && source.id !== p.id) target = p; }}
                on:click={() => handleCardClick(p)}
                on:keydown={(e) => e.key === 'Enter' && handleCardClick(p)}
                class="liquid-card group relative rounded-2xl p-3.5 flex flex-col items-center text-center cursor-pointer transition-all spring-tap {isSelected ? 'ring-2 ring-purple-500 shadow-md' : isFiltered ? 'liquid-card-filtered' : ''}"
              >
                <!-- Selection Checkbox Button (Top Right) -->
                <button
                  type="button"
                  on:click|stopPropagation={() => toggleSelection(p.id)}
                  class="absolute top-2 right-2 w-5 h-5 rounded-full flex items-center justify-center transition-all cursor-pointer z-10 {isSelected ? 'bg-purple-600 text-white shadow-sm' : 'bg-black/35 backdrop-blur-md opacity-0 group-hover:opacity-100 text-white/80 hover:text-white border border-white/20'}"
                  title={isSelected ? 'Deselect' : 'Select for merge or delete'}
                  aria-label="Toggle selection"
                >
                  {#if isSelected}
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 text-white pointer-events-none" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
                      <polyline points="20 6 9 17 4 12"></polyline>
                    </svg>
                  {:else}
                    <div class="w-1.5 h-1.5 rounded-full bg-white/60"></div>
                  {/if}
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
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-7 h-7 text-[var(--text-muted)] opacity-50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
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
                    class="liquid-input w-full text-xs text-center text-[var(--text-main)] rounded-lg px-2 py-1 outline-none"
                  />
                {:else}
                  <div class="flex items-center justify-center gap-1.5 w-full px-1">
                    <span class="text-xs font-semibold text-[var(--text-main)] truncate max-w-[100px] sm:max-w-[120px]">
                      {p.name || 'Unnamed'}
                    </span>
                    <button
                      type="button"
                      on:click|stopPropagation={() => { editingId = p.id; editingName = p.name ?? ''; }}
                      class="text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer p-0.5 transition-colors spring-tap"
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
                <span class="text-[10px] text-[var(--text-muted)] mt-1 font-mono">
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

<!-- Drag & Drop Single Merge Overlay -->
{#if source && target}
  <div class="fixed inset-0 bg-black/70 backdrop-blur-md flex items-center justify-center p-4 z-[60]">
    <div class="liquid-modal rounded-3xl p-6 max-w-xs w-full space-y-3.5 text-center text-[var(--text-main)]">
      <div class="w-10 h-10 rounded-full bg-purple-500/15 border border-purple-500/25 flex items-center justify-center text-purple-600 dark:text-purple-300 mx-auto">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="16 3 21 3 21 8"></polyline>
          <line x1="4" y1="20" x2="21" y2="3"></line>
          <polyline points="21 16 21 21 16 21"></polyline>
          <line x1="15" y1="15" x2="21" y2="21"></line>
          <line x1="4" y1="4" x2="9" y2="9"></line>
        </svg>
      </div>

      <h3 class="text-sm font-semibold tracking-tight text-[var(--text-main)]">Merge Identities?</h3>
      <p class="text-xs text-[var(--text-muted)] leading-relaxed">
        Merge <span class="text-[var(--text-main)] font-semibold">{source.name || 'Unnamed'}</span> into <span class="text-[var(--text-main)] font-semibold">{target.name || 'Unnamed'}</span>?
      </p>
      <p class="text-[10px] text-[var(--text-muted)] opacity-70 font-mono">
        All photo associations will be re-assigned.
      </p>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          on:click={() => { source = null; target = null; }}
          class="liquid-btn-secondary px-3.5 py-1.5 rounded-xl text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={confirmSingleMerge}
          disabled={isMerging}
          class="px-4 py-1.5 rounded-xl text-xs font-semibold text-white bg-purple-600 hover:bg-purple-500 transition-all spring-tap cursor-pointer disabled:opacity-40"
        >
          {isMerging ? 'Merging...' : 'Merge'}
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Multi-Select Merge Modal -->
{#if showMultiMergeModal}
  <div class="fixed inset-0 bg-black/70 backdrop-blur-md flex items-center justify-center p-4 z-[60]">
    <div class="liquid-modal rounded-3xl p-6 max-w-sm w-full space-y-4 text-[var(--text-main)]">
      <div class="w-10 h-10 rounded-full bg-purple-500/15 border border-purple-500/25 flex items-center justify-center text-purple-600 dark:text-purple-300 mx-auto">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="16 3 21 3 21 8"></polyline>
          <line x1="4" y1="20" x2="21" y2="3"></line>
          <polyline points="21 16 21 21 16 21"></polyline>
          <line x1="15" y1="15" x2="21" y2="21"></line>
          <line x1="4" y1="4" x2="9" y2="9"></line>
        </svg>
      </div>

      <div class="text-center space-y-1">
        <h3 class="text-sm font-semibold tracking-tight text-[var(--text-main)]">Merge {selectedPeople.length} People</h3>
        <p class="text-xs text-[var(--text-muted)]">
          Select the primary identity to keep. All other faces will be merged into this name.
        </p>
      </div>

      <div class="space-y-1.5 max-h-48 overflow-y-auto no-scrollbar pr-0.5">
        {#each selectedPeople as p (p.id)}
          <label class="liquid-card flex items-center gap-3 p-2.5 rounded-xl cursor-pointer hover:bg-[var(--dock-bg-hover)] transition-colors {multiMergeTargetId === p.id ? 'ring-1.5 ring-purple-500' : ''}">
            <input
              type="radio"
              name="merge-target"
              value={p.id}
              bind:group={multiMergeTargetId}
              class="accent-purple-600"
            />
            <div class="w-7 h-7 rounded-full overflow-hidden bg-black/20 flex-shrink-0">
              {#if p.avatar_thumb}
                <img src={p.avatar_thumb.startsWith('/') ? p.avatar_thumb : `/${p.avatar_thumb}`} alt="" class="w-full h-full object-cover" />
              {/if}
            </div>
            <div class="flex-1 min-w-0">
              <div class="text-xs font-semibold truncate text-[var(--text-main)]">{p.name || 'Unnamed Person'}</div>
              <div class="text-[10px] text-[var(--text-muted)] font-mono">{p.face_count} photos</div>
            </div>
          </label>
        {/each}
      </div>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          on:click={() => (showMultiMergeModal = false)}
          class="liquid-btn-secondary px-3.5 py-1.5 rounded-xl text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={executeMultiMerge}
          disabled={isMerging}
          class="px-4 py-1.5 rounded-xl text-xs font-semibold text-white bg-purple-600 hover:bg-purple-500 transition-all spring-tap cursor-pointer disabled:opacity-40"
        >
          {isMerging ? 'Merging...' : 'Merge All'}
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Batch Delete Confirmation Modal -->
{#if showBatchDeleteConfirm}
  <div class="fixed inset-0 bg-black/70 backdrop-blur-md flex items-center justify-center p-4 z-[60]">
    <div class="liquid-modal rounded-3xl p-6 max-w-xs w-full space-y-3.5 text-center text-[var(--text-main)]">
      <div class="w-10 h-10 rounded-full bg-rose-500/10 border border-rose-500/20 text-rose-500 flex items-center justify-center mx-auto">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"></polyline>
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
        </svg>
      </div>

      <h3 class="text-sm font-semibold tracking-tight text-[var(--text-main)]">
        Remove {selectedIds.size} {selectedIds.size === 1 ? 'Identity' : 'Identities'}?
      </h3>
      <p class="text-xs text-[var(--text-muted)] leading-relaxed">
        Original photos will NOT be deleted. Only identity groups and face bindings will be dismissed.
      </p>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          on:click={() => (showBatchDeleteConfirm = false)}
          class="liquid-btn-secondary px-3.5 py-1.5 rounded-xl text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={executeBatchDelete}
          disabled={isDeleting}
          class="px-4 py-1.5 rounded-xl text-xs font-semibold text-rose-200 bg-rose-600 hover:bg-rose-500 transition-all spring-tap cursor-pointer disabled:opacity-40"
        >
          {isDeleting ? 'Removing...' : 'Remove'}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .liquid-modal {
    background: var(--bg-surface-elevated);
    border: 1px solid var(--border-glass);
    backdrop-filter: blur(40px) saturate(190%);
    -webkit-backdrop-filter: blur(40px) saturate(190%);
    box-shadow:
      0 30px 70px var(--dock-shadow),
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

  .liquid-card-filtered {
    background: var(--pill-bg);
    border-color: var(--border-subtle);
  }

  .liquid-avatar-frame {
    background: var(--card-bg);
    border: 1px solid var(--border-glass);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.2);
  }

  .liquid-input {
    background: var(--pill-bg);
    border: 1px solid var(--border-subtle);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.08);
  }

  .liquid-icon-btn {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
  }

  .liquid-icon-btn:hover {
    background: var(--dock-bg-hover);
    border-color: var(--border-subtle);
  }

  .liquid-btn-secondary {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
  }

  .liquid-btn-secondary:hover:not(:disabled) {
    background: var(--dock-bg-hover);
    border-color: var(--border-subtle);
  }
</style>