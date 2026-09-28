// photo-app/frontend/src/lib/api/assets.ts

export interface AssetObjectDetail {
  id: string;
  asset_id: string;
  class_id: number;
  label: string;
  score: number;
  bbox_x: number;
  bbox_y: number;
  bbox_w: number;
  bbox_h: number;
}

export interface KeypointDetail {
  x: number;
  y: number;
  score: number;
}

export interface AssetPoseDetail {
  id: string;
  asset_id: string;
  score: number;
  bbox_x: number;
  bbox_y: number;
  bbox_w: number;
  bbox_h: number;
  keypoints: KeypointDetail[];
}

export interface FavoriteToggleResponse {
  asset_id: string;
  is_favorite: boolean;
}

export interface SoftDeleteResponse {
  id: string;
  is_deleted: boolean;
  deleted_at: string | null;
}

export interface BatchActionResponse {
  affected_count: number;
}

export interface SimilarMediaItem {
  id: string;
  thumb_path: string;
  mime_type: string;
  similarity: number;
}

// ---------------------------------------------------------------------------
// Single Asset Operations
// ---------------------------------------------------------------------------

export async function toggleAssetFavorite(assetId: string): Promise<FavoriteToggleResponse> {
  const res = await fetch(`/api/assets/${assetId}/favorite`, {
    method: 'POST',
    credentials: 'include'
  });
  if (!res.ok) throw new Error('Failed to toggle favorite');
  return res.json();
}

export async function toggleAssetSoftDelete(assetId: string): Promise<SoftDeleteResponse> {
  const res = await fetch(`/api/assets/${assetId}/delete`, {
    method: 'POST',
    credentials: 'include'
  });
  if (!res.ok) throw new Error('Failed to toggle delete');
  return res.json();
}

export async function purgeAsset(assetId: string): Promise<boolean> {
  const res = await fetch(`/api/assets/${assetId}/purge`, {
    method: 'POST',
    credentials: 'include'
  });
  return res.status === 204 || res.ok;
}

// ---------------------------------------------------------------------------
// YOLO Detections & AI Inferences
// ---------------------------------------------------------------------------

export async function fetchAssetObjects(assetId: string, signal?: AbortSignal): Promise<AssetObjectDetail[]> {
  const res = await fetch(`/api/assets/${assetId}/objects`, {
    credentials: 'include',
    signal
  });
  if (!res.ok) return [];
  return res.json();
}

export async function fetchAssetPoses(assetId: string, signal?: AbortSignal): Promise<AssetPoseDetail[]> {
  const res = await fetch(`/api/assets/${assetId}/poses`, {
    credentials: 'include',
    signal
  });
  if (!res.ok) return [];
  return res.json();
}

export async function fetchAssetTags(assetId: string, signal?: AbortSignal): Promise<{ name: string; confidence: number }[]> {
  const res = await fetch(`/api/assets/${assetId}/tags`, {
    credentials: 'include',
    signal
  });
  if (!res.ok) return [];
  return res.json();
}

export async function fetchSimilarAssets(assetId: string, signal?: AbortSignal): Promise<SimilarMediaItem[]> {
  const res = await fetch(`/api/assets/${assetId}/similar`, {
    credentials: 'include',
    signal
  });
  if (!res.ok) return [];
  return res.json();
}

// ---------------------------------------------------------------------------
// Batch Operations
// ---------------------------------------------------------------------------

export async function batchToggleSoftDelete(assetIds: string[]): Promise<BatchActionResponse> {
  const res = await fetch('/api/assets/batch/delete', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({ ids: assetIds })
  });
  if (!res.ok) throw new Error('Failed batch delete operation');
  return res.json();
}

export async function batchPurgeAssets(assetIds: string[]): Promise<BatchActionResponse> {
  const res = await fetch('/api/assets/batch/purge', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({ ids: assetIds })
  });
  if (!res.ok) throw new Error('Failed batch purge operation');
  return res.json();
}