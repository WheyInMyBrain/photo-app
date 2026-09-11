// frontend/src/lib/api/client.ts
import type { MediaSummary, PersonCard, AssetFaceDetail, AssetTagItem } from '$lib/types/api';

const BASE = ''; // Relies on Vite proxy during dev

export const api = {
	async getTimeline(isPrivate = false): Promise<MediaSummary[]> {
		const res = await fetch(`${BASE}/api/photos?is_private=${isPrivate}`);
		if (!res.ok) throw new Error('Failed to load photos');
		return res.json();
	},

	async getPeople(isPrivate = false): Promise<PersonCard[]> {
		const res = await fetch(`${BASE}/api/smart-albums/people?is_private=${isPrivate}`);
		if (!res.ok) throw new Error('Failed to load people');
		return res.json();
	},

	async getAssetFaces(assetId: string): Promise<AssetFaceDetail[]> {
		const res = await fetch(`${BASE}/api/assets/${assetId}/faces`);
		if (!res.ok) throw new Error('Failed to load asset faces');
		return res.json();
	},

	async getAssetTags(assetId: string): Promise<AssetTagItem[]> {
		const res = await fetch(`${BASE}/api/assets/${assetId}/tags`);
		if (!res.ok) throw new Error('Failed to load asset tags');
		return res.json();
	},

	async toggleFavorite(assetId: string): Promise<void> {
		await fetch(`${BASE}/api/assets/${assetId}/favorite`, { method: 'POST' });
	}
};