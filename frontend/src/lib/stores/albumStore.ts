// frontend/src/lib/stores/albumStore.ts
import { writable } from 'svelte/store';
import {
  fetchCustomAlbums,
  createCustomAlbum,
  updateAlbumDetails,
  setAlbumCover,
  addAssetsToAlbum,
  removeAssetsFromAlbum,
  reorderAlbumAssets,
  deleteCustomAlbum,
  type AlbumRecord
} from '$lib/api/albums';

function createAlbumStore() {
  const { subscribe, set, update } = writable<AlbumRecord[]>([]);
  const isLoading = writable<boolean>(false);

  async function load(): Promise<void> {
    isLoading.set(true);
    try {
      const data = await fetchCustomAlbums();
      set(data);
    } catch (err) {
      console.error('Failed to load albums:', err);
    } finally {
      isLoading.set(false);
    }
  }

  async function create(title: string, description?: string): Promise<string> {
    const res = await createCustomAlbum(title, description);
    await load();
    return res.album_id;
  }

  async function renameAlbum(
    albumId: string,
    newTitle: string,
    description?: string
  ): Promise<boolean> {
    try {
      await updateAlbumDetails(albumId, newTitle, description);
      update((list) =>
        list.map((a) =>
          a.id === albumId
            ? { ...a, title: newTitle, description: description ?? a.description }
            : a
        )
      );
      return true;
    } catch (err) {
      console.error('Failed to rename album:', err);
      return false;
    }
  }

  async function setCover(
    albumId: string,
    assetId: string,
    thumbPath?: string
  ): Promise<boolean> {
    try {
      await setAlbumCover(albumId, assetId);
      update((list) =>
        list.map((a) =>
          a.id === albumId
            ? {
                ...a,
                cover_asset_id: assetId,
                cover_thumb: thumbPath ?? a.cover_thumb
              }
            : a
        )
      );
      return true;
    } catch (err) {
      console.error('Failed to set album cover:', err);
      return false;
    }
  }

  async function addItems(albumId: string, assetIds: string[]): Promise<void> {
    try {
      await addAssetsToAlbum(albumId, assetIds);
      await load();
    } catch (err) {
      console.error('Failed to add assets:', err);
      throw err;
    }
  }

  async function removeItems(albumId: string, assetIds: string[]): Promise<boolean> {
    try {
      await removeAssetsFromAlbum(albumId, assetIds);
      update((list) =>
        list.map((a) =>
          a.id === albumId
            ? { ...a, media_count: Math.max(0, a.media_count - assetIds.length) }
            : a
        )
      );
      return true;
    } catch (err) {
      console.error('Failed to remove assets from album:', err);
      return false;
    }
  }

  async function reorder(albumId: string, assetIds: string[]): Promise<boolean> {
    try {
      await reorderAlbumAssets(albumId, assetIds);
      return true;
    } catch (err) {
      console.error('Failed to reorder assets:', err);
      return false;
    }
  }

  async function removeAlbum(albumId: string, deleteMedia = false): Promise<boolean> {
    try {
      await deleteCustomAlbum(albumId, deleteMedia);
      update((list) => list.filter((a) => a.id !== albumId));
      return true;
    } catch (err) {
      console.error('Failed to remove album:', err);
      return false;
    }
  }

  return {
    subscribe,
    isLoading: { subscribe: isLoading.subscribe },
    load,
    create,
    renameAlbum,
    setCover,
    addItems,
    removeItems,
    reorder,
    removeAlbum
  };
}

export const albumStore = createAlbumStore();