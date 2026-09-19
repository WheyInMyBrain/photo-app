<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';
  import { authStore } from '$lib/stores/authStore';

  export let isOpen = false;
  export let initialFiles: File[] = [];

  const dispatch = createEventDispatcher<{
    close: void;
    uploaded: { count: number };
  }>();

  type CandidateItem = {
    id: string;
    media_type: string;
    thumbnail_url?: string;
    thumbnail_base64?: string;
    high_res_url: string;
    audio_url?: string;
    suggested_filename: string;
  };

  type InspectPreview = {
    suggested_folder: string;
    platform: string;
    author: string;
    caption: string;
    total_items: number;
    items: CandidateItem[];
  };

  // UI State
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
  const CHUNK_THRESHOLD_BYTES = 75 * 1024 * 1024;
  const CHUNK_SIZE_BYTES = 20 * 1024 * 1024;

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

  // --- LOCAL FILE LOGIC ---
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

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
  }

  async function uploadDirect(file: File) {
    const formData = new FormData();
    formData.append('folder', folderPath.trim() || 'root');
    formData.append('file', file);
    const res = await fetch('/api/upload', { method: 'POST', body: formData });
    if (res.status === 401) throw new Error('Session expired.');
    if (!res.ok) throw new Error(await res.text());
  }

  async function uploadChunked(file: File) {
    const uploadId = crypto.randomUUID();
    const totalChunks = Math.ceil(file.size / CHUNK_SIZE_BYTES);

    for (let chunkIdx = 0; chunkIdx < totalChunks; chunkIdx++) {
      const start = chunkIdx * CHUNK_SIZE_BYTES;
      const end = Math.min(file.size, start + CHUNK_SIZE_BYTES);
      const slice = file.slice(start, end);

      statusMessage = `Uploading ${file.name} (Part ${chunkIdx + 1}/${totalChunks})...`;
      const params = new URLSearchParams({
        upload_id: uploadId,
        chunk_index: chunkIdx.toString(),
        chunk_size: CHUNK_SIZE_BYTES.toString(),
        total_chunks: totalChunks.toString(),
      });

      const res = await fetch(`/api/upload/chunk?${params.toString()}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/octet-stream' },
        body: slice,
      });

      if (!res.ok) throw new Error(`Failed uploading chunk ${chunkIdx + 1}`);
    }

    statusMessage = `Processing ${file.name}...`;
    const finalizeParams = new URLSearchParams({
      upload_id: uploadId,
      file_name: file.name,
      folder: folderPath.trim() || 'root',
    });

    const finalizeRes = await fetch(`/api/upload/chunk/finalize?${finalizeParams.toString()}`, {
      method: 'POST',
    });
    if (!finalizeRes.ok) throw new Error(`Failed to finalize ${file.name}`);
  }

  // --- EXTERNAL LINK LOGIC ---
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
        throw new Error(errorText || `Server responded with ${res.status}`);
      }

      const data = await res.json();

      // Support either enum wrapped ({ "Preview": { ... } }) or direct struct representation
      const previewData: InspectPreview | null = data.Preview ?? (data.items ? data : null);

      if (previewData && Array.isArray(previewData.items)) {
        linkPreview = previewData;
        folderPath = previewData.suggested_folder || '';
        selectedLinkItems = new Set(previewData.items.map((i: CandidateItem) => i.id));
      } else if (data.Committed) {
        // Handled if server had auto-committed single items
        dispatch('uploaded', { count: data.Committed.total_uploaded || 1 });
        forceClose();
      } else {
        throw new Error('No media items could be found at this link.');
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
    uploadProgress = 10;
    statusMessage = 'Importing selected media...';

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

  // --- MASTER UPLOAD HANDLER ---
  async function handleMasterUpload() {
    if (isUploading || !canUpload) return;

    if (uploadMode === 'link') {
      await commitLink();
      return;
    }

    // Local files path
    isUploading = true;
    uploadProgress = 5;
    const totalCount = stagedFiles.length;

    try {
      for (let i = 0; i < stagedFiles.length; i++) {
        const file = stagedFiles[i];
        if (file.size > CHUNK_THRESHOLD_BYTES) {
          statusMessage = `Chunking large file: ${file.name}...`;
          await uploadChunked(file);
        } else {
          statusMessage = `Uploading ${file.name}...`;
          await uploadDirect(file);
        }
        uploadProgress = Math.round(((i + 1) / totalCount) * 100);
      }

      statusMessage = 'Upload complete!';
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
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-xs p-4 select-none"
    role="dialog"
    aria-modal="true"
  >
    <div
      class="bg-neutral-900 border border-neutral-800 rounded-2xl w-full max-w-lg shadow-2xl flex flex-col max-h-[85vh] overflow-hidden"
    >
      <!-- Header -->
      <div class="px-5 py-4 border-b border-neutral-800 flex items-center justify-between bg-neutral-900">
        <div>
          <h2 class="text-sm font-bold text-white flex items-center gap-2">
            Add Media
          </h2>
          <p class="text-[11px] text-neutral-400 mt-0.5">
            Save files to your personal library.
          </p>
        </div>
        <button
          type="button"
          on:click={forceClose}
          disabled={isUploading}
          class="text-neutral-500 hover:text-white p-1 rounded-md transition-colors cursor-pointer disabled:opacity-30"
        >
          ✕
        </button>
      </div>

      <!-- Mode Switcher -->
      <div class="px-5 pt-3 bg-neutral-900">
        <div class="flex p-1 bg-neutral-950 border border-neutral-800 rounded-lg">
          <button
            class="flex-1 text-xs py-1.5 rounded-md transition-colors font-medium {uploadMode === 'files' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300'}"
            on:click={() => { uploadMode = 'files'; inspectError = ''; }}
            disabled={isUploading}
          >
            Device Files
          </button>
          <button
            class="flex-1 text-xs py-1.5 rounded-md transition-colors font-medium {uploadMode === 'link' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300'}"
            on:click={() => { uploadMode = 'link'; inspectError = ''; }}
            disabled={isUploading}
          >
            Web Link
          </button>
        </div>
      </div>

      <!-- Body / Form -->
      <div class="p-5 space-y-4 overflow-y-auto flex-1">
        <!-- Target Album/Folder Input (Shared) -->
        <div class="space-y-1.5">
          <label for="upload-folder-input" class="text-[11px] uppercase font-semibold text-neutral-400 tracking-wider">
            Destination Album / Folder
          </label>
          <input
            id="upload-folder-input"
            type="text"
            list="folder-suggestions"
            bind:value={folderPath}
            placeholder="root (or enter e.g. vacation/day1)..."
            disabled={isUploading || linkPreview !== null}
            class="w-full bg-neutral-950 border border-neutral-800 focus:border-purple-500 rounded-lg px-3 py-2 text-xs text-white placeholder-neutral-600 outline-none transition-colors disabled:opacity-50"
          />
        </div>

        {#if uploadMode === 'files'}
          <!-- LOCAL FILES TAB -->
          <div class="space-y-2">
            <div class="flex items-center justify-between">
              <span class="text-[11px] uppercase font-semibold text-neutral-400 tracking-wider">
                Files ({stagedFiles.length})
              </span>
              <button
                type="button"
                on:click={() => fileInputEl?.click()}
                disabled={isUploading}
                class="text-xs text-purple-400 hover:text-purple-300 font-medium cursor-pointer"
              >
                + Add more files
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
                class="w-full border-2 border-dashed border-neutral-800 hover:border-neutral-700 rounded-xl p-8 flex flex-col items-center justify-center gap-1.5 text-center cursor-pointer transition-colors"
              >
                <div class="text-2xl mb-1">📁</div>
                <span class="text-xs text-neutral-300 font-medium">Click to select files</span>
                <span class="text-[10px] text-neutral-500">Supports JPG, PNG, WEBP, MP4, HEIC</span>
              </button>
            {:else}
              <div class="space-y-1.5 max-h-56 overflow-y-auto pr-1">
                {#each stagedFiles as file, idx}
                  <div
                    class="flex items-center justify-between p-2 bg-neutral-950/80 border border-neutral-800/80 rounded-lg text-xs"
                  >
                    <div class="truncate mr-3">
                      <div class="text-neutral-200 font-medium truncate max-w-[280px] flex items-center gap-1.5">
                        <span class="truncate">{file.name}</span>
                        {#if file.size > CHUNK_THRESHOLD_BYTES}
                          <span class="text-[9px] bg-blue-950/80 text-blue-300 border border-blue-800/50 px-1 rounded">CHUNKED</span>
                        {/if}
                      </div>
                      <div class="text-[10px] text-neutral-500">
                        {formatBytes(file.size)} • {file.type || 'unknown'}
                      </div>
                    </div>
                    {#if !isUploading}
                      <button
                        type="button"
                        on:click={() => removeFile(idx)}
                        class="text-neutral-500 hover:text-red-400 p-1 cursor-pointer text-xs"
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
          <!-- WEB LINK TAB -->
          {#if !linkPreview}
            <div class="space-y-3">
              <div class="space-y-1.5">
                <label for="link-url-input" class="text-[11px] uppercase font-semibold text-neutral-400 tracking-wider">
                  Post URL
                </label>
                <div class="flex gap-2">
                  <input
                    id="link-url-input"
                    type="url"
                    bind:value={linkUrl}
                    placeholder="https://instagram.com/p/... or https://reddit.com/r/..."
                    disabled={isUploading}
                    on:keydown={(e) => e.key === 'Enter' && inspectLink()}
                    class="flex-1 bg-neutral-950 border border-neutral-800 focus:border-purple-500 rounded-lg px-3 py-2 text-xs text-white placeholder-neutral-600 outline-none transition-colors"
                  />
                  <button
                    type="button"
                    on:click={inspectLink}
                    disabled={!linkUrl.trim() || isUploading}
                    class="bg-neutral-800 hover:bg-neutral-700 text-white px-4 py-2 rounded-lg text-xs font-medium transition-colors disabled:opacity-50 cursor-pointer flex items-center gap-1.5 min-w-[75px] justify-center"
                  >
                    {#if isUploading}
                      <span class="inline-block w-3 h-3 border-2 border-white/20 border-t-white rounded-full animate-spin"></span>
                    {:else}
                      <span>Inspect</span>
                    {/if}
                  </button>
                </div>
                {#if inspectError}
                  <p class="text-[11px] text-red-400 pt-1 leading-snug">{inspectError}</p>
                {/if}
              </div>
            </div>
          {:else}
            <!-- Preview Grid -->
            <div class="space-y-3">
              <div class="flex items-center justify-between text-xs">
                <div>
                  <span class="text-white font-medium">{linkPreview.author}</span>
                  <span class="text-neutral-500"> on {linkPreview.platform} ({linkPreview.items.length} items)</span>
                </div>
                <button
                  type="button"
                  on:click={() => { linkPreview = null; inspectError = ''; }}
                  disabled={isUploading}
                  class="text-purple-400 hover:text-purple-300 font-medium cursor-pointer"
                >
                  Change Link
                </button>
              </div>

              {#if linkPreview.caption}
                <p class="text-[11px] text-neutral-400 line-clamp-2 italic border-l-2 border-neutral-700 pl-2">
                  "{linkPreview.caption}"
                </p>
              {/if}

              <div class="grid grid-cols-3 gap-2 max-h-56 overflow-y-auto pr-0.5">
                {#each linkPreview.items as item}
                  <button
                    type="button"
                    disabled={isUploading}
                    on:click={() => toggleLinkItem(item.id)}
                    class="relative aspect-square bg-neutral-950 border border-neutral-800 rounded-lg overflow-hidden cursor-pointer group focus:outline-none"
                  >
                    {#if item.thumbnail_base64 || item.thumbnail_url}
                      <img
                        src={getThumbnailSrc(item)}
                        alt="Preview"
                        class="w-full h-full object-cover transition-opacity {selectedLinkItems.has(item.id) ? 'opacity-100' : 'opacity-40 group-hover:opacity-75'}"
                      />
                    {:else}
                      <div class="w-full h-full flex flex-col items-center justify-center text-neutral-500 text-xs gap-1 {selectedLinkItems.has(item.id) ? 'text-white' : ''}">
                        <span>{item.media_type === 'video' ? '🎬' : '🖼️'}</span>
                        <span class="capitalize text-[10px]">{item.media_type}</span>
                      </div>
                    {/if}

                    <!-- Selection Indicator -->
                    <div class="absolute top-1.5 left-1.5 w-4 h-4 rounded-full border flex items-center justify-center transition-colors {selectedLinkItems.has(item.id) ? 'bg-purple-600 border-purple-500 text-white' : 'bg-black/50 border-white/40'}">
                      {#if selectedLinkItems.has(item.id)}
                        <svg class="w-2.5 h-2.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="3" d="M5 13l4 4L19 7" /></svg>
                      {/if}
                    </div>

                    <!-- Media Type Badge -->
                    <span class="absolute bottom-1 right-1 text-[9px] uppercase px-1 py-0.5 rounded bg-black/70 text-neutral-300 font-mono">
                      {item.media_type}
                    </span>
                  </button>
                {/each}
              </div>

              {#if inspectError}
                <p class="text-[11px] text-red-400 pt-1 leading-snug">{inspectError}</p>
              {/if}
            </div>
          {/if}
        {/if}

        <!-- Progress Indicator (Shared) -->
        {#if isUploading && (uploadMode === 'files' || uploadProgress > 0)}
          <div class="space-y-1.5 pt-2">
            <div class="flex justify-between text-xs text-neutral-400">
              <span class="truncate max-w-[260px]">{statusMessage}</span>
              <span>{uploadMode === 'files' ? `${uploadProgress}%` : ''}</span>
            </div>
            <div class="w-full bg-neutral-800 rounded-full h-1.5 overflow-hidden">
              <div
                class="bg-purple-600 h-full transition-all duration-300 ease-out"
                style="width: {uploadProgress > 0 ? uploadProgress : 100}%"
              ></div>
            </div>
          </div>
        {/if}
      </div>

      <!-- Footer Buttons -->
      <div class="px-5 py-3 border-t border-neutral-800 flex justify-end gap-2 bg-neutral-900/50">
        <button
          type="button"
          on:click={forceClose}
          disabled={isUploading}
          class="px-4 py-1.5 rounded-lg text-xs text-neutral-400 hover:text-white border border-neutral-800 hover:bg-neutral-800 transition-colors cursor-pointer disabled:opacity-40"
        >
          Cancel
        </button>
        <button
          type="button"
          on:click={handleMasterUpload}
          disabled={!canUpload || isUploading}
          class="px-4 py-1.5 rounded-lg text-xs font-semibold bg-purple-600 hover:bg-purple-500 text-white transition-colors disabled:opacity-40 cursor-pointer flex items-center gap-1.5"
        >
          {#if isUploading}
            <span class="inline-block w-3 h-3 border-2 border-white/20 border-t-white rounded-full animate-spin"></span>
            <span>Processing...</span>
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