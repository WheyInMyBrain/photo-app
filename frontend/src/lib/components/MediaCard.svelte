<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import type { MediaItem } from '$lib/types/media';

  export let asset: MediaItem;
  export let isSelected = false;

  const dispatch = createEventDispatcher<{
    select: MouseEvent;
    open: void;
  }>();

  let isHovered = false;
  let videoEl: HTMLVideoElement | null = null;
  let hoverTimeout: ReturnType<typeof setTimeout> | null = null;

  $: isMotionMedia =
    Boolean(asset.mime_type?.startsWith('video/')) || asset.mime_type === 'image/gif';

  // Normalize storage paths for static and static-proxied routes
  function resolveUrl(path: string | undefined | null): string {
    if (!path) return '';
    if (path.startsWith('http')) return path;
    const clean = path.startsWith('/') ? path.slice(1) : path;
    return clean.startsWith('users/') ? `/${clean}` : `/thumbs/${clean}`;
  }

  $: thumbUrl = resolveUrl(asset.thumb_path);
  $: motionUrl = isMotionMedia && asset.thumb_path
    ? resolveUrl(asset.thumb_path.replace(/_thumb\.webp$/, '_motion.mp4'))
    : null;

  function handleMouseEnter() {
    if (!isMotionMedia || !motionUrl) return;
    hoverTimeout = setTimeout(() => {
      isHovered = true;
    }, 150);
  }

  function handleMouseLeave() {
    if (hoverTimeout) clearTimeout(hoverTimeout);
    isHovered = false;
    if (videoEl) {
      videoEl.pause();
      videoEl.currentTime = 0;
    }
  }

  function getDaysRemaining(deletedAt: string | null): number {
    if (!deletedAt) return 30;
    const diffMs = Date.now() - new Date(deletedAt).getTime();
    const daysPassed = Math.floor(diffMs / (1000 * 60 * 60 * 24));
    return Math.max(0, 30 - daysPassed);
  }

  function formatDuration(sec: number | null | undefined): string {
    if (!sec || sec <= 0) return '';
    const m = Math.floor(sec / 60);
    const s = Math.floor(sec % 60);
    return `${m}:${s < 10 ? '0' : ''}${s}`;
  }
</script>

<div
  role="button"
  tabindex="0"
  on:click={() => dispatch('open')}
  on:keydown={(e) => e.key === 'Enter' && dispatch('open')}
  on:mouseenter={handleMouseEnter}
  on:mouseleave={handleMouseLeave}
  class="group relative aspect-square bg-neutral-900 rounded-lg overflow-hidden border transition-all cursor-pointer {isSelected ? 'border-purple-500 ring-2 ring-purple-500/40' : 'border-neutral-800/80 hover:border-neutral-700'}"
>
  <!-- 1. Default Static WebP Thumbnail -->
  <img
    src={thumbUrl}
    alt={asset.file_name}
    loading="lazy"
    decoding="async"
    class="w-full h-full object-cover transition-transform duration-300 group-hover:scale-105 pointer-events-none {isHovered && isMotionMedia ? 'opacity-0' : 'opacity-100'}"
  />

  <!-- 2. Dynamic 480p Motion Hover Loop -->
  {#if isHovered && motionUrl}
    <video
      bind:this={videoEl}
      src={motionUrl}
      autoplay
      loop
      muted
      playsinline
      disablepictureinpicture
      class="absolute inset-0 w-full h-full object-cover z-10 pointer-events-none"
    />
  {/if}

  <!-- Multi-Select Checkbox -->
  <button
    type="button"
    on:click|stopPropagation={(e) => dispatch('select', e)}
    class="absolute top-1.5 left-1.5 w-5 h-5 rounded-md flex items-center justify-center transition-all z-30 cursor-pointer {isSelected ? 'bg-purple-600 text-white' : 'bg-black/40 text-transparent hover:bg-black/70 hover:text-neutral-400 border border-neutral-700/60'}"
    title="Select media"
  >
    <span class="text-xs font-bold leading-none">✓</span>
  </button>

  <!-- Favorite Badge -->
  {#if asset.is_favorite}
    <div class="absolute top-1.5 right-1.5 bg-black/60 p-1 rounded-full text-amber-400 text-[10px] leading-none z-20 pointer-events-none">
      ★
    </div>
  {/if}

  <!-- Trash Badge -->
  {#if asset.deleted_at}
    <div class="absolute bottom-1.5 left-1.5 bg-red-950/90 border border-red-800/80 px-1.5 py-0.5 rounded text-[9px] text-red-300 font-mono z-20 shadow pointer-events-none">
      🗑️ {getDaysRemaining(asset.deleted_at)}d left
    </div>
  <!-- GIF Badge (Only shown when not deleted) -->
  {:else if asset.mime_type === 'image/gif'}
    <div class="absolute bottom-1.5 left-1.5 bg-black/75 px-1.5 py-0.5 rounded text-[9px] text-neutral-200 font-mono font-medium tracking-wider z-20 pointer-events-none">
      GIF
    </div>
  {/if}

  <!-- Video Duration Badge -->
  {#if asset.duration_seconds}
    <div class="absolute bottom-1.5 right-1.5 bg-black/75 px-1.5 py-0.5 rounded text-[9px] text-white font-mono z-20 pointer-events-none flex items-center gap-1">
      <span>▶</span>
      <span>{formatDuration(asset.duration_seconds)}</span>
    </div>
  {/if}
</div>