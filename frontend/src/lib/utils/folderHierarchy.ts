// photo-app/frontend/src/lib/utils/folderHierarchy.ts

export interface FolderNode {
  name: string;          // e.g. "outings"
  fullPath: string;      // e.g. "College/outings"
  hasChildren: boolean;  // true if it contains sub-albums
  childCount: number;    // number of direct sub-albums
}

export function normalizePath(path: string): string {
  return (path || '').trim().replace(/^\/+|\/+$/g, '');
}

/**
 * Returns immediate children folders under `currentLevelPath`.
 */
export function getDirectChildren(
  allAlbumPaths: string[],
  currentLevelPath: string
): FolderNode[] {
  
  const cleanCurrent = normalizePath(currentLevelPath);
  const prefix = cleanCurrent ? `${cleanCurrent}/` : '';

  const cleanPaths = Array.from(
    new Set(allAlbumPaths.map((p) => normalizePath(p)))
  ).filter(Boolean);

  const directMap = new Map<string, { fullPath: string; deeperCount: number }>();

  for (const p of cleanPaths) {
    if (prefix && !p.startsWith(prefix)) continue;

    const remainder = prefix ? p.slice(prefix.length) : p;
    const segments = remainder.split('/');
    const immediateName = segments[0];

    if (!immediateName) continue;

    const immediateFullPath = cleanCurrent
      ? `${cleanCurrent}/${immediateName}`
      : immediateName;

    if (!directMap.has(immediateName)) {
      directMap.set(immediateName, { fullPath: immediateFullPath, deeperCount: 0 });
    }

    if (segments.length > 1) {
      directMap.get(immediateName)!.deeperCount += 1;
    }
  }

  const result = Array.from(directMap.entries())
    .map(([name, info]) => ({
      name,
      fullPath: info.fullPath,
      hasChildren: info.deeperCount > 0,
      childCount: info.deeperCount
    }))
    .sort((a, b) => {
      if (a.hasChildren && !b.hasChildren) return -1;
      if (!a.hasChildren && b.hasChildren) return 1;
      return a.name.localeCompare(b.name);
    });

  return result;
}