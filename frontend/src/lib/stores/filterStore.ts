import { writable, derived } from 'svelte/store';

export interface FilterState {
  is_private: boolean;
  q: string;
  media_type: 'all' | 'photos' | 'videos';
  is_favorite: boolean;
  folder_path: string;
  from: string;
  to: string;
  person_ids: Set<string>;
  tags: Set<string>;
  city: string;
  camera_model: string;
}

const initial: FilterState = {
  is_private: false,
  q: '',
  media_type: 'all',
  is_favorite: false,
  folder_path: '',
  from: '',
  to: '',
  person_ids: new Set(),
  tags: new Set(),
  city: '',
  camera_model: ''
};

function createFilterStore() {
  const { subscribe, set, update } = writable<FilterState>(initial);

  return {
    subscribe,
    toggleVaultMode: () => update(s => ({
      ...initial,
      is_private: !s.is_private,
      person_ids: new Set(),
      tags: new Set()
    })),
    lockVault: () => update(() => ({
      ...initial,
      is_private: false,
      person_ids: new Set(),
      tags: new Set()
    })),
    setQ: (q: string) => update(s => ({ ...s, q })),
    setMediaType: (media_type: 'all' | 'photos' | 'videos') => update(s => ({ ...s, media_type })),
    toggleFavorite: () => update(s => ({ ...s, is_favorite: !s.is_favorite })),
    setFolderPath: (folder_path: string) => update(s => ({ ...s, folder_path })),
    setFrom: (from: string) => update(s => ({ ...s, from })),
    setTo: (to: string) => update(s => ({ ...s, to })),
    setCity: (city: string) => update(s => ({ ...s, city })),
    setCamera: (camera_model: string) => update(s => ({ ...s, camera_model })),
    togglePerson: (id: string) => update(s => {
      const p = new Set(s.person_ids);
      p.has(id) ? p.delete(id) : p.add(id);
      return { ...s, person_ids: p };
    }),
    toggleTag: (tag: string) => update(s => {
      const t = new Set(s.tags);
      t.has(tag) ? t.delete(tag) : t.add(tag);
      return { ...s, tags: t };
    }),
    reset: () => update(s => ({
      ...initial,
      is_private: s.is_private,
      person_ids: new Set(),
      tags: new Set()
    }))
  };
}

export const filterStore = createFilterStore();

export const filterQueryString = derived(filterStore, ($s) => {
  const params = new URLSearchParams();
  params.set('is_private', $s.is_private ? 'true' : 'false');

  if ($s.q) params.set('q', $s.q);
  if ($s.media_type !== 'all') params.set('media_type', $s.media_type);
  if ($s.is_favorite) params.set('is_favorite', 'true');
  if ($s.folder_path) params.set('folder_path', $s.folder_path);
  if ($s.from) params.set('from', $s.from);
  if ($s.to) params.set('to', $s.to);
  if ($s.city) params.set('city', $s.city);
  if ($s.camera_model) params.set('camera_model', $s.camera_model);
  if ($s.person_ids.size > 0) params.set('person_id', Array.from($s.person_ids).join(','));
  if ($s.tags.size > 0) params.set('tag', Array.from($s.tags).join(','));

  return `?${params.toString()}`;
});