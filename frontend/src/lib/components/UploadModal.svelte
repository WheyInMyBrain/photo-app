<!-- photo-app/frontend/src/lib/components/UploadModal.svelte -->
<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import { authStore } from '$lib/stores/authStore';
  import { fetchCustomAlbums } from '$lib/api/albums';
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

  let uploadMode: 'files' | 'link' = 'files';
  let folderPath = '';
  let isUploading = false;
  let uploadProgress = 0;
  let currentFileProgress = 0;
  let statusMessage = '';
  let uploadError = '';
  let existingFolders: string[] = [];
  let isFolderDropdownOpen = false;

  let stagedFiles: File[] = [];
  let fileInputEl: HTMLInputElement;

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

  $: filteredFolders = existingFolders.filter(
    (f) =>
      f.toLowerCase().includes(folderPath.trim().toLowerCase()) &&
      f.toLowerCase() !== folderPath.trim().toLowerCase()
  );

  onMount(async () => {
    try {
      const albums = await fetchCustomAlbums();
      if (Array.isArray(albums)) {
        existingFolders = albums
          .map((a: any) => a.title || a.name || a.label || a.value)
          .filter(Boolean);
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

  function selectFolderSuggestion(folder: string) {
    folderPath = folder;
    isFolderDropdownOpen = false;
  }

  function addFiles(files: FileList | File[]) {
    const rawList = Array.from(files);
    const valid = rawList.filter(
      (f) =>
        f.type.startsWith('image/') ||
        f.type.startsWith('video/') ||
        /\.(heic|heif|mov|mp4|jpg|jpeg|png|webp|gif|mkv|webm)$/i.test(f.name)
    );
    stagedFiles = [...stagedFiles, ...valid];
  }

  function removeFile(index: number) {
    stagedFiles = stagedFiles.filter((_, i) => i !== index);
  }

  function handleFileSelect(e: Event) {
    const input = e.target as HTMLInputElement;
    if (input.files && input.files.length > 0) {
      const selected = Array.from(input.files);
      setTimeout(() => {
        addFiles(selected);
        input.value = '';
      }, 50);
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
    uploadError = '';
    statusMessage = 'Inspecting link...';

    try {
      const res = await fetch('/api/upload/inspect', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: linkUrl.trim() })
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
        throw new Error('No supported media found.');
      }
    } catch (e: any) {
      uploadError = e.message || 'Inspection failed.';
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
      selected_items: linkPreview.items.filter((i) => selectedLinkItems.has(i.id))
    };

    try {
      const res = await fetch('/api/upload/commit', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      });

      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();

      dispatch('uploaded', { count: data.total_uploaded || payload.selected_items.length });
      forceClose();
    } catch (e: any) {
      uploadError = `Import failed: ${e.message}`;
      isUploading = false;
    }
  }

  // Upload small files with single-part XHR progress
  function uploadSingleWithProgress(file: File, folder: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      const formData = new FormData();
      formData.append('file', file);
      if (folder.trim()) {
        formData.append('folder', folder.trim());
      }

      xhr.upload.onprogress = (e) => {
        if (e.lengthComputable) {
          currentFileProgress = Math.round((e.loaded / e.total) * 100);
        }
      };

      xhr.onload = () => {
        if (xhr.status >= 200 && xhr.status < 300) {
          resolve();
        } else if (xhr.status === 401) {
          authStore.checkStatus();
          reject(new Error('Session expired. Please log in again.'));
        } else {
          reject(new Error(xhr.responseText || `Upload failed with status ${xhr.status}`));
        }
      };

      xhr.onerror = () => reject(new Error('Network connection error or request terminated by server.'));
      xhr.open('POST', '/api/upload');
      xhr.withCredentials = true;
      xhr.send(formData);
    });
  }

  async function handleMasterUpload() {
    if (isUploading || !canUpload) return;

    if (uploadMode === 'link') {
      await commitLink();
      return;
    }

    isUploading = true;
    uploadError = '';
    uploadProgress = 0;
    currentFileProgress = 0;
    const totalCount = stagedFiles.length;

    try {
      for (let i = 0; i < stagedFiles.length; i++) {
        const file = stagedFiles[i];
        currentFileProgress = 0;

        if (file.size > CHUNK_THRESHOLD_BYTES) {
          // Use chunked upload for large files to avoid Vite proxy buffer overflow and Actix payload limits
          statusMessage = `Streaming ${file.name} in chunks...`;
          await uploadChunked(file, folderPath, (part, total) => {
            currentFileProgress = Math.round((part / total) * 100);
            statusMessage = `Uploading ${file.name} (chunk ${part}/${total})...`;
          });
        } else {
          // Use single-part upload with live XHR byte progress for standard files
          statusMessage = `Uploading (${i + 1}/${totalCount}): ${file.name}`;
          await uploadSingleWithProgress(file, folderPath);
        }

        uploadProgress = Math.round(((i + 1) / totalCount) * 100);
      }

      statusMessage = 'Upload completed.';
      dispatch('uploaded', { count: totalCount });
      forceClose();
    } catch (e: any) {
      uploadError = e.message || 'Upload failed.';
      isUploading = false;
    }
  }

  function forceClose() {
    isUploading = false;
    stagedFiles = [];
    folderPath = '';
    uploadProgress = 0;
    currentFileProgress = 0;
    statusMessage = '';
    uploadError = '';
    linkUrl = '';
    linkPreview = null;
    selectedLinkItems.clear();
    isFolderDropdownOpen = false;
    dispatch('close');
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !isUploading) forceClose();
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
    on:click|self={() => { if (!isUploading) forceClose(); }}
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
          disabled={isUploading}
          class="liquid-icon-btn w-7 h-7 flex items-center justify-center rounded-full text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer disabled:opacity-30"
          title="Close"
          aria-label="Close modal"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18"></line>
            <line x1="6" y1="6" x2="18" y2="18"></line>
          </svg>
        </button>
      </div>

      <!-- Segmented Mode Switcher -->
      <div class="px-6 pt-4">
        <div class="liquid-segmented p-0.5 rounded-xl flex text-xs">
          <button
            class="flex-1 py-1.5 rounded-lg transition-all font-medium spring-tap cursor-pointer {uploadMode === 'files' ? 'liquid-seg-active text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            on:click={() => { uploadMode = 'files'; uploadError = ''; }}
            disabled={isUploading}
          >
            Files
          </button>
          <button
            class="flex-1 py-1.5 rounded-lg transition-all font-medium spring-tap cursor-pointer {uploadMode === 'link' ? 'liquid-seg-active text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
            on:click={() => { uploadMode = 'link'; uploadError = ''; }}
            disabled={isUploading}
          >
            Web Link
          </button>
        </div>
      </div>

      <!-- Scrollable Form Container -->
      <div class="p-6 space-y-4 overflow-y-auto flex-1 no-scrollbar">
        <!-- Destination Album / Folder Input with Dynamic Suggestions -->
        <div class="space-y-1.5 relative">
          <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-1">
            Destination Album
          </span>
          <div class="relative">
            <input
              id="upload-folder-input"
              type="text"
              autocomplete="off"
              bind:value={folderPath}
              on:focus={() => (isFolderDropdownOpen = true)}
              on:blur={() => setTimeout(() => (isFolderDropdownOpen = false), 200)}
              placeholder="root (e.g. 2026/holidays)..."
              disabled={isUploading || linkPreview !== null}
              class="liquid-input w-full rounded-xl px-3.5 py-2 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] outline-none transition-all disabled:opacity-40"
            />

            {#if isFolderDropdownOpen && filteredFolders.length > 0 && !isUploading}
              <div class="absolute left-0 right-0 top-full mt-1.5 z-50 liquid-dropdown rounded-xl p-1 shadow-2xl max-h-40 overflow-y-auto no-scrollbar">
                {#each filteredFolders as folder}
                  <button
                    type="button"
                    class="w-full text-left px-3 py-1.5 rounded-lg text-xs text-[var(--text-main)] hover:bg-[var(--dock-bg-hover)] transition-colors flex items-center gap-2 cursor-pointer"
                    on:mousedown|preventDefault={() => selectFolderSuggestion(folder)}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 text-[var(--text-muted)]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                    </svg>
                    <span class="truncate">{folder}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        </div>

        {#if uploadMode === 'files'}
          <div class="space-y-2.5">
            <div class="flex items-center justify-between px-1">
              <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)]">
                Staged ({stagedFiles.length})
              </span>
              <button
                type="button"
                on:click={() => fileInputEl?.click()}
                disabled={isUploading}
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

            {#if stagedFiles.length === 0}
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
                {#each stagedFiles as file, idx}
                  <div class="liquid-card flex items-center justify-between p-2.5 rounded-xl text-xs">
                    <div class="truncate mr-3">
                      <div class="text-[var(--text-main)] font-medium truncate max-w-[280px] flex items-center gap-2">
                        <span class="truncate">{file.name}</span>
                        {#if file.size > CHUNK_THRESHOLD_BYTES}
                          <span class="text-[9px] liquid-tag px-1.5 py-0.2 rounded font-mono">CHUNKED</span>
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
                        class="text-[var(--text-muted)] hover:text-[var(--text-main)] p-1 cursor-pointer transition-colors spring-tap"
                        title="Remove file"
                        aria-label="Remove {file.name}"
                      >
                        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                          <line x1="18" y1="6" x2="6" y2="18"></line>
                          <line x1="6" y1="6" x2="18" y2="18"></line>
                        </svg>
                      </button>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          {#if !linkPreview}
            <div class="space-y-3">
              <div class="space-y-1.5">
                <span class="text-[9px] uppercase tracking-wider font-semibold text-[var(--text-muted)] block pl-1">
                  Source URL
                </span>
                <div class="flex gap-2">
                  <input
                    id="link-url-input"
                    type="url"
                    bind:value={linkUrl}
                    placeholder="https://..."
                    disabled={isUploading}
                    on:keydown={(e) => e.key === 'Enter' && inspectLink()}
                    class="liquid-input flex-1 rounded-xl px-3.5 py-2 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] outline-none transition-all"
                  />
                  <button
                    type="button"
                    on:click={inspectLink}
                    disabled={!linkUrl.trim() || isUploading}
                    class="liquid-btn-primary px-4 py-2 rounded-xl text-xs font-semibold text-white transition-all disabled:opacity-40 cursor-pointer flex items-center gap-1.5 min-w-[76px] justify-center spring-tap shadow-sm"
                  >
                    {#if isUploading}
                      <span class="inline-block w-3 h-3 border-2 border-white/20 border-t-white rounded-full animate-spin"></span>
                    {:else}
                      <span>Inspect</span>
                    {/if}
                  </button>
                </div>
              </div>
            </div>
          {:else}
            <div class="space-y-3">
              <div class="flex items-center justify-between text-xs px-1">
                <div>
                  <span class="text-[var(--text-main)] font-semibold">{linkPreview.author}</span>
                  <span class="text-[var(--text-muted)]"> on {linkPreview.platform} ({linkPreview.items.length})</span>
                </div>
                <button
                  type="button"
                  on:click={() => { linkPreview = null; uploadError = ''; }}
                  disabled={isUploading}
                  class="text-purple-600 dark:text-purple-400 hover:underline font-medium cursor-pointer text-xs"
                >
                  Change Link
                </button>
              </div>

              {#if linkPreview.caption}
                <p class="text-[11px] text-[var(--text-muted)] line-clamp-2 italic border-l-2 border-purple-500/40 pl-2.5">
                  "{linkPreview.caption}"
                </p>
              {/if}

              <div class="grid grid-cols-3 gap-2 max-h-52 overflow-y-auto pr-0.5 no-scrollbar">
                {#each linkPreview.items as item}
                  {@const isSelected = selectedLinkItems.has(item.id)}
                  <button
                    type="button"
                    disabled={isUploading}
                    on:click={() => toggleLinkItem(item.id)}
                    class="relative aspect-square liquid-card rounded-xl overflow-hidden cursor-pointer group focus:outline-none transition-all spring-tap {isSelected ? 'ring-2 ring-purple-500 shadow-md' : 'opacity-50'}"
                  >
                    {#if item.thumbnail_base64 || item.thumbnail_url}
                      <img
                        src={getThumbnailSrc(item)}
                        alt="Preview"
                        class="w-full h-full object-cover pointer-events-none"
                      />
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

        <!-- Active Progress Bar with live percentage and file status -->
        {#if isUploading}
          <div class="space-y-1.5 pt-2">
            <div class="flex justify-between text-xs text-[var(--text-muted)] font-mono">
              <span class="truncate max-w-[280px] text-[11px]">{statusMessage}</span>
              <span class="text-[var(--text-main)] font-semibold text-[11px]">{currentFileProgress}%</span>
            </div>
            <div class="w-full bg-[var(--pill-bg)] rounded-full h-1.5 overflow-hidden border border-[var(--border-glass)]">
              <div
                class="bg-gradient-to-r from-purple-600 to-emerald-500 h-full transition-all duration-150 ease-out rounded-full"
                style="width: {currentFileProgress}%"
              ></div>
            </div>
          </div>
        {/if}

        <!-- Visible Error Box -->
        {#if uploadError}
          <div class="p-2.5 rounded-xl bg-rose-500/15 border border-rose-500/30 text-rose-600 dark:text-rose-200 text-xs flex items-start gap-2">
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 flex-shrink-0 text-rose-500 mt-0.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="12" cy="12" r="10"></circle>
              <line x1="12" y1="8" x2="12" y2="12"></line>
              <line x1="12" y1="16" x2="12.01" y2="16"></line>
            </svg>
            <span class="leading-snug">{uploadError}</span>
          </div>
        {/if}
      </div>

      <!-- Footer Actions -->
      <div class="px-6 py-4 border-t border-[var(--border-glass)] flex justify-end gap-2.5">
        <button
          type="button"
          on:click={forceClose}
          disabled={isUploading}
          class="liquid-btn-secondary px-4 py-2 rounded-xl text-xs text-[var(--text-muted)] hover:text-[var(--text-main)] transition-all spring-tap cursor-pointer disabled:opacity-40"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={handleMasterUpload}
          disabled={!canUpload || isUploading}
          class="liquid-btn-primary px-5 py-2 rounded-xl text-xs font-semibold text-white transition-all disabled:opacity-40 cursor-pointer flex items-center gap-2 spring-tap shadow-sm"
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
    border: 1px solid var(--border-glass);
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

  .liquid-tag {
    background: var(--pill-bg);
    border: 1px solid var(--border-glass);
    color: var(--text-muted);
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