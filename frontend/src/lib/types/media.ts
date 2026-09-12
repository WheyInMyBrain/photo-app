export interface SubAlbum {
  name: string;
  path: string;
  count: number;
  cover_thumb: string | null;
}

export interface MediaItem {
  id: string;
  file_name: string;
  thumb_path: string;
  preview_path: string;
  aspect_ratio: number | null;
  duration_seconds: number | null;
  mime_type: string;
  captured_at: string | null;
  is_favorite: number;
  deleted_at: string | null;
}

export interface MediaPageResponse {
  albums: SubAlbum[];
  items: MediaItem[];
  next_cursor_captured_at: string | null;
  next_cursor_id: string | null;
  has_more: boolean;
}