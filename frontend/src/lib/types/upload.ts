export interface CandidateItem {
  id: string;
  media_type: string;
  thumbnail_url?: string;
  thumbnail_base64?: string;
  high_res_url: string;
  audio_url?: string;
  suggested_filename: string;
}

export interface InspectPreview {
  suggested_folder: string;
  platform: string;
  author: string;
  caption: string;
  total_items: number;
  items: CandidateItem[];
}