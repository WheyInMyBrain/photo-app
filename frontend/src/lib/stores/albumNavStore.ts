// photo-app/frontend/src/lib/stores/albumNavStore.ts
import { derived } from 'svelte/store';
import { filterStore } from './filterStore';
import { albumStore } from './albumStore';

export interface AlbumRecord {
  id: string;
  title?: string | null;
  cover_thumb?: string | null;
  media_count?: number;
  [key: string]: any;
}

export interface BreadcrumbSegment {
  name: string;
  path: string;
  albumId: string | null;
}

export interface RootAlbumItem extends AlbumRecord {
  cleanPath: string;
  rootName: string;
}

export interface ChildAlbumItem extends AlbumRecord {
  directSubName: string;
  displayTitle: string;
}

export const albumNav = derived(
  [filterStore, albumStore],
  ([$filter,$albums]) => {
    const albumList = ($albums as AlbumRecord[]) || [];

    const currentAlbum = $filter.album_id
      ? albumList.find((a) => a.id === $filter.album_id) ?? null
      : null;

    const currentAlbumPath = currentAlbum
      ? (currentAlbum.title || '').replace(/^\/+|\/+$/g, '')
      : ($filter.folder_path || '').replace(/^\/+\vert{}\/+$/g, '');

    // Breadcrumbs
    const breadcrumbSegments: BreadcrumbSegment[] = [];
    if (currentAlbumPath) {
      const parts = currentAlbumPath.split('/');
      let cumulative = '';
      for (const part of parts) {
        cumulative = cumulative ? `${cumulative}/${part}` : part;
        const matched = albumList.find(
          (a) => (a.title || '').replace(/^\/+|\/+$/g, '') === cumulative
        );
        breadcrumbSegments.push({
          name: part,
          path: cumulative,
          albumId: matched?.id ?? null
        });
      }
    }

    // Root Albums
    const rootAlbums: RootAlbumItem[] = albumList
      .map((alb) => {
        const clean = (alb.title || '').replace(/^\/+|\/+$/g, '');
        const parts = clean.split('/');
        return { ...alb, cleanPath: clean, rootName: parts[0] };
      })
      .filter((alb) => !currentAlbumPath && alb.cleanPath === alb.rootName);

    // Child Sub-Albums
    let childAlbums: ChildAlbumItem[] = [];
    if (currentAlbumPath) {
      const prefix = `${currentAlbumPath}/`;
      childAlbums = albumList
        .filter((alb) => {
          const clean = (alb.title || '').replace(/^\/+|\/+$/g, '');
          return clean.startsWith(prefix) && clean !== currentAlbumPath;
        })
        .map((alb) => {
          const clean = (alb.title || '').replace(/^\/+|\/+$/g, '');
          const remainder = clean.slice(prefix.length);
          const directSubName = remainder.split('/')[0];
          return {
            ...alb,
            directSubName,
            displayTitle: directSubName
          };
        })
        .filter((item, index, self) =>
          index === self.findIndex((t) => t.directSubName === item.directSubName)
        );
    }

    return {
      currentAlbum,
      currentAlbumPath,
      breadcrumbSegments,
      rootAlbums,
      childAlbums
    };
  }
);