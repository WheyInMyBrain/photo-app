<!-- photo-app/frontend/src/lib/components/UploadProgressWidget.svelte -->
<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import { uploadProgressStore } from '$lib/stores/uploadProgressStore';
  import { formatBytes } from '$lib/utils/uploader';

  $: state =$uploadProgressStore;

  $: overallPercent = state.totalBytes > 0 
    ? Math.min(100, Math.round((state.loadedBytes / state.totalBytes) * 100)) 
    : 0;

  $: etaString = (() => {
    if (state.loadedBytes <= 0 || state.completedFiles >= state.totalFiles) return 'Almost done...';
    const elapsedSec = (Date.now() - state.startTime) / 1000;
    if (elapsedSec < 1) return 'Calculating...';
    const bytesPerSec = state.loadedBytes / elapsedSec;
    const remainingBytes = state.totalBytes - state.loadedBytes;
    const remainingSec = Math.round(remainingBytes / bytesPerSec);

    if (remainingSec < 60) return `${remainingSec}s left`;
    const mins = Math.floor(remainingSec / 60);
    const secs = remainingSec % 60;
    return `${mins}m ${secs}s left`;
  })();
</script>

{#if state.isOpen}
  <div
    transition:fly={{ y: 20, duration: 200 }}
    class="fixed bottom-5 right-5 z-50 w-80 sm:w-96 liquid-toast rounded-3xl p-4 flex flex-col shadow-2xl text-[var(--text-main)] select-none border border-[var(--border-glass)]"
  >
    <!-- Header -->
    <div class="flex items-center justify-between pb-3 border-b border-[var(--border-glass)]">
      <div class="flex items-center gap-2">
        <div class="w-2.5 h-2.5 rounded-full bg-purple-500 animate-pulse"></div>
        <span class="text-xs font-semibold tracking-tight text-[var(--text-main)]">
          {state.completedFiles === state.totalFiles ? 'Upload Complete' : `Uploading (${state.completedFiles}/${state.totalFiles})`}
        </span>
      </div>

      <div class="flex items-center gap-1.5">
        <button
          type="button"
          on:click={() => uploadProgressStore.toggleMinimize()}
          class="liquid-icon-btn w-6 h-6 rounded-full flex items-center justify-center text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer text-xs"
          title={state.isMinimized ? 'Expand' : 'Minimize'}
        >
          {state.isMinimized ? '▲' : '▼'}
        </button>
        <button
          type="button"
          on:click={() => uploadProgressStore.close()}
          class="liquid-icon-btn w-6 h-6 rounded-full flex items-center justify-center text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer"
          title="Close"
        >
          ✕
        </button>
      </div>
    </div>

    <!-- Expanded Body -->
    {#if !state.isMinimized}
      <div class="py-3 space-y-3">
        <!-- Overall Stats Bar -->
        <div class="flex justify-between text-[11px] text-[var(--text-muted)] font-mono">
          <span>{formatBytes(state.loadedBytes)} / {formatBytes(state.totalBytes)}</span>
          <span class="text-purple-600 dark:text-purple-400 font-semibold">{etaString}</span>
        </div>

        <!-- Overall Progress Track -->
        <div class="w-full bg-[var(--pill-bg)] rounded-full h-1.5 overflow-hidden border border-[var(--border-glass)]">
          <div
            class="bg-gradient-to-r from-purple-600 to-emerald-500 h-full transition-all duration-300 rounded-full"
            style="width: {overallPercent}%"
          ></div>
        </div>

        <!-- Current Active File Slots (Simultaneous Concurrent Uploads) -->
        <div class="space-y-2 pt-1 max-h-48 overflow-y-auto no-scrollbar">
          {#each state.activeUploads as item (item.fileIdx)}
            <div class="liquid-card rounded-2xl p-2.5 space-y-1.5 text-xs">
              <div class="flex items-center justify-between gap-2">
                <span class="font-medium truncate text-[var(--text-main)]">{item.name}</span>
                <span class="font-mono text-[10px] text-[var(--text-muted)] flex-shrink-0">{item.percent}%</span>
              </div>
              <div class="w-full bg-[var(--pill-bg)] rounded-full h-1 overflow-hidden">
                <div
                  class="bg-purple-600 h-full transition-all duration-150 rounded-full"
                  style="width: {item.percent}%"
                ></div>
              </div>
            </div>
          {/each}
        </div>

        {#if state.error}
          <div class="text-rose-500 text-[11px] bg-rose-500/10 p-2 rounded-xl">
            {state.error}
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .liquid-toast {
    background: var(--bg-surface-elevated);
    backdrop-filter: blur(40px) saturate(190%);
    -webkit-backdrop-filter: blur(40px) saturate(190%);
    box-shadow: 0 25px 60px var(--dock-shadow), inset 0 1px 0 var(--border-specular);
  }

  .liquid-card {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
  }

  .liquid-icon-btn {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
  }
  .liquid-icon-btn:hover {
    background: var(--dock-bg-hover);
  }

  .no-scrollbar::-webkit-scrollbar { display: none; }
  .no-scrollbar { -ms-overflow-style: none; scrollbar-width: none; }
</style>