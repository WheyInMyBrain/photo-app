<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { fade, scale } from 'svelte/transition';

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
      tooltipY = Math.max(rect.top + 20, Math.min(e.clientY, rect.bottom - 20));
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
</script>

{#if markers.length > 2}
  <!-- Right Rail Anchor Zone (Broad touch target for easy thumb grabbing) -->
  <aside
    bind:this={railEl}
    role="slider"
    aria-label="Timeline date scrubber"
    aria-valuemin="0"
    aria-valuemax={markers.length - 1}
    aria-valuenow={activeIndex}
    aria-valuetext="{activeMarker?.label} {activeMarker?.year}"
    tabindex="0"
    on:pointerdown={handlePointerDown}
    on:pointermove={handlePointerMove}
    on:pointerup={handlePointerUp}
    on:pointercancel={handlePointerUp}
    class="fixed right-0 top-1/2 -translate-y-1/2 z-40 h-[60vh] w-9 flex flex-col items-center justify-between py-4 select-none touch-none cursor-pointer group"
  >
    <!-- Background track capsule that expands when hovered or dragging -->
    <div
      class="h-full w-1 rounded-full transition-all duration-200 flex flex-col justify-between items-center py-2 pointer-events-none {isScrubbing ? 'w-2 bg-white/20 backdrop-blur-md' : 'bg-white/10 group-hover:bg-white/15'}"
    >
      {#each markers as mark, i (mark.index)}
        {@const isFirstOfYear = i === 0 || markers[i - 1].year !== mark.year}
        {@const isCurrent = activeIndex === i}

        {#if isFirstOfYear}
          <!-- Year Indicator Dot (Prominent) -->
          <div
            class="relative flex items-center justify-center pointer-events-none"
          >
            <div
              class="rounded-full transition-all duration-150 {isCurrent ? 'w-2.5 h-2.5 bg-purple-400 shadow-[0_0_8px_#c084fc]' : 'w-1.5 h-1.5 bg-white/70'}"
            ></div>
          </div>
        {:else}
          <!-- Month Micro-Tick -->
          <div
            class="rounded-full transition-all duration-150 pointer-events-none {isCurrent ? 'w-2 h-0.5 bg-purple-300' : 'w-1 h-0.5 bg-white/30'}"
          ></div>
        {/if}
      {/each}
    </div>
  </aside>

  <!-- Floating Apple-style Date HUD Capsule -->
  {#if isScrubbing && activeMarker}
    <div
      transition:scale={{ duration: 160, start: 0.88 }}
      style="top: {tooltipY}px;"
      class="fixed right-12 -translate-y-1/2 z-50 pointer-events-none flex items-center gap-2"
    >
      <div
        class="glass-pill px-4 py-2 rounded-2xl shadow-2xl border border-white/25 flex items-baseline gap-1.5 bg-black/75 backdrop-blur-2xl"
      >
        <span class="text-sm font-bold tracking-tight text-white capitalize">
          {activeMarker.label}
        </span>
        {#if activeMarker.year}
          <span class="text-xs font-mono text-purple-300 font-semibold">
            {activeMarker.year}
          </span>
        {/if}
      </div>

      <!-- Arrow Pointer connecting Capsule to Rail -->
      <div
        class="w-0 h-0 border-y-[6px] border-y-transparent border-l-[6px] border-l-black/75 -ml-2"
      ></div>
    </div>
  {/if}
{/if}