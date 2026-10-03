<!-- photo-app/frontend/src/lib/components/TimelineScrubber.svelte -->
<script context="module" lang="ts">
  export interface ScrubberMarker {
    label: string;
    year: string;
    index: number;
    count?: number;
    latestCapturedAt?: string;
  }
</script>

<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { scale } from 'svelte/transition';

  export let markers: ScrubberMarker[] = [];

  const dispatch = createEventDispatcher<{
    jump: { marker: ScrubberMarker; index: number };
  }>();

  let railEl: HTMLElement | null = null;
  let isScrubbing = false;
  let activeIndex = 0;
  let tooltipY = 0;
  let lastHapticIndex = -1;

  $: activeMarker = markers[activeIndex] ?? markers[0];

  function getIndexFromPointer(e: PointerEvent): number {
    if (!railEl || markers.length === 0) return 0;
    const rect = railEl.getBoundingClientRect();
    const clampedY = Math.max(0, Math.min(e.clientY - rect.top, rect.height));
    const ratio = clampedY / rect.height;
    const rawIdx = Math.round(ratio * (markers.length - 1));
    return Math.max(0, Math.min(markers.length - 1, rawIdx));
  }

  function handlePointerDown(e: PointerEvent) {
    if (markers.length < 2 || !railEl) return;
    isScrubbing = true;

    try {
      railEl.setPointerCapture(e.pointerId);
    } catch {}

    updateScrub(e);
  }

  function handlePointerMove(e: PointerEvent) {
    if (!isScrubbing) return;
    updateScrub(e);
  }

  function updateScrub(e: PointerEvent) {
    const nextIdx = getIndexFromPointer(e);
    activeIndex = nextIdx;

    if (railEl) {
      const rect = railEl.getBoundingClientRect();
      tooltipY = Math.max(rect.top + 24, Math.min(e.clientY, rect.bottom - 24));
    }

    if (nextIdx !== lastHapticIndex) {
      lastHapticIndex = nextIdx;
      if (typeof navigator !== 'undefined' && 'vibrate' in navigator) {
        navigator.vibrate(6);
      }
      const marker = markers[nextIdx];
      dispatch('jump', { marker, index: marker.index });
    }
  }

  function handlePointerUp(e: PointerEvent) {
    if (!isScrubbing) return;
    isScrubbing = false;
    lastHapticIndex = -1;

    try {
      railEl?.releasePointerCapture(e.pointerId);
    } catch {}
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (markers.length < 2) return;
    let nextIdx = activeIndex;

    if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') {
      nextIdx = Math.max(0, activeIndex - 1);
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowRight') {
      nextIdx = Math.min(markers.length - 1, activeIndex + 1);
    } else if (e.key === 'PageUp') {
      nextIdx = Math.max(0, activeIndex - 5);
    } else if (e.key === 'PageDown') {
      nextIdx = Math.min(markers.length - 1, activeIndex + 5);
    } else if (e.key === 'Home') {
      nextIdx = 0;
    } else if (e.key === 'End') {
      nextIdx = markers.length - 1;
    } else {
      return;
    }

    e.preventDefault();
    activeIndex = nextIdx;
    const marker = markers[nextIdx];
    dispatch('jump', { marker, index: marker.index });
  }
</script>

{#if markers.length >= 2}
  <!-- Slider Track (Interactive Div Container) -->
  <div
    bind:this={railEl}
    role="slider"
    aria-label="Timeline date scrubber"
    aria-orientation="vertical"
    aria-valuemin="0"
    aria-valuemax={markers.length - 1}
    aria-valuenow={activeIndex}
    aria-valuetext="{activeMarker?.label} {activeMarker?.year}"
    tabindex="0"
    on:pointerdown={handlePointerDown}
    on:pointermove={handlePointerMove}
    on:pointerup={handlePointerUp}
    on:pointercancel={handlePointerUp}
    on:keydown={handleKeyDown}
    class="fixed right-1 top-1/2 -translate-y-1/2 z-40 h-[58vh] max-h-[500px] w-9 flex flex-col items-center justify-between py-2 select-none touch-none cursor-pointer group focus:outline-none"
  >
    <!-- Glass Track Capsule -->
    <div
      class="h-full rounded-full transition-all duration-300 flex flex-col justify-between items-center py-2.5 pointer-events-none {isScrubbing ? 'w-2 liquid-track-active' : 'w-1.5 liquid-track group-hover:w-2'}"
    >
      {#each markers as mark, i (mark.latestCapturedAt || `${mark.year}-${mark.label}-${i}`)}
        {@const isFirstOfYear = i === 0 || markers[i - 1].year !== mark.year}
        {@const isCurrent = activeIndex === i}

        {#if isFirstOfYear}
          <!-- Year Dot Indicator -->
          <div class="relative flex items-center justify-center pointer-events-none my-0.5">
            <div
              class="rounded-full transition-all duration-200 {isCurrent ? 'w-2.5 h-2.5 bg-purple-500 shadow-[0_0_10px_rgba(168,85,247,0.9)] scale-125' : 'w-1.5 h-1.5 bg-[var(--text-main)] opacity-75 group-hover:opacity-100'}"
            ></div>
          </div>
        {:else}
          <!-- Month Micro-Tick -->
          <div
            class="rounded-full transition-all duration-200 pointer-events-none {isCurrent ? 'w-2.5 h-1 bg-purple-500 shadow-[0_0_6px_rgba(168,85,247,0.7)]' : 'w-1 h-0.5 bg-[var(--text-muted)] opacity-40 group-hover:opacity-70'}"
          ></div>
        {/if}
      {/each}
    </div>
  </div>

  <!-- Liquid-Glass Date HUD Capsule -->
  {#if isScrubbing && activeMarker}
    <div
      transition:scale={{ duration: 140, start: 0.92 }}
      style="top: {tooltipY}px;"
      class="fixed right-12 -translate-y-1/2 z-50 pointer-events-none flex items-center shadow-2xl"
    >
      <div class="liquid-hud px-3.5 py-1.5 rounded-2xl flex items-center gap-2">
        <span class="text-xs font-semibold tracking-tight text-[var(--text-main)] capitalize">
          {activeMarker.label}
        </span>
        {#if activeMarker.year}
          <span class="text-[11px] font-mono text-[var(--text-muted)] font-medium">
            {activeMarker.year}
          </span>
        {/if}
        {#if activeMarker.count !== undefined && activeMarker.count > 0}
          <span class="text-[9px] font-mono px-1.5 py-0.5 rounded-md bg-purple-500/15 text-purple-600 dark:text-purple-300 font-medium">
            {activeMarker.count}
          </span>
        {/if}
      </div>

      <!-- Specular Pointer Arrow -->
      <div
        class="w-0 h-0 border-y-[5px] border-y-transparent border-l-[5px] border-l-[var(--border-glass)] -ml-[1px]"
      ></div>
    </div>
  {/if}
{/if}

<style>
  .liquid-track {
    background: var(--pill-bg);
    box-shadow: inset 0 0 1px var(--border-glass);
  }

  .liquid-track-active {
    background: var(--dock-border);
    box-shadow: 0 0 10px var(--dock-shadow), inset 0 1px 0 var(--border-specular);
  }

  .liquid-hud {
    background: var(--bg-surface-elevated);
    border: 1px solid var(--border-glass);
    backdrop-filter: blur(28px) saturate(190%);
    -webkit-backdrop-filter: blur(28px) saturate(190%);
    box-shadow:
      0 12px 30px var(--dock-shadow),
      inset 0 1px 0 0 var(--border-specular);
  }
</style>