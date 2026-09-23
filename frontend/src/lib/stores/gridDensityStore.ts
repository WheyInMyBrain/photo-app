import { writable } from 'svelte/store';

export interface DensityConfig {
  cols: number;       // Desktop columns
  colsMobile: number; // Phone/Mobile columns
  rowHeight: number;
}

export const DENSITY_PRESETS: Record<number, DensityConfig> = {
  0: { cols: 6, colsMobile: 4, rowHeight: 140 }, // S: Default Compact Overview
  1: { cols: 4, colsMobile: 3, rowHeight: 220 }, // M: Balanced
  2: { cols: 2, colsMobile: 1, rowHeight: 380 }  // L: Cinematic Detail
};

function createGridDensityStore() {
  // Default to 0 (Small)
  const { subscribe, update, set } = writable<number>(0);

  return {
    subscribe,
    setDensity: (density: number) => set(Math.max(0, Math.min(2, density))),
    zoomIn: () => update((d) => Math.min(2, d + 1)),
    zoomOut: () => update((d) => Math.max(0, d - 1))
  };
}

export const gridDensity = createGridDensityStore();