export interface FaceDetail {
  face_id: string;
  person_id: string | null;
  person_name: string | null;
  face_thumb_path: string;
  bbox_x: number;
  bbox_y: number;
  bbox_w: number;
  bbox_h: number;
  score: number;
}

export interface TagItem {
  name: string;
  confidence: number;
}

export interface PersonCandidate {
  id: string;
  name: string | null;
}

export interface SimilarItem {
  id: string;
  thumb_path: string;
  mime_type: string;
  similarity: number;
}