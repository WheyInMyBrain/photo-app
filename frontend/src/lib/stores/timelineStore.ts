import { writable } from 'svelte/store';
import { browser } from '$app/environment';
import { authStore } from '$lib/stores/authStore';
import type { SubAlbum, MediaSection, MediaPageResponse } from '$lib/types/media';

export function createTimelineStore() {
  const sections = writable<MediaSection[]>([]);
  const albums = writable<SubAlbum[]>([]);
  const isLoading = writable(false);
  const hasMore = writable(true);

  let nextCapturedAt: string | null = null;
  let nextId: string | null = null;
  let pageAbortCtrl: AbortController | null = null;

  function appendSections(current: MediaSection[], incoming: MediaSection[]): MediaSection[] {
    if (incoming.length === 0) return current;
    const cloned = [...current];

    if (cloned.length > 0) {
      const lastExisting = cloned[cloned.length - 1];
      const firstIncoming = incoming[0];

      if (lastExisting.title === firstIncoming.title) {
        lastExisting.items = [...lastExisting.items, ...firstIncoming.items];
        incoming.shift();
      }
    }
    return [...cloned, ...incoming];
  }

  async function fetchMedia(queryString = '', reset = false) {
    if (!browser) return;

    if (reset) {
      if (pageAbortCtrl) pageAbortCtrl.abort();
      nextCapturedAt = null;
      nextId = null;
      hasMore.set(true);
      sections.set([]);
    }

    pageAbortCtrl = new AbortController();
    isLoading.set(true);

    try {
      const params = new URLSearchParams(queryString.replace(/^\?/, ''));
      params.set('limit', '60');
      if (nextCapturedAt && nextId) {
        params.set('cursor_captured_at', nextCapturedAt);
        params.set('cursor_id', nextId);
      }

      const res = await fetch(`/api/media?${params.toString()}`, { signal: pageAbortCtrl.signal });
      if (res.status === 401) {
        authStore.checkStatus();
        return;
      }
      if (!res.ok) throw new Error(`HTTP ${res.status}`);

      const data: MediaPageResponse = await res.json();

      if (reset) albums.set(data.albums ?? []);

      sections.update((curr) => (reset ? (data.sections ?? []) : appendSections(curr, data.sections ?? [])));

      nextCapturedAt = data.next_cursor_captured_at;
      nextId = data.next_cursor_id;
      hasMore.set(data.has_more);
    } catch (err: any) {
      if (err?.name !== 'AbortError') console.error('Timeline fetch error:', err);
    } finally {
      isLoading.set(false);
    }
  }

  function patchFavorite(coords: [number, number], isFav: boolean) {
    sections.update((curr) => {
      const [s, i] = coords;
      if (curr[s]?.items[i]) {
        curr[s].items[i].is_favorite = isFav;
      }
      return curr;
    });
  }

  function destroy() {
    if (pageAbortCtrl) pageAbortCtrl.abort();
  }

  return {
    sections,
    albums,
    isLoading,
    hasMore,
    fetchMedia,
    patchFavorite,
    destroy,
  };
}