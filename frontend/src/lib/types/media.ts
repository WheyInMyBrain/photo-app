export interface SubAlbum {
  id?: string | null;
  name: string;
  path: string;
  count: number;
  cover_thumb: string | null;
}

export interface BreadcrumbSegment {
  name: string;
  path: string;
  album_id?: string | null;
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
  id: string;
  title: string | null;       // null in random/explore mode, formatted title in timeline mode
  month?: string | null;      // Pre-calculated by backend (e.g. "Sep")
  year?: string | null;       // Pre-calculated by backend (e.g. "2026")
  date_iso?: string | null;   // Pre-calculated by backend (e.g. "2026-09-12")
  items: MediaItemSummary[];
}

export interface MediaPageResponse {
  albums: SubAlbum[];
  breadcrumbs: BreadcrumbSegment[];
  sections: MediaSection[];
  next_cursor_captured_at: string | null;
  next_cursor_id: string | null;
  has_more: boolean;
}