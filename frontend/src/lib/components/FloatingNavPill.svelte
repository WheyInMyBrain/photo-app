<!-- photo-app/frontend/src/lib/components/NavigationDock.svelte -->
<script lang="ts">
  import { gridDensity } from '$lib/stores/gridDensityStore';
  import { modalStore } from '$lib/stores/modalStore';
</script>

<nav
  style="bottom: max(1.5rem, calc(var(--sab) + 0.75rem));"
  class="fixed left-1/2 -translate-x-1/2 z-30 glass-pill px-3 py-1.5 rounded-full flex items-center gap-3 sm:gap-4 shadow-2xl select-none backdrop-blur-2xl"
  aria-label="Primary Navigation and Density Controls"
>
  <!-- Main View Links -->
  <div class="flex items-center gap-1">
    <button
      type="button"
      on:click={() => window.scrollTo({ top: 0, behavior: 'smooth' })}
      class="text-xs font-semibold px-2.5 py-1.5 rounded-lg transition-colors hover:text-purple-600 dark:hover:text-purple-400 text-[var(--text-main)] cursor-pointer flex items-center gap-1.5 spring-tap"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 opacity-80" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
        <circle cx="8.5" cy="8.5" r="1.5"></circle>
        <polyline points="21 15 16 10 5 21"></polyline>
      </svg>
      <span>Photos</span>
    </button>

    <button
      type="button"
      on:click={() => modalStore.openPeople()}
      class="text-xs font-semibold px-2.5 py-1.5 rounded-lg transition-colors hover:text-purple-600 dark:hover:text-purple-400 text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer flex items-center gap-1.5 spring-tap"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 opacity-80" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"></path>
        <circle cx="9" cy="7" r="4"></circle>
        <path d="M23 21v-2a4 4 0 0 0-3-3.87"></path>
        <path d="M16 3.13a4 4 0 0 1 0 7.75"></path>
      </svg>
      <span>People</span>
    </button>

    <button
      type="button"
      on:click={() => modalStore.openMap()}
      class="text-xs font-semibold px-2.5 py-1.5 rounded-lg transition-colors hover:text-purple-600 dark:hover:text-purple-400 text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer flex items-center gap-1.5 spring-tap"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 opacity-80" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polygon points="1 6 1 22 8 18 16 22 23 18 23 2 16 6 8 2 1 6"></polygon>
        <line x1="8" y1="2" x2="8" y2="18"></line>
        <line x1="16" y1="6" x2="16" y2="22"></line>
      </svg>
      <span>Places</span>
    </button>
  </div>

  <div class="w-px h-4 bg-[var(--border-glass)]"></div>

  <!-- Density Switcher -->
  <div class="flex items-center bg-[var(--pill-bg)] p-0.5 rounded-full border border-[var(--border-glass)]" role="group" aria-label="Grid density">
    <button
      type="button"
      on:click={() => gridDensity.setDensity(0)}
      class="w-6 h-5 rounded-full text-[10px] font-mono flex items-center justify-center transition-all spring-tap cursor-pointer {$gridDensity === 0 ? 'bg-purple-600 text-white font-bold shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      title="Compact View"
      aria-label="Compact grid"
    >
      S
    </button>
    <button
      type="button"
      on:click={() => gridDensity.setDensity(1)}
      class="w-6 h-5 rounded-full text-[10px] font-mono flex items-center justify-center transition-all spring-tap cursor-pointer {$gridDensity === 1 ? 'bg-purple-600 text-white font-bold shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      title="Normal View"
      aria-label="Normal grid"
    >
      M
    </button>
    <button
      type="button"
      on:click={() => gridDensity.setDensity(2)}
      class="w-6 h-5 rounded-full text-[10px] font-mono flex items-center justify-center transition-all spring-tap cursor-pointer {$gridDensity === 2 ? 'bg-purple-600 text-white font-bold shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      title="Cinematic View"
      aria-label="Cinematic grid"
    >
      L
    </button>
  </div>
</nav>