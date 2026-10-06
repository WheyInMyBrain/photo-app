// photo-app/frontend/src/lib/stores/timelineStore.ts
import { writable } from 'svelte/store';
import { browser } from '$app/environment';
import { authStore } from '$lib/stores/authStore';
import type { SubAlbum, MediaSection, MediaPageResponse, AssetRecord } from '$lib/types/media';

export interface DailyMediaSection extends MediaSection {
  id?: string;
  month?: string;
  year?: string;
}

/** Formats a timestamp into: "Saturday, 12th September 2026" */
function formatDayTitle(date: Date): string {
  const day = date.getDate();
  const suffix = (d: number) => {
    if (d > 3 && d < 21) return 'th';
    switch (d % 10) {
      case 1:  return 'st';
      case 2:  return 'nd';
      case 3:  return 'rd';
      default: return 'th';
    }
  };

  const weekday = date.toLocaleDateString('en-US', { weekday: 'long' });
  const month = date.toLocaleDateString('en-US', { month: 'long' });
  const year = date.getFullYear();

  return `${weekday}, ${day}${suffix(day)} ${month} ${year}`;
}

/** Resolves an asset's day key: "YYYY-MM-DD" */
function getDayKey(asset: AssetRecord): string {
  const rawDate = asset.captured_at || asset.created_at;
  if (!rawDate) return 'undated';
  const d = new Date(rawDate);
  if (isNaN(d.getTime())) return 'undated';
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** Regroups arbitrary sections or flat assets into strict day-by-day sections (TIMELINE ONLY) */
function repartitionIntoDailySections(incomingSections: MediaSection[]): DailyMediaSection[] {
  const result: DailyMediaSection[] = [];
  const map = new Map<string, DailyMediaSection>();

  for (const sec of incomingSections) {
    for (const item of sec.items) {
      const dayKey = getDayKey(item);

      if (!map.has(dayKey)) {
        let title = 'Undated';
        let month = '';
        let year = '';

        if (dayKey !== 'undated') {
          const rawDate = item.captured_at || item.created_at;
          const d = rawDate ? new Date(rawDate) : null;
          if (d && !isNaN(d.getTime())) {
            title = formatDayTitle(d);
            month = d.toLocaleDateString('en-US', { month: 'short' });
            year = String(d.getFullYear());
          }
        }

        const newSection: DailyMediaSection = {
          id: dayKey,
          title,
          month,
          year,
          items: []
        };
        map.set(dayKey, newSection);
        result.push(newSection);
      }

      map.get(dayKey)!.items.push(item);
    }
  }

  return result;
}

/** Merges incoming sections into existing sections across pagination boundaries */
function appendSections(current: DailyMediaSection[], incoming: DailyMediaSection[], isTimeline: boolean): DailyMediaSection[] {
  if (incoming.length === 0) return current;
  const cloned = [...current];

  if (!isTimeline) {
    // In random mode: simply append new chunked sections directly
    return [...cloned, ...incoming];
  }

  // In timeline mode: merge matching day boundaries
  for (const inc of incoming) {
    if (cloned.length > 0) {
      const last = cloned[cloned.length - 1];
      if ((last.id && inc.id && last.id === inc.id) || last.title === inc.title) {
        last.items = [...last.items, ...inc.items];
        continue;
      }
    }
    cloned.push(inc);
  }

  return cloned;
}

export function createTimelineStore() {
  const sections = writable<DailyMediaSection[]>([]);
  const albums = writable<SubAlbum[]>([]);
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

      // Pass the appropriate cursors based on mode
      if (isTimeline) {
        if (nextCapturedAt && nextId) {
          params.set('cursor_captured_at', nextCapturedAt);
          params.set('cursor_id', nextId);
        }
      } else {
        // Random mode only needs cursor_id
        if (nextId) {
          params.set('cursor_id', nextId);
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

      // In timeline mode: re-group into calendar days.
      // In random mode: KEEP THE EXACT BACKEND RANDOM ORDER!
      const processedSections: DailyMediaSection[] = isTimeline
        ? repartitionIntoDailySections(data.sections ?? [])
        : (data.sections ?? []);

      if (reset) {
        albums.set(data.albums ?? []);
        sections.set(processedSections);
      } else {
        sections.update((curr) => appendSections(curr, processedSections, isTimeline));
      }

      nextCapturedAt = data.next_cursor_captured_at;
      nextId = data.next_cursor_id;
      hasMore.set(data.has_more);
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

      const dailySections = repartitionIntoDailySections(data.sections ?? []);
      sections.set(dailySections);
      albums.set(data.albums ?? []);

      nextCapturedAt = data.next_cursor_captured_at;
      nextId = data.next_cursor_id;
      hasMore.set(data.has_more);
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
    currentRequestId++;
    if (pageAbortCtrl) pageAbortCtrl.abort();
  }

  return {
    sections,
    albums,
    isLoading,
    hasMore,
    fetchMedia,
    jumpToDate,
    patchFavorite,
    destroy
  };
}