<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';
  import { authStore } from '$lib/stores/authStore';
  import type { CandidateItem, InspectPreview } from '$lib/types/upload';
  import {
    CHUNK_THRESHOLD_BYTES,
    formatBytes,
    uploadDirect,
    uploadChunked
  } from '$lib/utils/uploader';

  export let isOpen = false;
  export let initialFiles: File[] = [];

  const dispatch = createEventDispatcher<{
    close: void;
    uploaded: { count: number };
  }>();

  // Mode & Navigation
  let uploadMode: 'files' | 'link' = 'files';
  let folderPath = '';
  let isUploading = false;
  let uploadProgress = 0;
  let statusMessage = '';
  let inspectError = '';
  let existingFolders: string[] = [];

  // File Upload State
  let stagedFiles: File[] = [];
  let fileInputEl: HTMLInputElement;

  // Link Upload State
  let linkUrl = '';
  let linkPreview: InspectPreview | null = null;
  let selectedLinkItems: Set<string> = new Set();

  $: if (initialFiles && initialFiles.length > 0) {
    uploadMode = 'files';
    addFiles(initialFiles);
    initialFiles = [];
  }

  $: canUpload =
    uploadMode === 'files'
      ? stagedFiles.length > 0
      : linkPreview !== null && selectedLinkItems.size > 0;

  onMount(async () => {
    try {
      const res = await fetch('/api/media/filters');
      if (res.status === 401) {
        authStore.checkStatus();
        return;
      }
      if (res.ok) {
        const data = await res.json();
        if (Array.isArray(data.albums)) {
          existingFolders = data.albums.map((a: any) => a.value || a.label).filter(Boolean);
        }
      }
    } catch {
      existingFolders = [];
    }
  });

  onMount(() => {
    const originalOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = originalOverflow;
    };
  });

  function addFiles(files: FileList | File[]) {
    const valid = Array.from(files).filter(
      (f) => f.type.startsWith('image/') || f.type.startsWith('video/')
    );
    stagedFiles = [...stagedFiles, ...valid];
  }

  function removeFile(index: number) {
    stagedFiles = stagedFiles.filter((_, i) => i !== index);
  }

  function handleFileSelect(e: Event) {
    const input = e.target as HTMLInputElement;
    if (input.files) {
      addFiles(input.files);
      input.value = '';
    }
  }

  function getThumbnailSrc(item: CandidateItem): string {
    if (item.thumbnail_base64 && item.thumbnail_base64.trim().length > 0) {
      return item.thumbnail_base64.startsWith('data:')
        ? item.thumbnail_base64
        : `data:image/jpeg;base64,${item.thumbnail_base64}`;
    }
    return item.thumbnail_url || '';
  }

  async function inspectLink() {
    if (!linkUrl.trim() || isUploading) return;
    isUploading = true;
    inspectError = '';
    statusMessage = 'Inspecting web link...';

    try {
      const res = await fetch('/api/upload/inspect', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: linkUrl.trim() }),
      });

      if (res.status === 401) {
        authStore.checkStatus();
        throw new Error('Session expired.');
      }

      if (!res.ok) {
        const errorText = await res.text();
        throw new Error(errorText || `Server error (${res.status})`);
      }

      const data = await res.json();
      const previewData: InspectPreview | null = data.Preview ?? (data.items ? data : null);

      if (previewData && Array.isArray(previewData.items)) {
        linkPreview = previewData;
        folderPath = previewData.suggested_folder || '';
        selectedLinkItems = new Set(previewData.items.map((i: CandidateItem) => i.id));
      } else if (data.Committed) {
        dispatch('uploaded', { count: data.Committed.total_uploaded || 1 });
        forceClose();
      } else {
        throw new Error('No supported media found at this address.');
      }
    } catch (e: any) {
      inspectError = e.message || 'Inspection failed.';
    } finally {
      isUploading = false;
      statusMessage = '';
    }
  }

  function toggleLinkItem(id: string) {
    if (selectedLinkItems.has(id)) {
      selectedLinkItems.delete(id);
    } else {
      selectedLinkItems.add(id);
    }
    selectedLinkItems = new Set(selectedLinkItems);
  }

  async function commitLink() {
    if (!linkPreview || selectedLinkItems.size === 0) return;

    isUploading = true;
    uploadProgress = 15;
    statusMessage = 'Importing selected items...';

    const payload = {
      platform: linkPreview.platform,
      folder: folderPath.trim() || undefined,
      selected_items: linkPreview.items.filter((i) => selectedLinkItems.has(i.id)),
    };

    try {
      const res = await fetch('/api/upload/commit', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });

      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();

      dispatch('uploaded', { count: data.total_uploaded || payload.selected_items.length });
      forceClose();
    } catch (e: any) {
      inspectError = `Import failed: ${e.message}`;
      isUploading = false;
    }
  }

  async function handleMasterUpload() {
    if (isUploading || !canUpload) return;

    if (uploadMode === 'link') {
      await commitLink();
      return;
    }

    isUploading = true;
    uploadProgress = 5;
    const totalCount = stagedFiles.length;

    try {
      for (let i = 0; i < stagedFiles.length; i++) {
        const file = stagedFiles[i];
        if (file.size > CHUNK_THRESHOLD_BYTES) {
          await uploadChunked(file, folderPath, (part, total) => {
            statusMessage = `Streaming ${file.name} (chunk ${part}/${total})...`;
          });
        } else {
          statusMessage = `Uploading ${file.name}...`;
          await uploadDirect(file, folderPath);
        }
        uploadProgress = Math.round(((i + 1) / totalCount) * 100);
      }

      statusMessage = 'Upload completed successfully.';
      dispatch('uploaded', { count: totalCount });
      forceClose();
    } catch (e: any) {
      statusMessage = `Failed: ${e.message}`;
      isUploading = false;
    }
  }

  function forceClose() {
    isUploading = false;
    stagedFiles = [];
    folderPath = '';
    uploadProgress = 0;
    statusMessage = '';
    inspectError = '';
    linkUrl = '';
    linkPreview = null;
    selectedLinkItems.clear();
    dispatch('close');
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !isUploading) forceClose();
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<datalist id="folder-suggestions">
  {#each existingFolders as folder}
    <option value={folder}>{folder}</option>
  {/each}
</datalist>

{#if isOpen}
  <!-- Backdrop -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-md p-3 sm:p-6 select-none"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    on:click|self={() => { if (!isUploading) forceClose(); }}
  >
    <!-- Modal Card Shell -->
    <div
      class="glass-panel rounded-3xl w-full max-w-lg shadow-2xl flex flex-col max-h-[85vh] overflow-hidden border border-[var(--border-glass)] bg-[var(--bg-surface-elevated)]"
    >
      <!-- Specular Header -->
      <div class="px-6 py-4 border-b border-[var(--border-glass)] flex items-center justify-between">
        <div>
          <h2 class="text-sm sm:text-base font-bold text-[var(--text-main)] flex items-center gap-2">
            <span>Add Media</span>
          </h2>
          <p class="text-[11px] text-[var(--text-muted)] mt-0.5">
            Store raw originals with zero cloud compression.
          </p>
        </div>
        <button
          type="button"
          on:click={forceClose}
          disabled={isUploading}
          class="w-7 h-7 flex items-center justify-center rounded-full glass-panel text-[var(--text-muted)] hover:text-[var(--text-main)] transition-colors spring-tap cursor-pointer disabled:opacity-30 text-xs"
          title="Close (Esc)"
          aria-label="Close modal"
        >
          ✕
        </button>
      </div>

      <!-- Segmented Mode Selector -->
      <div class="px-6 pt-4">
        <div class="flex p-1 rounded-xl glass-panel text-xs">
          <button
            class="flex-1 py-1.5 rounded-lg transition-all font-medium spring-tap cursor-pointer {uploadMode === 'files' ? 'bg-purple-600 text-white shadow-md' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            on:click={() => { uploadMode = 'files'; inspectError = ''; }}
            disabled={isUploading}
          >
            Device Files
          </button>
          <button
            class="flex-1 py-1.5 rounded-lg transition-all font-medium spring-tap cursor-pointer {uploadMode === 'link' ? 'bg-purple-600 text-white shadow-md' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            on:click={() => { uploadMode = 'link'; inspectError = ''; }}
            disabled={isUploading}
          >
            Web Link
          </button>
        </div>
      </div>

      <!-- Scrollable Form Container -->
      <div class="p-6 space-y-5 overflow-y-auto flex-1 no-scrollbar">
        <!-- Destination Album Input -->
        <div class="space-y-1.5">
          <label for="upload-folder-input" class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider block pl-0.5">
            Destination Album / Folder
          </label>
          <input
            id="upload-folder-input"
            type="text"
            list="folder-suggestions"
            bind:value={folderPath}
            placeholder="root (e.g. 2026/holidays)..."
            disabled={isUploading || linkPreview !== null}
            class="w-full bg-[var(--bg-surface)] border border-[var(--border-glass)] focus:ring-2 focus:ring-purple-500/50 rounded-xl px-3.5 py-2 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] outline-none transition-all disabled:opacity-50"
          />
        </div>

        {#if uploadMode === 'files'}
          <!-- Local Files Section -->
          <div class="space-y-2.5">
            <div class="flex items-center justify-between px-0.5">
              <span class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider">
                Files ({stagedFiles.length})
              </span>
              <button
                type="button"
                on:click={() => fileInputEl?.click()}
                disabled={isUploading}
                class="text-xs text-purple-400 hover:text-purple-300 font-medium cursor-pointer"
              >
                + Add files
              </button>
            </div>

            <input
              bind:this={fileInputEl}
              type="file"
              multiple
              accept="image/*,video/*"
              on:change={handleFileSelect}
              class="hidden"
            />

            {#if stagedFiles.length === 0}
              <button
                type="button"
                on:click={() => fileInputEl?.click()}
                class="w-full border border-dashed border-[var(--border-glass)] hover:border-purple-500/50 rounded-2xl p-8 flex flex-col items-center justify-center gap-2 text-center cursor-pointer transition-colors glass-panel"
              >
                <div class="w-11 h-11 rounded-2xl bg-purple-600/10 text-purple-400 flex items-center justify-center text-xl shadow-inner">
                  📁
                </div>
                <span class="text-xs font-semibold text-[var(--text-main)]">Tap to select photos & videos</span>
                <span class="text-[10px] text-[var(--text-muted)]">Original EXIF, Live Photos, 4K videos & HEIC preserved</span>
              </button>
            {:else}
              <div class="space-y-2 max-h-56 overflow-y-auto pr-1 no-scrollbar">
                {#each stagedFiles as file, idx}
                  <div
                    class="flex items-center justify-between p-2.5 glass-panel rounded-xl text-xs"
                  >
                    <div class="truncate mr-3">
                      <div class="text-[var(--text-main)] font-medium truncate max-w-[280px] flex items-center gap-2">
                        <span class="truncate">{file.name}</span>
                        {#if file.size > CHUNK_THRESHOLD_BYTES}
                          <span class="text-[9px] bg-purple-500/20 text-purple-300 border border-purple-500/30 px-1.5 py-0.2 rounded font-mono font-bold">STREAM</span>
                        {/if}
                      </div>
                      <div class="text-[10px] text-[var(--text-muted)] mt-0.5 font-mono">
                        {formatBytes(file.size)} • {file.type || 'binary'}
                      </div>
                    </div>
                    {#if !isUploading}
                      <button
                        type="button"
                        on:click={() => removeFile(idx)}
                        class="text-[var(--text-muted)] hover:text-red-400 p-1 cursor-pointer text-xs spring-tap"
                        title="Remove file"
                      >
                        ✕
                      </button>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <!-- Web Link Section -->
          {#if !linkPreview}
            <div class="space-y-3">
              <div class="space-y-1.5">
                <label for="link-url-input" class="text-[10px] uppercase font-bold text-[var(--text-muted)] tracking-wider block pl-0.5">
                  Post or Album URL
                </label>
                <div class="flex gap-2">
                  <input
                    id="link-url-input"
                    type="url"
                    bind:value={linkUrl}
                    placeholder="https://instagram.com/p/... or https://reddit.com/r/..."
                    disabled={isUploading}
                    on:keydown={(e) => e.key === 'Enter' && inspectLink()}
                    class="flex-1 bg-[var(--bg-surface)] border border-[var(--border-glass)] focus:ring-2 focus:ring-purple-500/50 rounded-xl px-3.5 py-2 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] outline-none transition-all"
                  />
                  <button
                    type="button"
                    on:click={inspectLink}
                    disabled={!linkUrl.trim() || isUploading}
                    class="bg-purple-600 hover:bg-purple-500 text-white px-4 py-2 rounded-xl text-xs font-semibold transition-all disabled:opacity-40 cursor-pointer flex items-center gap-1.5 min-w-[80px] justify-center spring-tap shadow-md shadow-purple-600/20"
                  >
                    {#if isUploading}
                      <span class="inline-block w-3 h-3 border-2 border-white/20 border-t-white rounded-full animate-spin"></span>
                    {:else}
                      <span>Inspect</span>
                    {/if}
                  </button>
                </div>
                {#if inspectError}
                  <p class="text-[11px] text-red-400 pt-1 leading-snug pl-0.5">{inspectError}</p>
                {/if}
              </div>
            </div>
          {:else}
            <!-- Link Candidate Grid -->
            <div class="space-y-3">
              <div class="flex items-center justify-between text-xs px-0.5">
                <div>
                  <span class="text-[var(--text-main)] font-semibold">{linkPreview.author}</span>
                  <span class="text-[var(--text-muted)]"> on {linkPreview.platform} ({linkPreview.items.length})</span>
                </div>
                <button
                  type="button"
                  on:click={() => { linkPreview = null; inspectError = ''; }}
                  disabled={isUploading}
                  class="text-purple-400 hover:text-purple-300 font-medium cursor-pointer text-xs"
                >
                  Change Link
                </button>
              </div>

              {#if linkPreview.caption}
                <p class="text-[11px] text-[var(--text-muted)] line-clamp-2 italic border-l-2 border-purple-500/50 pl-2.5">
                  "{linkPreview.caption}"
                </p>
              {/if}

              <div class="grid grid-cols-3 gap-2.5 max-h-56 overflow-y-auto pr-0.5 no-scrollbar">
                {#each linkPreview.items as item}
                  {@const isSelected = selectedLinkItems.has(item.id)}
                  <button
                    type="button"
                    disabled={isUploading}
                    on:click={() => toggleLinkItem(item.id)}
                    class="relative aspect-square glass-panel rounded-xl overflow-hidden cursor-pointer group focus:outline-none transition-all spring-tap {isSelected ? 'ring-2 ring-purple-500' : 'opacity-50'}"
                  >
                    {#if item.thumbnail_base64 || item.thumbnail_url}
                      <img
                        src={getThumbnailSrc(item)}
                        alt="Preview thumbnail"
                        class="w-full h-full object-cover pointer-events-none"
                      />
                    {:else}
                      <div class="w-full h-full flex flex-col items-center justify-center text-[var(--text-muted)] text-xs gap-1">
                        <span>{item.media_type === 'video' ? '🎬' : '🖼️'}</span>
                        <span class="capitalize text-[10px]">{item.media_type}</span>
                      </div>
                    {/if}

                    <div class="absolute top-1.5 left-1.5 w-5 h-5 rounded-full flex items-center justify-center transition-colors {isSelected ? 'bg-purple-600 text-white shadow-md' : 'bg-black/40 text-transparent border border-white/30'}">
                      <span class="text-[10px] font-bold">✓</span>
                    </div>

                    <span class="absolute bottom-1 right-1 text-[9px] uppercase px-1.5 py-0.5 rounded-full bg-black/60 backdrop-blur-md text-white font-mono tracking-tighter">
                      {item.media_type}
                    </span>
                  </button>
                {/each}
              </div>

              {#if inspectError}
                <p class="text-[11px] text-red-400 pt-1 leading-snug pl-0.5">{inspectError}</p>
              {/if}
            </div>
          {/if}
        {/if}

        <!-- Stream / Upload Progress Indicator -->
        {#if isUploading && (uploadMode === 'files' || uploadProgress > 0)}
          <div class="space-y-2 pt-2">
            <div class="flex justify-between text-xs text-[var(--text-muted)]">
              <span class="truncate max-w-[280px] font-mono">{statusMessage}</span>
              <span class="font-mono font-bold text-purple-400">{uploadMode === 'files' ? `${uploadProgress}%` : ''}</span>
            </div>
            <div class="w-full bg-[var(--bg-surface)] rounded-full h-1.5 overflow-hidden border border-[var(--border-glass)]">
              <div
                class="bg-purple-600 h-full transition-all duration-300 ease-out rounded-full"
                style="width: {uploadProgress > 0 ? uploadProgress : 100}%"
              ></div>
            </div>
          </div>
        {/if}
      </div>

      <!-- Footer Actions -->
      <div class="px-6 py-4 border-t border-[var(--border-glass)] flex justify-end gap-2.5 bg-[var(--bg-surface)]">
        <button
          type="button"
          on:click={forceClose}
          disabled={isUploading}
          class="px-4 py-2 rounded-xl text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] glass-panel transition-all spring-tap cursor-pointer disabled:opacity-40"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={handleMasterUpload}
          disabled={!canUpload || isUploading}
          class="px-5 py-2 rounded-xl text-xs font-semibold bg-purple-600 hover:bg-purple-500 text-white transition-all disabled:opacity-40 cursor-pointer flex items-center gap-2 spring-tap shadow-lg shadow-purple-600/25"
        >
          {#if isUploading}
            <span class="inline-block w-3.5 h-3.5 border-2 border-white/20 border-t-white rounded-full animate-spin"></span>
            <span>Uploading...</span>
          {:else if uploadMode === 'files'}
            Upload {stagedFiles.length} item{stagedFiles.length === 1 ? '' : 's'}
          {:else}
            Import {selectedLinkItems.size} item{selectedLinkItems.size === 1 ? '' : 's'}
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}