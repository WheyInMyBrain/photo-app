<script lang="ts">
  import { gridDensity } from '$lib/stores/gridDensityStore';
  import { modalStore } from '$lib/stores/modalStore';
</script>

<nav
  class="fixed bottom-6 left-1/2 -translate-x-1/2 z-30 glass-pill px-3 py-1.5 rounded-full flex items-center gap-3 sm:gap-5 shadow-2xl select-none"
  aria-label="Primary Navigation and Density Controls"
>
  <div class="flex items-center gap-1">
    <button
      type="button"
      on:click={() => window.scrollTo({ top: 0, behavior: 'smooth' })}
      class="text-xs font-semibold px-2.5 py-1 rounded-lg transition-colors hover:text-purple-400 text-[var(--text-main)] cursor-pointer"
    >
      Photos
    </button>
    <button
      type="button"
      on:click={() => modalStore.openPeople()}
      class="text-xs font-semibold px-2.5 py-1 rounded-lg transition-colors hover:text-purple-400 text-[var(--text-muted)] cursor-pointer"
    >
      People
    </button>
    <button
      type="button"
      on:click={() => modalStore.openMap()}
      class="text-xs font-semibold px-2.5 py-1 rounded-lg transition-colors hover:text-purple-400 text-[var(--text-muted)] cursor-pointer"
    >
      Places
    </button>
  </div>

  <div class="w-px h-4 bg-[var(--border-glass)]"></div>

  <div class="flex items-center bg-[var(--bg-surface)] p-0.5 rounded-full border border-[var(--border-glass)]" role="group" aria-label="Grid density">
    <button
      type="button"
      on:click={() => gridDensity.setDensity(0)}
      class="px-2 py-0.5 rounded-full text-[10px] font-mono transition-all spring-tap cursor-pointer {$gridDensity === 0 ? 'bg-purple-600 text-white font-bold shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      title="Compact View"
      aria-label="Compact grid"
    >
      S
    </button>
    <button
      type="button"
      on:click={() => gridDensity.setDensity(1)}
      class="px-2 py-0.5 rounded-full text-[10px] font-mono transition-all spring-tap cursor-pointer {$gridDensity === 1 ? 'bg-purple-600 text-white font-bold shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      title="Normal View"
      aria-label="Normal grid"
    >
      M
    </button>
    <button
      type="button"
      on:click={() => gridDensity.setDensity(2)}
      class="px-2 py-0.5 rounded-full text-[10px] font-mono transition-all spring-tap cursor-pointer {$gridDensity === 2 ? 'bg-purple-600 text-white font-bold shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      title="Cinematic View"
      aria-label="Cinematic grid"
    >
      L
    </button>
  </div>
</nav>