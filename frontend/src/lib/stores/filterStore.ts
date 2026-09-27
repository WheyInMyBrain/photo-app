import { writable, derived } from 'svelte/store';

export type ViewMode = 'timeline' | 'albums';

export interface FilterState {
  is_private: boolean;
  show_trash: boolean;
  q: string;
  media_type: 'all' | 'photos' | 'videos';
  is_favorite: boolean;
  album_id: string;
  view_mode: ViewMode; 
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
  show_trash: false,
  q: '',
  media_type: 'all',
  is_favorite: false,
  album_id: '',
  view_mode: 'timeline', 
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

    // --- VIEW MODE CONTROLS ---
    setViewMode: (view_mode: ViewMode) =>
      update((s) => ({
        ...s,
        view_mode,
        // Reset specific album selection if returning to full timeline
        album_id: view_mode === 'timeline' ? '' : s.album_id
      })),
    toggleViewMode: () =>
      update((s) => {
        const nextMode: ViewMode = s.view_mode === 'timeline' ? 'albums' : 'timeline';
        return {
          ...s,
          view_mode: nextMode,
          album_id: nextMode === 'timeline' ? '' : s.album_id
        };
      }),

    toggleVaultMode: () =>
      update((s) => ({
        ...initial,
        is_private: !s.is_private,
        show_trash: false,
        person_ids: new Set(),
        tags: new Set()
      })),
    lockVault: () =>
      update(() => ({
        ...initial,
        is_private: false,
        show_trash: false,
        person_ids: new Set(),
        tags: new Set()
      })),
    toggleTrash: () =>
      update((s) => ({
        ...s,
        show_trash: !s.show_trash
      })),
    setQ: (q: string) => update((s) => ({ ...s, q })),
    setMediaType: (media_type: 'all' | 'photos' | 'videos') =>
      update((s) => ({ ...s, media_type })),
    toggleFavorite: () => update((s) => ({ ...s, is_favorite: !s.is_favorite })),

    // Custom album selector (clears folder_path to avoid conflicting directory seeks)
    setAlbumId: (album_id: string) =>
      update((s) => ({
        ...s,
        album_id,
        folder_path: ''
      })),
    clearAlbum: () => update((s) => ({ ...s, album_id: '' })),

    // Folder navigation (clears album_id)
    setFolderPath: (folder_path: string) =>
      update((s) => ({
        ...s,
        folder_path,
        album_id: ''
      })),

    setFrom: (from: string) => update((s) => ({ ...s, from })),
    setTo: (to: string) => update((s) => ({ ...s, to })),
    setCity: (city: string) => update((s) => ({ ...s, city })),
    setCamera: (camera_model: string) => update((s) => ({ ...s, camera_model })),
    togglePerson: (id: string) =>
      update((s) => {
        const p = new Set(s.person_ids);
        p.has(id) ? p.delete(id) : p.add(id);
        return { ...s, person_ids: p };
      }),
    toggleTag: (tag: string) =>
      update((s) => {
        const t = new Set(s.tags);
        t.has(tag) ? t.delete(tag) : t.add(tag);
        return { ...s, tags: t };
      }),
    reset: () =>
      update((s) => ({
        ...initial,
        is_private: s.is_private,
        show_trash: false,
        person_ids: new Set(),
        tags: new Set()
      }))
  };
}

export const filterStore = createFilterStore();

export const filterQueryString = derived(filterStore, ($s) => {
  const params = new URLSearchParams();

  if ($s.is_private) params.set('is_private', 'true');
  if ($s.show_trash) params.set('show_trash', 'true');

  if ($s.album_id) params.set('album_id',$s.album_id);
  if ($s.folder_path) params.set('folder_path',$s.folder_path);

  if ($s.q) params.set('q',$s.q);
  if ($s.media_type !== 'all') params.set('media_type',$s.media_type);
  if ($s.is_favorite) params.set('is_favorite', 'true');
  if ($s.from) params.set('from',$s.from);
  if ($s.to) params.set('to',$s.to);
  if ($s.city) params.set('city',$s.city);
  if ($s.camera_model) params.set('camera_model',$s.camera_model);
  if ($s.person_ids.size > 0) params.set('person_id', Array.from($s.person_ids).join(','));
  if ($s.tags.size > 0) params.set('tag', Array.from($s.tags).join(','));

  const query = params.toString();
  return query ? `?${query}` : '';
});