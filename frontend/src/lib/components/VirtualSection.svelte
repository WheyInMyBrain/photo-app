<script lang="ts">
  import { onMount, onDestroy } from 'svelte';

  export let itemCount: number = 0;
  export let minHeight: number = 240;

  let containerEl: HTMLElement;
  // Default to true so the active viewport never flashes an empty spacer on mount
  let isVisible = true;
  let recordedHeight: number = minHeight;
  let observer: IntersectionObserver | null = null;
  let resizeObserver: ResizeObserver | null = null;

  $: if (itemCount > 0) {
    const estimatedRows = Math.ceil(itemCount / 4);
    const calculated = estimatedRows * 220 + 40;
    if (calculated > recordedHeight) {
      recordedHeight = calculated;
    }
  }

  onMount(() => {
    const scrollContainer = containerEl ? containerEl.closest('main') : null;

    observer = new IntersectionObserver(
      ([entry]) => {
        isVisible = entry.isIntersecting;
      },
      {
        root: scrollContainer,
        rootMargin: '800px 0px 800px 0px',
        threshold: 0
      }
    );

    resizeObserver = new ResizeObserver(([entry]) => {
      if (entry && entry.contentRect.height > 0) {
        recordedHeight = Math.round(entry.contentRect.height);
      }
    });

    if (containerEl) {
      observer.observe(containerEl);
      resizeObserver.observe(containerEl);
    }
  });

  onDestroy(() => {
    if (observer) observer.disconnect();
    if (resizeObserver) resizeObserver.disconnect();
  });
</script>

<section
  bind:this={containerEl}
  style="min-height: {isVisible ? 'auto' : `${recordedHeight}px`};"
  class="relative will-change-transform"
>
  {#if isVisible}
    <slot></slot>
  {:else}
    <div style="height: {recordedHeight}px;" class="w-full pointer-events-none"></div>
  {/if}
</section>