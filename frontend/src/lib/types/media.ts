export interface SubAlbum {
  name: string;
  path: string;
  count: number;
  cover_thumb: string | null;
}

export interface MediaItemSummary {
  id: string;
  file_name: string;
  thumb_path: string;
  preview_path: string;
  aspect_ratio: number;
  duration_seconds: number | null;
  mime_type: string;
  captured_at: string | null;
  is_favorite: boolean;
  days_remaining: number | null;
  latitude: number | null;
  longitude: number | null;
}

export interface MediaSection {
  title: string;
  items: MediaItemSummary[];
}

export interface MediaPageResponse {
  albums: SubAlbum[];
  sections: MediaSection[];
  next_cursor_captured_at: string | null;
  next_cursor_id: string | null;
  has_more: bool;
}