import type { MediaSection, MediaItemSummary } from '$lib/types/media';

export type Coords = [number, number];

export function resolveAsset(sections: MediaSection[], coords: Coords | null): MediaItemSummary | null {
  if (!coords) return null;
  const [s, i] = coords;
  return sections[s]?.items[i] ?? null;
}

export function getPrevCoords(sections: MediaSection[], coords: Coords | null): Coords | null {
  if (!coords) return null;
  const [s, i] = coords;
  if (i > 0) return [s, i - 1];
  if (s > 0) return [s - 1, sections[s - 1].items.length - 1];
  return null;
}

export function getNextCoords(sections: MediaSection[], coords: Coords | null): Coords | null {
  if (!coords) return null;
  const [s, i] = coords;
  if (i < sections[s].items.length - 1) return [s, i + 1];
  if (s < sections.length - 1) return [s + 1, 0];
  return null;
}

export function findCoordsById(sections: MediaSection[], targetId: string): Coords | null {
  for (let s = 0; s < sections.length; s++) {
    const items = sections[s].items;
    for (let i = 0; i < items.length; i++) {
      if (items[i].id === targetId) return [s, i];
    }
  }
  return null;
}