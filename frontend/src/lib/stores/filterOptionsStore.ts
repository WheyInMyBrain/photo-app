import { writable } from 'svelte/store';
import { browser } from '$app/environment';
import { authStore } from '$lib/stores/authStore';

export interface FilterOption {
  value: string;
  label: string;
  count: number;
}

export interface FilterData {
  total_media: number;
  photos_count: number;
  videos_count: number;
  min_date: string | null;
  max_date: string | null;
  times_of_day: FilterOption[];
  people: FilterOption[];
  tags: FilterOption[];
  locations: FilterOption[];
  cameras: FilterOption[];
  albums: FilterOption[];
}

const initialFilters: FilterData = {
  total_media: 0,
  photos_count: 0,
  videos_count: 0,
  min_date: null,
  max_date: null,
  times_of_day: [],
  people: [],
  tags: [],
  locations: [],
  cameras: [],
  albums: []
};

function createFilterOptionsStore() {
  const { subscribe, set } = writable<FilterData>(initialFilters);
  let abortCtrl: AbortController | null = null;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  async function fetchFilters(queryString: string) {
    if (!browser) return;

    if (abortCtrl) abortCtrl.abort();
    abortCtrl = new AbortController();

    try {
      const res = await fetch(`/api/media/filters${queryString}`, {
        signal: abortCtrl.signal
      });

      if (res.status === 401) {
        authStore.checkStatus();
        return;
      }

      if (res.ok) {
        const data: FilterData = await res.json();
        set(data);
      }
    } catch (err: any) {
      if (err?.name !== 'AbortError') {
        console.error('Failed loading dynamic filters', err);
      }
    }
  }

  function scheduleRefresh(queryString: string, delay = 150) {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      fetchFilters(queryString);
    }, delay);
  }

  function destroy() {
    if (debounceTimer) clearTimeout(debounceTimer);
    if (abortCtrl) abortCtrl.abort();
  }

  return {
    subscribe,
    scheduleRefresh,
    destroy
  };
}

export const filterOptionsStore = createFilterOptionsStore();