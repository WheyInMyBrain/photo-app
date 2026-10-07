// photo-app/frontend/src/lib/stores/timelineStore.ts
import { writable } from 'svelte/store';
import { browser } from '$app/environment';
import { authStore } from '$lib/stores/authStore';
import type { SubAlbum, BreadcrumbSegment, MediaSection, MediaPageResponse, MediaItemSummary } from '$lib/types/media';

/**
 * Merges incoming sections across pagination boundaries.
 * If the first incoming section shares an ID with the last loaded section
 * (e.g. photos from the same day crossing cursor limits), append items directly.
 */
function mergeSections(current: MediaSection[], incoming: MediaSection[]): MediaSection[] {
  if (incoming.length === 0) return current;
  if (current.length === 0) return incoming;

  const cloned = [...current];
  const firstInc = incoming[0];
  const lastCur = cloned[cloned.length - 1];

  let startIdx = 0;

  // Stitch items if the boundary section ID matches (covers both matching dates AND 'explore-feed')
  if (lastCur.id === firstInc.id) {
    lastCur.items = [...lastCur.items, ...firstInc.items];
    startIdx = 1;
  }

  for (let i = startIdx; i < incoming.length; i++) {
    cloned.push(incoming[i]);
  }

  return cloned;
}

export function createTimelineStore() {
  const sections = writable<MediaSection[]>([]);
  const albums = writable<SubAlbum[]>([]);
  const breadcrumbs = writable<BreadcrumbSegment[]>([]);
  const isLoading = writable(false);
  const hasMore = writable(true);

  let nextCapturedAt: string | null = null;
  let nextId: string | null = null;
  let pageAbortCtrl: AbortController | null = null;
  let currentRequestId = 0;

  async function fetchMedia(queryString = '', reset = false) {
    if (!browser) return;

    const requestId = ++currentRequestId;

    if (reset) {
      if (pageAbortCtrl) pageAbortCtrl.abort();
      nextCapturedAt = null;
      nextId = null;
      hasMore.set(true);
    }

    pageAbortCtrl = new AbortController();
    isLoading.set(true);

    try {
      const params = new URLSearchParams(queryString.replace(/^\?/, ''));
      params.set('limit', '60');

      const isTimeline = params.get('sort') === 'timeline';

      // Ensure cursors are cleanly set or removed without empty string leaks
      if (isTimeline) {
        if (nextCapturedAt && nextCapturedAt.trim() !== '') {
          params.set('cursor_captured_at', nextCapturedAt);
        } else {
          params.delete('cursor_captured_at');
        }

        if (nextId && nextId.trim() !== '') {
          params.set('cursor_id', nextId);
        } else {
          params.delete('cursor_id');
        }
      } else {
        // Random mode uses only cursor_id — strip timeline date cursor
        params.delete('cursor_captured_at');

        if (nextId && nextId.trim() !== '') {
          params.set('cursor_id', nextId);
        } else {
          params.delete('cursor_id');
        }
      }

      const res = await fetch(`/api/media?${params.toString()}`, { signal: pageAbortCtrl.signal });

      if (requestId !== currentRequestId) return;

      if (res.status === 401) {
        authStore.checkStatus();
        return;
      }
      if (!res.ok) throw new Error(`HTTP ${res.status}`);

      const data: MediaPageResponse = await res.json();
      if (requestId !== currentRequestId) return;

      const incoming = data.sections ?? [];

      if (reset) {
        albums.set(data.albums ?? []);
        breadcrumbs.set(data.breadcrumbs ?? []);
        sections.set(incoming);
      } else {
        sections.update((curr) => mergeSections(curr, incoming));
      }

      nextCapturedAt = data.next_cursor_captured_at ?? null;
      nextId = data.next_cursor_id ?? null;
      hasMore.set(Boolean(data.has_more));
    } catch (err: any) {
      if (err?.name !== 'AbortError' && requestId === currentRequestId) {
        console.error('Timeline fetch error:', err);
      }
    } finally {
      if (requestId === currentRequestId) {
        isLoading.set(false);
      }
    }
  }

  async function jumpToDate(capturedAtIso: string, queryString = '') {
    if (!browser) return;
    const requestId = ++currentRequestId;
    if (pageAbortCtrl) pageAbortCtrl.abort();

    pageAbortCtrl = new AbortController();
    isLoading.set(true);
    hasMore.set(true);

    try {
      const params = new URLSearchParams(queryString.replace(/^\?/, ''));
      params.set('limit', '60');
      params.set('cursor_captured_at', capturedAtIso);

      const res = await fetch(`/api/media?${params.toString()}`, { signal: pageAbortCtrl.signal });
      if (requestId !== currentRequestId) return;

      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data: MediaPageResponse = await res.json();
      if (requestId !== currentRequestId) return;

      sections.set(data.sections ?? []);
      albums.set(data.albums ?? []);
      breadcrumbs.set(data.breadcrumbs ?? []);

      nextCapturedAt = data.next_cursor_captured_at ?? null;
      nextId = data.next_cursor_id ?? null;
      hasMore.set(Boolean(data.has_more));
    } catch (err: any) {
      if (err?.name !== 'AbortError' && requestId === currentRequestId) {
        console.error('Timeline jump error:', err);
      }
    } finally {
      if (requestId === currentRequestId) {
        isLoading.set(false);
      }
    }
  }

  function patchFavorite(coordsOrId: [number, number] | string, isFav: boolean) {
    sections.update((curr) => {
      if (typeof coordsOrId === 'string') {
        for (const sec of curr) {
          const item = sec.items.find((i: MediaItemSummary) => i.id === coordsOrId);
          if (item) {
            item.is_favorite = isFav;
            break;
          }
        }
      } else {
        const [s, i] = coordsOrId;
        if (curr[s]?.items[i]) {
          curr[s].items[i].is_favorite = isFav;
        }
      }
      return curr;
    });
  }

  function destroy() {
    currentRequestId++;
    if (pageAbortCtrl) pageAbortCtrl.abort();
  }

  return {
    sections,
    albums,
    breadcrumbs,
    isLoading,
    hasMore,
    fetchMedia,
    jumpToDate,
    patchFavorite,
    destroy
  };
}