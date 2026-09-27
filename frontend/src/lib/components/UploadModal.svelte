<!-- photo-app/frontend/src/lib/components/UploadModal.svelte -->
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

  let uploadMode: 'files' | 'link' = 'files';
  let folderPath = '';
  let isUploading = false;
  let uploadProgress = 0;
  let statusMessage = '';
  let inspectError = '';
  let existingFolders: string[] = [];

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
    statusMessage = 'Inspecting link...';

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
        throw new Error('No supported media found.');
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
            statusMessage = `Streaming ${file.name} (${part}/${total})...`;
          });
        } else {
          statusMessage = `Uploading ${file.name}...`;
          await uploadDirect(file, folderPath);
        }
        uploadProgress = Math.round(((i + 1) / totalCount) * 100);
      }

      statusMessage = 'Upload completed.';
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
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md p-4 sm:p-6 select-none"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    on:click|self={() => { if (!isUploading) forceClose(); }}
  >
    <!-- Liquid-Glass Modal Card -->
    <div
      class="liquid-modal rounded-3xl w-full max-w-lg flex flex-col max-h-[85vh] overflow-hidden"
    >
      <!-- Header -->
      <div class="px-6 py-4.5 border-b border-white/[0.08] flex items-center justify-between">
        <div>
          <h2 class="text-sm font-semibold tracking-tight text-white flex items-center gap-2">
            <span>Add Media</span>
          </h2>
          <p class="text-[11px] text-white/45 mt-0.5 font-normal tracking-tight">
            Original files preserved with full metadata.
          </p>
        </div>
        <button
          type="button"
          on:click={forceClose}
          disabled={isUploading}
          class="liquid-icon-btn w-7 h-7 flex items-center justify-center rounded-full text-white/50 hover:text-white transition-all spring-tap cursor-pointer disabled:opacity-30"
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
            class="flex-1 py-1.5 rounded-lg transition-all font-medium spring-tap cursor-pointer {uploadMode === 'files' ? 'liquid-seg-active text-white' : 'text-white/50 hover:text-white'}"
            on:click={() => { uploadMode = 'files'; inspectError = ''; }}
            disabled={isUploading}
          >
            Files
          </button>
          <button
            class="flex-1 py-1.5 rounded-lg transition-all font-medium spring-tap cursor-pointer {uploadMode === 'link' ? 'liquid-seg-active text-white' : 'text-white/50 hover:text-white'}"
            on:click={() => { uploadMode = 'link'; inspectError = ''; }}
            disabled={isUploading}
          >
            Web Link
          </button>
        </div>
      </div>

      <!-- Scrollable Form Container -->
      <div class="p-6 space-y-4 overflow-y-auto flex-1 no-scrollbar">
        <!-- Destination Album / Folder Input -->
        <div class="space-y-1.5">
          <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40 block pl-1">
            Destination Album
          </span>
          <input
            id="upload-folder-input"
            type="text"
            list="folder-suggestions"
            bind:value={folderPath}
            placeholder="root (e.g. 2026/holidays)..."
            disabled={isUploading || linkPreview !== null}
            class="liquid-input w-full rounded-xl px-3.5 py-2 text-xs text-white placeholder-white/30 outline-none transition-all disabled:opacity-40"
          />
        </div>

        {#if uploadMode === 'files'}
          <!-- Local Files Section -->
          <div class="space-y-2.5">
            <div class="flex items-center justify-between px-1">
              <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40">
                Staged ({stagedFiles.length})
              </span>
              <button
                type="button"
                on:click={() => fileInputEl?.click()}
                disabled={isUploading}
                class="text-xs text-purple-300 hover:text-purple-200 font-medium cursor-pointer transition-colors"
              >
                + Add more
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
                class="liquid-dropzone w-full rounded-2xl p-8 flex flex-col items-center justify-center gap-2.5 text-center cursor-pointer transition-all spring-tap"
              >
                <div class="w-10 h-10 rounded-full liquid-icon-plate flex items-center justify-center text-white/70">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
                  </svg>
                </div>
                <span class="text-xs font-medium text-white tracking-tight">Select photos or videos</span>
                <span class="text-[10px] text-white/40">HEIC, RAW, 4K video, Live Photos preserved</span>
              </button>
            {:else}
              <div class="space-y-1.5 max-h-52 overflow-y-auto pr-0.5 no-scrollbar">
                {#each stagedFiles as file, idx}
                  <div class="liquid-card flex items-center justify-between p-2.5 rounded-xl text-xs">
                    <div class="truncate mr-3">
                      <div class="text-white/90 font-medium truncate max-w-[280px] flex items-center gap-2">
                        <span class="truncate">{file.name}</span>
                        {#if file.size > CHUNK_THRESHOLD_BYTES}
                          <span class="text-[9px] liquid-tag px-1.5 py-0.2 rounded font-mono">STREAM</span>
                        {/if}
                      </div>
                      <div class="text-[10px] text-white/40 mt-0.5 font-mono">
                        {formatBytes(file.size)} • {file.type || 'binary'}
                      </div>
                    </div>
                    {#if !isUploading}
                      <button
                        type="button"
                        on:click={() => removeFile(idx)}
                        class="text-white/40 hover:text-white p-1 cursor-pointer transition-colors spring-tap"
                        title="Remove file"
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
          <!-- Web Link Section -->
          {#if !linkPreview}
            <div class="space-y-3">
              <div class="space-y-1.5">
                <span class="text-[9px] uppercase tracking-wider font-semibold text-white/40 block pl-1">
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
                    class="liquid-input flex-1 rounded-xl px-3.5 py-2 text-xs text-white placeholder-white/30 outline-none transition-all"
                  />
                  <button
                    type="button"
                    on:click={inspectLink}
                    disabled={!linkUrl.trim() || isUploading}
                    class="liquid-btn-primary px-4 py-2 rounded-xl text-xs font-medium text-white transition-all disabled:opacity-40 cursor-pointer flex items-center gap-1.5 min-w-[76px] justify-center spring-tap"
                  >
                    {#if isUploading}
                      <span class="inline-block w-3 h-3 border-2 border-white/20 border-t-white rounded-full animate-spin"></span>
                    {:else}
                      <span>Inspect</span>
                    {/if}
                  </button>
                </div>
                {#if inspectError}
                  <p class="text-[11px] text-rose-300 pt-1 leading-snug pl-1">{inspectError}</p>
                {/if}
              </div>
            </div>
          {:else}
            <!-- Link Candidate Grid -->
            <div class="space-y-3">
              <div class="flex items-center justify-between text-xs px-1">
                <div>
                  <span class="text-white font-medium">{linkPreview.author}</span>
                  <span class="text-white/45"> on {linkPreview.platform} ({linkPreview.items.length})</span>
                </div>
                <button
                  type="button"
                  on:click={() => { linkPreview = null; inspectError = ''; }}
                  disabled={isUploading}
                  class="text-purple-300 hover:text-purple-200 font-medium cursor-pointer text-xs"
                >
                  Change Link
                </button>
              </div>

              {#if linkPreview.caption}
                <p class="text-[11px] text-white/60 line-clamp-2 italic border-l-2 border-white/20 pl-2.5">
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
                    class="relative aspect-square liquid-card rounded-xl overflow-hidden cursor-pointer group focus:outline-none transition-all spring-tap {isSelected ? 'ring-1.5 ring-white/70' : 'opacity-40'}"
                  >
                    {#if item.thumbnail_base64 || item.thumbnail_url}
                      <img
                        src={getThumbnailSrc(item)}
                        alt="Preview"
                        class="w-full h-full object-cover pointer-events-none"
                      />
                    {:else}
                      <div class="w-full h-full flex flex-col items-center justify-center text-white/50 text-[10px] gap-1 font-mono uppercase">
                        <span>{item.media_type}</span>
                      </div>
                    {/if}

                    <div class="absolute top-1.5 left-1.5 w-4 h-4 rounded-full flex items-center justify-center transition-colors {isSelected ? 'bg-white text-black' : 'bg-black/50 text-transparent border border-white/30'}">
                      <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
                        <polyline points="20 6 9 17 4 12"></polyline>
                      </svg>
                    </div>

                    <span class="absolute bottom-1 right-1 text-[8px] uppercase px-1.5 py-0.2 rounded-full bg-black/70 text-white/80 font-mono tracking-wider">
                      {item.media_type}
                    </span>
                  </button>
                {/each}
              </div>

              {#if inspectError}
                <p class="text-[11px] text-rose-300 pt-1 leading-snug pl-1">{inspectError}</p>
              {/if}
            </div>
          {/if}
        {/if}

        <!-- Stream / Upload Progress Indicator -->
        {#if isUploading && (uploadMode === 'files' || uploadProgress > 0)}
          <div class="space-y-1.5 pt-1">
            <div class="flex justify-between text-xs text-white/50">
              <span class="truncate max-w-[280px] font-mono text-[11px]">{statusMessage}</span>
              <span class="font-mono text-white/80 text-[11px]">{uploadMode === 'files' ? `${uploadProgress}%` : ''}</span>
            </div>
            <div class="w-full bg-white/[0.06] rounded-full h-1 overflow-hidden border border-white/[0.08]">
              <div
                class="bg-white/80 h-full transition-all duration-300 ease-out rounded-full"
                style="width: {uploadProgress > 0 ? uploadProgress : 100}%"
              ></div>
            </div>
          </div>
        {/if}
      </div>

      <!-- Footer Actions -->
      <div class="px-6 py-4 border-t border-white/[0.08] flex justify-end gap-2.5">
        <button
          type="button"
          on:click={forceClose}
          disabled={isUploading}
          class="liquid-btn-secondary px-4 py-2 rounded-xl text-xs text-white/60 hover:text-white transition-all spring-tap cursor-pointer disabled:opacity-40"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={handleMasterUpload}
          disabled={!canUpload || isUploading}
          class="liquid-btn-primary px-5 py-2 rounded-xl text-xs font-medium text-white transition-all disabled:opacity-40 cursor-pointer flex items-center gap-2 spring-tap"
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
  /* Apple Liquid-Glass Modal Styling */
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

  .liquid-dropzone {
    background: rgba(255, 255, 255, 0.02);
    border: 1px dashed rgba(255, 255, 255, 0.16);
  }

  .liquid-dropzone:hover {
    background: rgba(255, 255, 255, 0.05);
    border-color: rgba(255, 255, 255, 0.3);
  }

  .liquid-card {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow: inset 0 1px 0 0 rgba(255, 255, 255, 0.1);
  }

  .liquid-input {
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.4);
  }

  .liquid-input:focus {
    border-color: rgba(255, 255, 255, 0.25);
    box-shadow: 
      inset 0 1px 2px rgba(0, 0, 0, 0.4),
      0 0 0 1px rgba(255, 255, 255, 0.15);
  }

  .liquid-segmented {
    background: rgba(0, 0, 0, 0.4);
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.4);
  }

  .liquid-seg-active {
    background: rgba(255, 255, 255, 0.14);
    border: 1px solid rgba(255, 255, 255, 0.18);
    box-shadow:
      0 2px 6px rgba(0, 0, 0, 0.25),
      inset 0 1px 0 rgba(255, 255, 255, 0.3);
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
    border: 1px solid rgba(255, 255, 255, 0.12);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.25);
  }

  .liquid-tag {
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.15);
    color: rgba(255, 255, 255, 0.85);
  }

  .liquid-btn-primary {
    background: rgba(255, 255, 255, 0.15);
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

  .liquid-btn-secondary {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .liquid-btn-secondary:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.14);
  }
</style>