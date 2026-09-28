// photo-app/frontend/src/lib/utils/folderHierarchy.ts
import type { FolderSuggestion } from '$lib/types/upload';

export function normalizePath(path: string): string {
  return path.trim().replace(/^\/+|\/+$/g, '');
}

export function getParentPath(cleanPath: string): string | null {
  if (!cleanPath) return null;
  const parts = cleanPath.split('/');
  if (parts.length <= 1) return '';
  return parts.slice(0, -1).join('/');
}

export function computeFolderSuggestions(
  rawAlbumPaths: string[],
  cleanInputPath: string
): FolderSuggestion[] {
  const rawUnique = Array.from(
    new Set(rawAlbumPaths.map((p) => p.replace(/^\/+|\/+$/g, '')))
  ).filter(Boolean);

  // 1. Root level suggestions when input is empty
  if (!cleanInputPath) {
    const roots = new Set<string>();
    for (const p of rawUnique) {
      const firstSegment = p.split('/')[0];
      if (firstSegment) roots.add(firstSegment);
    }
    return Array.from(roots).map((r) => ({
      fullPath: r,
      displayName: r,
      isFolder: rawUnique.some((p) => p.startsWith(`${r}/`))
    }));
  }

  // 2. Direct sub-folder level inside this directory
  const exactPrefix = `${cleanInputPath}/`;
  const directChildren = rawUnique.filter((p) => p.startsWith(exactPrefix));

  if (directChildren.length > 0) {
    const subEntries = new Map<string, { fullPath: string; isFolder: boolean }>();
    for (const child of directChildren) {
      const remainder = child.slice(exactPrefix.length);
      const nextSegment = remainder.split('/')[0];
      const nextFullPath = `${cleanInputPath}/${nextSegment}`;
      const hasDeeper = child.length > nextFullPath.length;
      if (!subEntries.has(nextSegment)) {
        subEntries.set(nextSegment, { fullPath: nextFullPath, isFolder: hasDeeper });
      }
    }

    return Array.from(subEntries.entries()).map(([name, info]) => ({
      fullPath: info.fullPath,
      displayName: name,
      isFolder: info.isFolder
    }));
  }

  // 3. Fuzzy search for manual typing
  const query = cleanInputPath.toLowerCase();
  return rawUnique
    .filter((p) => p.toLowerCase().includes(query) && p.toLowerCase() !== query)
    .slice(0, 8)
    .map((p) => ({
      fullPath: p,
      displayName: p,
      isFolder: false
    }));
}