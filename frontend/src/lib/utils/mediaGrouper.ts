import type { MediaItem } from '$lib/types/media';

export function getGroupHeader(dateStr: string | null): string {
  if (!dateStr) return 'Undated';
  const d = new Date(dateStr);
  return isNaN(d.getTime())
    ? 'Undated'
    : d.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
}

export function buildGroupedSections(items: MediaItem[]): [string, MediaItem[]][] {
  const map = new Map<string, MediaItem[]>();

  for (const item of items) {
    const key = getGroupHeader(item.captured_at);
    const list = map.get(key);
    if (!list) {
      map.set(key, [item]);
    } else {
      list.push(item);
    }
  }

  return Array.from(map.entries());
}

export function buildIndexMap(items: MediaItem[]): Map<string, number> {
  const map = new Map<string, number>();
  for (let i = 0; i < items.length; i++) {
    map.set(items[i].id, i);
  }
  return map;
}