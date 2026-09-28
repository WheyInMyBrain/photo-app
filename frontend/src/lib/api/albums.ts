// frontend/src/lib/api/albums.ts

export interface AlbumRecord {
  id: string;
  user_id: string;
  title: string;
  description: string | null;
  album_type: 'MANUAL' | 'SMART';
  cover_asset_id: string | null;
  cover_thumb: string | null;
  media_count: number;
  filter_criteria: string | null;
  created_at: string;
  updated_at: string;
}

export async function fetchCustomAlbums(): Promise<AlbumRecord[]> {
  const res = await fetch('/api/albums', { credentials: 'include' });
  if (!res.ok) throw new Error('Failed to load albums');
  return res.json();
}

export async function createCustomAlbum(
  title: string,
  description?: string
): Promise<{ status: string; album_id: string }> {
  const res = await fetch('/api/albums', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({ title, description, album_type: 'MANUAL' })
  });
  if (!res.ok) throw new Error('Failed to create album');
  return res.json();
}

export async function updateAlbumDetails(
  albumId: string,
  title: string,
  description?: string
): Promise<void> {
  const res = await fetch(`/api/albums/${albumId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({ title, description })
  });
  if (!res.ok) throw new Error('Failed to update album details');
}

export async function setAlbumCover(albumId: string, assetId?: string): Promise<void> {
  const res = await fetch(`/api/albums/${albumId}/cover`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({ asset_id: assetId ?? null })
  });
  if (!res.ok) throw new Error('Failed to update album cover');
}

export async function addAssetsToAlbum(albumId: string, assetIds: string[]): Promise<void> {
  const res = await fetch(`/api/albums/${albumId}/assets`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({ asset_ids: assetIds })
  });
  if (!res.ok) throw new Error('Failed to add assets to album');
}

export async function removeAssetsFromAlbum(albumId: string, assetIds: string[]): Promise<void> {
  const res = await fetch(`/api/albums/${albumId}/assets/remove`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({ asset_ids: assetIds })
  });
  if (!res.ok) throw new Error('Failed to remove assets from album');
}

export async function reorderAlbumAssets(albumId: string, assetIds: string[]): Promise<void> {
  const res = await fetch(`/api/albums/${albumId}/reorder`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({ asset_ids: assetIds })
  });
  if (!res.ok) throw new Error('Failed to reorder album assets');
}

export async function deleteCustomAlbum(albumId: string, deleteMedia = false): Promise<void> {
  // Uses delete_media query param to optionally move assets to trash
  const res = await fetch(`/api/albums/${albumId}?delete_media=${deleteMedia}`, {
    method: 'DELETE',
    credentials: 'include'
  });
  if (!res.ok) throw new Error('Failed to delete album');
}