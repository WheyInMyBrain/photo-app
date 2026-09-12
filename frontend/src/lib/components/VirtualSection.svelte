<script lang="ts">
  import { onMount, onDestroy } from 'svelte';

  export let minHeight = 320;

  let containerEl: HTMLElement;
  let isVisible = false;
  let recordedHeight = minHeight;
  let observer: IntersectionObserver | null = null;

  onMount(() => {
    observer = new IntersectionObserver(
      ([entry]) => {
        isVisible = entry.isIntersecting;
        if (entry.isIntersecting && containerEl) {
          const rect = containerEl.getBoundingClientRect();
          if (rect.height > 0) {
            recordedHeight = Math.round(rect.height);
          }
        }
      },
      {
        rootMargin: '1000px 0px 1000px 0px',
        threshold: 0
      }
    );

    if (containerEl) {
      observer.observe(containerEl);
    }
  });

  onDestroy(() => {
    if (observer) {
      observer.disconnect();
    }
  });
</script>

<section
  bind:this={containerEl}
  style="min-height: {isVisible ? 'auto' : `${recordedHeight}px`};"
  class="relative"
>
  {#if isVisible}
    <slot></slot>
  {:else}
    <div style="height: {recordedHeight}px;" class="w-full pointer-events-none"></div>
  {/if}
</section>