// frontend/src/lib/types/api.ts

export interface MediaSummary {
	id: string;
	file_name: string;
	thumb_path: string;
	preview_path: string;
	aspect_ratio: number;
	duration_seconds?: number;
	mime_type: string;
	captured_at?: string;
}

export interface PersonCard {
	id: string;
	name?: string;
	face_count: number;
	avatar_thumb?: string;
}

export interface AssetFaceDetail {
	face_id: string;
	person_id?: string;
	person_name?: string;
	face_thumb_path: string;
	bbox_x: number;
	bbox_y: number;
	bbox_w: number;
	bbox_h: number;
	score: number;
	is_verified: boolean;
}

export interface AssetTagItem {
	tag_id: number;
	name: string;
	confidence: number;
	source: 'AI' | 'USER';
}