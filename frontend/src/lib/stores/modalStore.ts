import { writable } from 'svelte/store';

export type ModalType = 'people' | 'upload' | 'map' | null;

function createModalStore() {
  const { subscribe, set } = writable<ModalType>(null);

  return {
    subscribe,
    openPeople: () => set('people'),
    openUpload: () => set('upload'),
    openMap: () => set('map'),
    close: () => set(null)
  };
}

export const modalStore = createModalStore();