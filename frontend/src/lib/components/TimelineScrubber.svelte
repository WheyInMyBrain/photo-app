<!-- photo-app/frontend/src/lib/components/TimelineScrubber.svelte -->
<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { scale } from 'svelte/transition';

  export let markers: { label: string; year: string; index: number }[] = [];

  const dispatch = createEventDispatcher<{
    jump: { index: number };
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
      dispatch('jump', { index: markers[nextIdx].index });
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
    dispatch('jump', { index: markers[nextIdx].index });
  }
</script>

{#if markers.length > 2}
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
    class="fixed right-1 top-1/2 -translate-y-1/2 z-40 h-[55vh] w-8 flex flex-col items-center justify-between py-3 select-none touch-none cursor-pointer group focus:outline-none"
  >
    <!-- Glass Track Capsule -->
    <div
      class="h-full rounded-full transition-all duration-300 flex flex-col justify-between items-center py-2.5 pointer-events-none {isScrubbing ? 'w-1.5 liquid-track-active' : 'w-1 liquid-track group-hover:w-1.5'}"
    >
      {#each markers as mark, i (mark.index)}
        {@const isFirstOfYear = i === 0 || markers[i - 1].year !== mark.year}
        {@const isCurrent = activeIndex === i}

        {#if isFirstOfYear}
          <!-- Year Dot Indicator -->
          <div class="relative flex items-center justify-center pointer-events-none">
            <div
              class="rounded-full transition-all duration-200 {isCurrent ? 'w-2 h-2 bg-white shadow-[0_0_8px_rgba(255,255,255,0.8)] scale-125' : 'w-1.5 h-1.5 bg-white/70 group-hover:bg-white/90'}"
            ></div>
          </div>
        {:else}
          <!-- Month Micro-Tick -->
          <div
            class="rounded-full transition-all duration-200 pointer-events-none {isCurrent ? 'w-2 h-0.5 bg-white/90 shadow-[0_0_4px_rgba(255,255,255,0.6)]' : 'w-1 h-0.5 bg-white/25 group-hover:bg-white/40'}"
          ></div>
        {/if}
      {/each}
    </div>
  </div>

  <!-- Apple Liquid-Glass Date HUD Capsule -->
  {#if isScrubbing && activeMarker}
    <div
      transition:scale={{ duration: 150, start: 0.9 }}
      style="top: {tooltipY}px;"
      class="fixed right-11 -translate-y-1/2 z-50 pointer-events-none flex items-center"
    >
      <div
        class="liquid-hud px-4 py-2 rounded-2xl flex items-baseline gap-1.5"
      >
        <span class="text-xs font-semibold tracking-tight text-white capitalize">
          {activeMarker.label}
        </span>
        {#if activeMarker.year}
          <span class="text-[11px] font-mono text-white/60 font-medium">
            {activeMarker.year}
          </span>
        {/if}
      </div>

      <!-- Specular Pointer Arrow -->
      <div
        class="w-0 h-0 border-y-[5px] border-y-transparent border-l-[5px] border-l-white/20 -ml-[1px]"
      ></div>
    </div>
  {/if}
{/if}

<style>
  /* Apple Liquid-Glass Track */
  .liquid-track {
    background: rgba(255, 255, 255, 0.1);
    box-shadow: inset 0 0 1px rgba(255, 255, 255, 0.2);
  }

  .liquid-track-active {
    background: rgba(255, 255, 255, 0.25);
    box-shadow: 
      0 0 12px rgba(255, 255, 255, 0.2),
      inset 0 1px 0 rgba(255, 255, 255, 0.4);
  }

  /* Liquid-Glass HUD Capsule */
  .liquid-hud {
    background: rgba(20, 20, 24, 0.72);
    border: 1px solid rgba(255, 255, 255, 0.16);
    backdrop-filter: blur(28px) saturate(190%);
    -webkit-backdrop-filter: blur(28px) saturate(190%);
    box-shadow:
      0 12px 30px rgba(0, 0, 0, 0.6),
      inset 0 1px 0 0 rgba(255, 255, 255, 0.3),
      inset 0 -1px 0 0 rgba(0, 0, 0, 0.4);
  }
</style>