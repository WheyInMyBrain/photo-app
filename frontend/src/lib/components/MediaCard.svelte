<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import type { MediaItem } from '$lib/types/media';

  export let asset: MediaItem;
  export let isSelected = false;

  const dispatch = createEventDispatcher<{
    select: MouseEvent;
    open: void;
  }>();

  function getDaysRemaining(deletedAt: string | null): number {
    if (!deletedAt) return 30;
    const diffMs = Date.now() - new Date(deletedAt).getTime();
    const daysPassed = Math.floor(diffMs / (1000 * 60 * 60 * 24));
    return Math.max(0, 30 - daysPassed);
  }
</script>

<div
  role="button"
  tabindex="0"
  on:click={() => dispatch('open')}
  on:keydown={(e) => {
    if (e.key === 'Enter') dispatch('open');
  }}
  class="group relative aspect-square bg-neutral-900 rounded-lg overflow-hidden border transition-all cursor-pointer {isSelected ? 'border-purple-500 ring-2 ring-purple-500/40' : 'border-neutral-800/80 hover:border-neutral-700'}"
>
  <img
    src="/{asset.thumb_path}"
    alt={asset.file_name}
    loading="lazy"
    decoding="async"
    class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-200 pointer-events-none"
  />

  <!-- Multi-Select Checkbox -->
  <button
    type="button"
    on:click={(e) => dispatch('select', e)}
    class="absolute top-1.5 left-1.5 w-5 h-5 rounded-md flex items-center justify-center transition-all z-20 cursor-pointer {isSelected ? 'bg-purple-600 text-white' : 'bg-black/40 text-transparent hover:bg-black/70 hover:text-neutral-400 border border-neutral-700/60'}"
    title="Select photo"
  >
    <span class="text-xs font-bold leading-none">✓</span>
  </button>

  <!-- Trash Badge -->
  {#if asset.deleted_at}
    <div class="absolute bottom-1.5 left-1.5 bg-red-950/90 border border-red-800/80 px-1.5 py-0.5 rounded text-[9px] text-red-300 font-mono z-10 shadow">
      🗑️ {getDaysRemaining(asset.deleted_at)}d left
    </div>
  {/if}

  <!-- Favorite Badge -->
  {#if asset.is_favorite}
    <div class="absolute top-1.5 right-1.5 bg-black/60 p-1 rounded-full text-amber-400 text-[10px] leading-none">★</div>
  {/if}

  <!-- Video Duration Badge -->
  {#if asset.duration_seconds}
    <div class="absolute bottom-1 right-1 bg-black/75 px-1 py-0.5 rounded text-[9px] text-white font-mono">
      {Math.floor(asset.duration_seconds / 60)}:{Math.floor(asset.duration_seconds % 60).toString().padStart(2, '0')}
    </div>
  {/if}
</div>