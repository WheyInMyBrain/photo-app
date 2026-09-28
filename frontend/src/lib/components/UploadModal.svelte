<!-- photo-app/frontend/src/lib/components/UploadModal.svelte -->
<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import { fetchCustomAlbums } from '$lib/api/albums';
  import { uploadQueue } from '$lib/stores/uploadQueueStore';
  import { uploadProgressStore } from '$lib/stores/uploadProgressStore';
  import { formatBytes } from '$lib/utils/uploader';
  import { normalizePath, getDirectChildren, type FolderNode } from '$lib/utils/folderHierarchy';
  import type { CandidateItem } from '$lib/types/upload';

  export let isOpen = false;
  export let initialFiles: File[] = [];

  const dispatch = createEventDispatcher<{
    close: void;
    uploaded: { count: number };
  }>();

  let rawAlbumPaths: string[] = [];
  let isFolderDropdownOpen = false;
  let fileInputEl: HTMLInputElement;

  // Active directory level inside the folder tree browser (e.g. "" for root, "College" for College/)
  let browsingDir = '';
  let searchQuery = '';

  $: if (initialFiles && initialFiles.length > 0) {
    uploadQueue.setMode('files');
    uploadQueue.addFiles(initialFiles);
    initialFiles = [];
  }

  $: canUpload =
    $uploadQueue.uploadMode === 'files'
      ? $uploadQueue.stagedFiles.length > 0
      : $uploadQueue.linkPreview !== null && $uploadQueue.selectedLinkItems.size > 0;

  // Normalized folder path
  $: cleanInputPath = normalizePath($uploadQueue.folderPath);

  // Fetch children under the folder we are currently browsing
  $: currentChildren = getDirectChildren(rawAlbumPaths, browsingDir);

  // Filter children by live search query if typing
  $: visibleNodes = searchQuery.trim()
    ? currentChildren.filter((node) =>
        node.name.toLowerCase().includes(searchQuery.trim().toLowerCase())
      )
    : currentChildren;

  // Split browsingDir into clickable breadcrumb steps
  $: browsingCrumbs = (() => {
    if (!browsingDir) return [];
    const parts = browsingDir.split('/');
    let cumulative = '';
    return parts.map((part) => {
      cumulative = cumulative ? `${cumulative}/${part}` : part;
      return { name: part, path: cumulative };
    });
  })();

  onMount(async () => {
    try {
      const albums = await fetchCustomAlbums();
      if (Array.isArray(albums)) {
        rawAlbumPaths = albums
          .map((a: any) => a.title || a.name || a.label || a.value)
          .filter(Boolean);
      }
    } catch {
      rawAlbumPaths = [];
    }
  });

  onMount(() => {
    const originalOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = originalOverflow;
    };
  });

  function handleFileSelect(e: Event) {
    const input = e.target as HTMLInputElement;
    if (input.files && input.files.length > 0) {
      const selected = Array.from(input.files);
      setTimeout(() => {
        uploadQueue.addFiles(selected);
        input.value = '';
      }, 50);
    }
  }

  function getThumbnailSrc(item: CandidateItem): string {
    if (item.thumbnail_base64?.trim()) {
      return item.thumbnail_base64.startsWith('data:')
        ? item.thumbnail_base64
        : `data:image/jpeg;base64,${item.thumbnail_base64}`;
    }
    return item.thumbnail_url || '';
  }

  function drillDown(node: FolderNode) {
    browsingDir = node.fullPath;
    searchQuery = '';
  }

  function jumpToDirectory(path: string) {
    browsingDir = path;
    searchQuery = '';
  }

  function selectDirectory(path: string) {
    uploadQueue.setFolderPath(path);
    isFolderDropdownOpen = false;
    searchQuery = '';
  }

  function createSubFolder() {
    const newName = searchQuery.trim();
    if (!newName) return;
    const newFullPath = browsingDir ? `${browsingDir}/${newName}` : newName;
    selectDirectory(newFullPath);
  }

  // Non-blocking handoff: Closes modal instantly and starts floating widget
  function handleStartUpload() {
    const state = $uploadQueue;
    if (state.uploadMode === 'files' && state.stagedFiles.length > 0) {
      const filesToUpload = [...state.stagedFiles];
      const targetFolder = cleanInputPath;

      // Close modal immediately
      forceClose();

      // Launch floating bottom-right persistent progress widget
      uploadProgressStore.startBatch(filesToUpload, targetFolder, () => {
        dispatch('uploaded', { count: filesToUpload.length });
      });
    } else {
      // Web link import path
      uploadQueue.executeUpload((count) => {
        dispatch('uploaded', { count });
        forceClose();
      });
    }
  }

  function forceClose() {
    uploadQueue.reset();
    isFolderDropdownOpen = false;
    browsingDir = '';
    searchQuery = '';
    dispatch('close');
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') forceClose();
  }
</script>

<svelte:window on:keydown={handleKeydown} />

{#if isOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    transition:fade={{ duration: 150 }}
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md p-4 sm:p-6 select-none"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    on:click|self={forceClose}
  >
    <div
      transition:scale={{ start: 0.96, duration: 150 }}
      class="liquid-modal rounded-3xl w-full max-w-lg flex flex-col max-h-[85vh] overflow-visible text-[var(--text-main)]"
    >
      <!-- Header -->
      <div class="px-6 py-4.5 border-b border-[var(--border-glass)] flex items-center justify-between">
        <div>
          <h2 class="text-sm font-semibold tracking-tight text-[var(--text-main)] flex items-center gap-2">
            <span>Add Media</span>
          </h2>
          <p class="text-[11px] text-[var(--text-muted)] mt-0.5 font-normal tracking-tight">
            Original files preserved with full metadata.
          </p>
        </div>
        <button
          type="button"
          on:click={forceClose}
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

      <!-- Mode Switcher -->
      <div class="px-6 pt-4">
        <div class="liquid-segmented p-0.5 rounded-xl flex text-xs">
          <button
            class="flex-1 py-1.5 rounded-lg transition-all font-medium spring-tap cursor-pointer {$uploadQueue.uploadMode === 'files' ? 'liquid-seg-active text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            on:click={() => uploadQueue.setMode('files')}
          >
            Files
          </button>
          <button
            class="flex-1 py-1.5 rounded-lg transition-all font-medium spring-tap cursor-pointer {$uploadQueue.uploadMode === 'link' ? 'liquid-seg-active text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            on:click={() => uploadQueue.setMode('link')}
          >
            Web Link
          </button>
        </div>
      </div>

      <!-- Form Body -->
      <div class="p-6 space-y-4 overflow-y-auto flex-1 no-scrollbar">
        <!-- Interactive Tree Album & Folder Selector -->
        <div class="space-y-1.5 relative">
          <div class="flex items-center justify-between px-1">
            <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block">
              Destination Album
            </span>
            {#if $uploadQueue.folderPath}
              <button
                type="button"
                on:click={() => {
                  uploadQueue.setFolderPath('');
                  browsingDir = '';
                }}
                class="text-[10px] text-purple-600 dark:text-purple-400 hover:underline cursor-pointer"
              >
                Clear to root
              </button>
            {/if}
          </div>

          <!-- Current Selection Box with Browse Trigger -->
          <div class="relative">
            <button
              type="button"
              disabled={$uploadQueue.linkPreview !== null}
              on:click={() => (isFolderDropdownOpen = !isFolderDropdownOpen)}
              class="liquid-input w-full rounded-xl px-3.5 py-2 text-xs text-left text-[var(--text-main)] outline-none transition-all flex items-center justify-between gap-2 cursor-pointer disabled:opacity-40"
            >
              <div class="flex items-center gap-2 truncate">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 text-purple-500/80 flex-shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                </svg>
                <span class="truncate font-medium">
                  {$uploadQueue.folderPath ? $uploadQueue.folderPath : 'root (no album)'}
                </span>
              </div>
              <span class="text-[10px] text-[var(--text-muted)] opacity-60">
                {isFolderDropdownOpen ? '▲' : '▼'}
              </span>
            </button>

            <!-- Hierarchical Tree Navigation Drawer -->
            {#if isFolderDropdownOpen}
              <div class="absolute left-0 right-0 top-full mt-1.5 z-50 liquid-dropdown rounded-2xl p-2.5 shadow-2xl max-h-64 overflow-y-auto no-scrollbar border border-[var(--border-glass)] flex flex-col gap-2">
                
                <!-- Tree Mini-Breadcrumb Bar -->
                <div class="flex items-center gap-1 text-[11px] text-[var(--text-muted)] overflow-x-auto no-scrollbar py-1 border-b border-[var(--border-glass)]">
                  <button
                    type="button"
                    on:click={() => jumpToDirectory('')}
                    class="hover:text-[var(--text-main)] font-semibold cursor-pointer whitespace-nowrap {!browsingDir ? 'text-purple-500' : ''}"
                  >
                    root
                  </button>

                  {#each browsingCrumbs as crumb, idx}
                    <span class="opacity-40">/</span>
                    <button
                      type="button"
                      on:click={() => jumpToDirectory(crumb.path)}
                      class="hover:text-[var(--text-main)] truncate max-w-[110px] cursor-pointer whitespace-nowrap {idx === browsingCrumbs.length - 1 ? 'font-semibold text-purple-500' : ''}"
                    >
                      {crumb.name}
                    </button>
                  {/each}
                </div>

                <!-- Fast Filter / New Subfolder Input -->
                <div class="flex gap-1.5">
                  <input
                    type="text"
                    bind:value={searchQuery}
                    placeholder={browsingDir ? `Filter or name new subfolder...` : `Filter or create folder...`}
                    class="liquid-input flex-1 rounded-lg px-2.5 py-1 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] outline-none"
                  />
                  {#if searchQuery.trim()}
                    <button
                      type="button"
                      on:click={createSubFolder}
                      class="px-2.5 py-1 rounded-lg bg-purple-600 hover:bg-purple-500 text-white text-[11px] font-medium transition cursor-pointer flex-shrink-0"
                    >
                      Use
                    </button>
                  {/if}
                </div>

                <!-- "Choose Current Directory" action row -->
                <button
                  type="button"
                  on:click={() => selectDirectory(browsingDir)}
                  class="w-full text-left px-2.5 py-1.5 rounded-lg text-xs bg-purple-500/10 hover:bg-purple-500/20 text-purple-600 dark:text-purple-300 font-medium transition-colors flex items-center justify-between cursor-pointer"
                >
                  <span class="truncate">Select "{browsingDir || 'root'}"</span>
                  <span class="text-[10px] uppercase font-mono">Use Current</span>
                </button>

                <!-- Children Directory List -->
                <div class="space-y-0.5 overflow-y-auto max-h-36 no-scrollbar">
                  {#if visibleNodes.length === 0}
                    <div class="py-3 text-center text-xs text-[var(--text-muted)] italic">
                      No sub-albums here.
                    </div>
                  {:else}
                    {#each visibleNodes as node}
                      <div class="flex items-center justify-between rounded-lg hover:bg-[var(--dock-bg-hover)] px-2 py-1.5 group transition-colors">
                        <button
                          type="button"
                          on:click={() => drillDown(node)}
                          class="flex items-center gap-2 min-w-0 flex-1 text-left cursor-pointer"
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 text-purple-500/70 group-hover:text-purple-500 flex-shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                          </svg>
                          <span class="truncate text-xs text-[var(--text-main)] font-medium">
                            {node.name}
                          </span>
                          {#if node.hasChildren}
                            <span class="text-[9px] text-[var(--text-muted)] font-mono opacity-60">
                              ({node.childCount})
                            </span>
                          {/if}
                        </button>

                        <div class="flex items-center gap-1">
                          <button
                            type="button"
                            on:click|stopPropagation={() => selectDirectory(node.fullPath)}
                            class="text-[10px] px-2 py-0.5 rounded bg-[var(--card-bg)] hover:bg-purple-600 hover:text-white text-[var(--text-muted)] transition cursor-pointer"
                            title="Choose {node.fullPath}"
                          >
                            Select
                          </button>

                          {#if node.hasChildren}
                            <button
                              type="button"
                              on:click={() => drillDown(node)}
                              class="text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] px-1 cursor-pointer font-bold"
                              title="Open folder"
                            >
                              ›
                            </button>
                          {/if}
                        </div>
                      </div>
                    {/each}
                  {/if}
                </div>
              </div>
            {/if}
          </div>
        </div>

        {#if $uploadQueue.uploadMode === 'files'}
          <div class="space-y-2.5">
            <div class="flex items-center justify-between px-1">
              <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)]">
                Staged ({$uploadQueue.stagedFiles.length})
              </span>
              <button
                type="button"
                on:click={() => fileInputEl?.click()}
                class="text-xs text-purple-600 dark:text-purple-400 hover:underline font-medium cursor-pointer transition-colors"
              >
                + Add more
              </button>
            </div>

            <input
              bind:this={fileInputEl}
              type="file"
              multiple
              accept="image/heic,image/heif,image/jpeg,image/png,image/webp,video/mp4,video/quicktime,.heic,.heif,.mov,.mp4,.jpg,.jpeg,.png,.webp,.gif"
              on:change={handleFileSelect}
              class="hidden"
            />

            {#if $uploadQueue.stagedFiles.length === 0}
              <button
                type="button"
                on:click={() => fileInputEl?.click()}
                class="liquid-dropzone w-full rounded-2xl p-8 flex flex-col items-center justify-center gap-2.5 text-center cursor-pointer transition-all spring-tap"
              >
                <div class="w-10 h-10 rounded-full liquid-icon-plate flex items-center justify-center text-[var(--text-main)]">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
                  </svg>
                </div>
                <span class="text-xs font-medium text-[var(--text-main)] tracking-tight">Select photos or videos</span>
                <span class="text-[10px] text-[var(--text-muted)]">HEIC, RAW, 4K video, Live Photos preserved</span>
              </button>
            {:else}
              <div class="space-y-1.5 max-h-52 overflow-y-auto pr-0.5 no-scrollbar">
                {#each $uploadQueue.stagedFiles as file, idx}
                  <div class="liquid-card flex items-center justify-between p-2.5 rounded-xl text-xs">
                    <div class="truncate mr-3">
                      <div class="text-[var(--text-main)] font-medium truncate max-w-[280px]">
                        {file.name}
                      </div>
                      <div class="text-[10px] text-[var(--text-muted)] mt-0.5 font-mono">
                        {formatBytes(file.size)} • {file.type || 'binary'}
                      </div>
                    </div>
                    <button
                      type="button"
                      on:click={() => uploadQueue.removeFile(idx)}
                      class="text-[var(--text-muted)] hover:text-[var(--text-main)] p-1 cursor-pointer transition-colors spring-tap"
                      title="Remove file"
                      aria-label="Remove {file.name}"
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <line x1="18" y1="6" x2="6" y2="18"></line>
                        <line x1="6" y1="6" x2="18" y2="18"></line>
                      </svg>
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <!-- Web Link Section -->
          {#if !$uploadQueue.linkPreview}
            <div class="space-y-3">
              <div class="space-y-1.5">
                <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-1">
                  Source URL
                </span>
                <div class="flex gap-2">
                  <input
                    id="link-url-input"
                    type="url"
                    value={$uploadQueue.linkUrl}
                    on:input={(e) => uploadQueue.setLinkUrl(e.currentTarget.value)}
                    placeholder="https://..."
                    on:keydown={(e) => e.key === 'Enter' && uploadQueue.inspectLink()}
                    class="liquid-input flex-1 rounded-xl px-3.5 py-2 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] outline-none transition-all"
                  />
                  <button
                    type="button"
                    on:click={() => uploadQueue.inspectLink()}
                    disabled={!$uploadQueue.linkUrl.trim()}
                    class="liquid-btn-primary px-4 py-2 rounded-xl text-xs font-semibold text-white transition-all disabled:opacity-40 cursor-pointer flex items-center gap-1.5 min-w-[76px] justify-center spring-tap shadow-sm"
                  >
                    <span>Inspect</span>
                  </button>
                </div>
              </div>
            </div>
          {:else}
            <!-- Scraped Link Preview -->
            <div class="space-y-3">
              <div class="flex items-center justify-between text-xs px-1">
                <div>
                  <span class="text-[var(--text-main)] font-semibold">{$uploadQueue.linkPreview.author}</span>
                  <span class="text-[var(--text-muted)]"> on {$uploadQueue.linkPreview.platform} ({$uploadQueue.linkPreview.items.length})</span>
                </div>
                <button
                  type="button"
                  on:click={() => uploadQueue.clearLinkPreview()}
                  class="text-purple-600 dark:text-purple-400 hover:underline font-medium cursor-pointer text-xs"
                >
                  Change Link
                </button>
              </div>

              {#if $uploadQueue.linkPreview.caption}
                <p class="text-[11px] text-[var(--text-muted)] line-clamp-2 italic border-l-2 border-purple-500/40 pl-2.5">
                  "{$uploadQueue.linkPreview.caption}"
                </p>
              {/if}

              <div class="grid grid-cols-3 gap-2 max-h-52 overflow-y-auto pr-0.5 no-scrollbar">
                {#each $uploadQueue.linkPreview.items as item}
                  {@const isSelected = $uploadQueue.selectedLinkItems.has(item.id)}
                  <button
                    type="button"
                    on:click={() => uploadQueue.toggleLinkItem(item.id)}
                    class="relative aspect-square liquid-card rounded-xl overflow-hidden cursor-pointer group focus:outline-none transition-all spring-tap {isSelected ? 'ring-2 ring-purple-500 shadow-md' : 'opacity-50'}"
                  >
                    {#if item.thumbnail_base64 || item.thumbnail_url}
                      <img src={getThumbnailSrc(item)} alt="" class="w-full h-full object-cover pointer-events-none" />
                    {:else}
                      <div class="w-full h-full flex flex-col items-center justify-center text-[var(--text-muted)] text-[10px] gap-1 font-mono uppercase">
                        <span>{item.media_type}</span>
                      </div>
                    {/if}

                    <div class="absolute top-1.5 left-1.5 w-4 h-4 rounded-full flex items-center justify-center transition-colors {isSelected ? 'bg-purple-600 text-white' : 'bg-black/50 text-transparent border border-white/40'}">
                      <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
                        <polyline points="20 6 9 17 4 12"></polyline>
                      </svg>
                    </div>

                    <span class="absolute bottom-1 right-1 text-[8px] uppercase px-1.5 py-0.2 rounded-full bg-black/75 text-white font-mono tracking-wider">
                      {item.media_type}
                    </span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}
        {/if}
      </div>

      <!-- Footer Buttons -->
      <div class="px-6 py-4 border-t border-[var(--border-glass)] flex justify-end gap-2.5">
        <button
          type="button"
          on:click={forceClose}
          class="liquid-btn-secondary px-4 py-2 rounded-xl text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={handleStartUpload}
          disabled={!canUpload}
          class="liquid-btn-primary px-5 py-2 rounded-xl text-xs font-semibold text-white transition-all disabled:opacity-40 cursor-pointer flex items-center gap-2 spring-tap shadow-sm"
        >
          {#if $uploadQueue.uploadMode === 'files'}
            Upload {$uploadQueue.stagedFiles.length} item{$uploadQueue.stagedFiles.length === 1 ? '' : 's'}
          {:else}
            Import {$uploadQueue.selectedLinkItems.size} item{$uploadQueue.selectedLinkItems.size === 1 ? '' : 's'}
          {/if}
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

  .liquid-dropzone {
    background: var(--card-bg);
    border: 1.5px dashed var(--border-glass);
  }

  .liquid-dropzone:hover {
    background: var(--dock-bg-hover);
    border-color: var(--border-subtle);
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

  .liquid-input {
    background: var(--pill-bg);
    border: 1px solid var(--border-glass);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.08);
  }

  .liquid-input:focus {
    border-color: var(--border-subtle);
    box-shadow: 0 0 0 1px var(--border-subtle);
  }

  .liquid-dropdown {
    background: var(--bg-surface-elevated);
    backdrop-filter: blur(28px) saturate(190%);
    -webkit-backdrop-filter: blur(28px) saturate(190%);
    box-shadow: 0 12px 32px var(--dock-shadow), inset 0 1px 0 var(--border-specular);
  }

  .liquid-segmented {
    background: var(--pill-bg);
    border: 1px solid var(--border-glass);
  }

  .liquid-seg-active {
    background: var(--dock-bg);
    border: 1px solid var(--dock-border);
    box-shadow:
      0 2px 6px var(--dock-shadow),
      inset 0 1px 0 var(--dock-highlight);
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

  .liquid-icon-plate {
    background: var(--pill-bg);
    border: 1px solid var(--border-glass);
    box-shadow: inset 0 1px 0 var(--border-specular);
  }

  .liquid-btn-primary {
    background: #9333ea;
  }

  .liquid-btn-primary:hover:not(:disabled) {
    background: #7e22ce;
  }

  .liquid-btn-secondary {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
  }

  .liquid-btn-secondary:hover:not(:disabled) {
    background: var(--dock-bg-hover);
    border-color: var(--border-subtle);
  }

  .no-scrollbar::-webkit-scrollbar {
    display: none;
  }
  .no-scrollbar {
    -ms-overflow-style: none;
    scrollbar-width: none;
  }
</style>