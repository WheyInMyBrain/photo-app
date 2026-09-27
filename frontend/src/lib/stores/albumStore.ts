// frontend/src/lib/stores/albumStore.ts
import { writable } from 'svelte/store';
import {
  fetchCustomAlbums,
  createCustomAlbum,
  addAssetsToAlbum,
  deleteCustomAlbum,
  type AlbumRecord
} from '$lib/api/albums';

function createAlbumStore() {
  const { subscribe, set, update } = writable<AlbumRecord[]>([]);
  let isLoading = writable<boolean>(false);

  async function load() {
    isLoading.set(true);
    try {
      const data = await fetchCustomAlbums();
      set(data);
    } catch (err) {
      console.error(err);
    } finally {
      isLoading.set(false);
    }
  }

  async function create(title: string, description?: string) {
    const res = await createCustomAlbum(title, description);
    await load();
    return res.album_id;
  }

  async function addItems(albumId: string, assetIds: string[]) {
    await addAssetsToAlbum(albumId, assetIds);
    await load();
  }

  async function removeAlbum(albumId: string) {
    await deleteCustomAlbum(albumId);
    update((list) => list.filter((a) => a.id !== albumId));
  }

  return {
    subscribe,
    isLoading: { subscribe: isLoading.subscribe },
    load,
    create,
    addItems,
    removeAlbum
  };
}

export const albumStore = createAlbumStore();