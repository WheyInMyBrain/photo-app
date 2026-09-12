<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';

  export let isOpen = false;
  export let initialFiles: File[] = [];
  export let isPrivate = false; // Synchronized initial value from +layout.svelte

  const dispatch = createEventDispatcher<{
    close: void;
    uploaded: { count: number };
  }>();

  let stagedFiles: File[] = [];
  let folderPath = '';
  let uploadIsPrivate = false;
  let isUploading = false;
  let uploadProgress = 0;
  let statusMessage = '';
  let fileInputEl: HTMLInputElement;
  let existingFolders: string[] = [];

  // Sync internal toggle whenever modal opens or parent changes
  $: if (isOpen) {
    uploadIsPrivate = isPrivate;
  }

  $: if (initialFiles && initialFiles.length > 0) {
    addFiles(initialFiles);
    initialFiles = [];
  }

  onMount(async () => {
    try {
      const res = await fetch('/api/media/filters');
      if (res.ok) {
        const data = await res.json();
        if (Array.isArray(data.albums)) {
          existingFolders = data.albums.map((a: any) => a.value || a.label).filter(Boolean);
        }
      }
    } catch {
      existingFolders = ['college', 'college/sem1', 'vacation', 'family'];
    }
  });

  function addFiles(files: FileList | File[]) {
    const valid = Array.from(files).filter(
      (f) => f.type.startsWith('image/') || f.type.startsWith('video/')
    );
    stagedFiles = [...stagedFiles, ...valid];
  }

  function removeFile(index: number) {
    stagedFiles = stagedFiles.filter((_, i) => i !== index);
    if (stagedFiles.length === 0) forceClose();
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

  async function uploadAll() {
    if (stagedFiles.length === 0 || isUploading) return;

    isUploading = true;
    statusMessage = 'Uploading...';
    uploadProgress = 15;

    const formData = new FormData();
    formData.append('folder', folderPath.trim() || 'root');
    formData.append('is_private', uploadIsPrivate ? 'true' : 'false');

    for (const file of stagedFiles) {
      formData.append('file', file);
    }

    try {
      uploadProgress = 65;
      statusMessage = 'Saving to storage & indexing...';

      const res = await fetch('/api/upload', {
        method: 'POST',
        body: formData,
      });

      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || `HTTP ${res.status}`);
      }

      uploadProgress = 100;
      statusMessage = 'Upload complete!';

      const uploadedCount = stagedFiles.length;
      dispatch('uploaded', { count: uploadedCount });
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
    dispatch('close');
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !isUploading) {
      forceClose();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- Explicitly closed <option> tag prevents Vite compiler warning -->
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
      <div class="px-5 py-4 border-b border-neutral-800 flex items-center justify-between">
        <div>
          <h2 class="text-sm font-bold text-white flex items-center gap-2">
            Upload Media
            {#if uploadIsPrivate}
              <span class="text-[10px] bg-purple-950/80 text-purple-300 border border-purple-800/60 px-1.5 py-0.2 rounded font-mono font-medium">
                TO PRIVATE VAULT
              </span>
            {:else}
              <span class="text-[10px] bg-neutral-800 text-neutral-400 border border-neutral-700/60 px-1.5 py-0.2 rounded font-mono font-medium">
                TO PUBLIC
              </span>
            {/if}
          </h2>
          <p class="text-[11px] text-neutral-400 mt-0.5">
            Images and videos will be processed with background AI indexing.
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

      <!-- Body / Form -->
      <div class="p-5 space-y-4 overflow-y-auto flex-1">
        <!-- Privacy Destination Toggle -->
        <div class="space-y-1.5">
          <span class="text-[11px] uppercase font-semibold text-neutral-400 tracking-wider">
            Privacy Realm
          </span>
          <div class="grid grid-cols-2 bg-neutral-950 p-1 rounded-lg border border-neutral-800 gap-1 text-xs">
            <button
              type="button"
              disabled={isUploading}
              on:click={() => (uploadIsPrivate = false)}
              class="py-1.5 rounded-md text-center cursor-pointer transition-all flex items-center justify-center gap-1.5 {!uploadIsPrivate ? 'bg-neutral-800 text-white font-medium shadow-xs' : 'text-neutral-500 hover:text-neutral-300'}"
            >
              <span>📷</span>
              <span>Public Gallery</span>
            </button>
            <button
              type="button"
              disabled={isUploading}
              on:click={() => (uploadIsPrivate = true)}
              class="py-1.5 rounded-md text-center cursor-pointer transition-all flex items-center justify-center gap-1.5 {uploadIsPrivate ? 'bg-purple-600 text-white font-medium shadow-xs' : 'text-neutral-500 hover:text-purple-400'}"
            >
              <span>🔒</span>
              <span>Private Vault</span>
            </button>
          </div>
        </div>

        <!-- Target Album/Folder Input -->
        <div class="space-y-1.5">
          <label for="upload-folder-input" class="text-[11px] uppercase font-semibold text-neutral-400 tracking-wider">
            Destination Album / Folder
          </label>
          <input
            id="upload-folder-input"
            type="text"
            list="folder-suggestions"
            bind:value={folderPath}
            placeholder="root (or type e.g. college/sem1)..."
            disabled={isUploading}
            class="w-full bg-neutral-950 border border-neutral-800 focus:border-purple-500 rounded-lg px-3 py-2 text-xs text-white placeholder-neutral-600 outline-none transition-colors"
          />
        </div>

        <!-- Staged Files List -->
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
              <span class="text-[10px] text-neutral-500">Supports JPG, PNG, WEBP, MP4, MOV</span>
            </button>
          {:else}
            <div class="space-y-1.5 max-h-56 overflow-y-auto pr-1">
              {#each stagedFiles as file, idx}
                <div
                  class="flex items-center justify-between p-2 bg-neutral-950/80 border border-neutral-800/80 rounded-lg text-xs"
                >
                  <div class="truncate mr-3">
                    <div class="text-neutral-200 font-medium truncate max-w-[280px]">
                      {file.name}
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

        <!-- Progress Indicator -->
        {#if isUploading}
          <div class="space-y-1.5 pt-2">
            <div class="flex justify-between text-xs text-neutral-400">
              <span>{statusMessage}</span>
              <span>{uploadProgress}%</span>
            </div>
            <div class="w-full bg-neutral-800 rounded-full h-1.5 overflow-hidden">
              <div
                class="bg-purple-600 h-full transition-all duration-300 ease-out"
                style="width: {uploadProgress}%"
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
          on:click={uploadAll}
          disabled={isUploading || stagedFiles.length === 0}
          class="px-4 py-1.5 rounded-lg text-xs font-semibold bg-purple-600 hover:bg-purple-500 text-white transition-colors disabled:opacity-40 cursor-pointer"
        >
          {isUploading ? 'Uploading...' : `Upload ${stagedFiles.length} item${stagedFiles.length === 1 ? '' : 's'}`}
        </button>
      </div>
    </div>
  </div>
{/if}