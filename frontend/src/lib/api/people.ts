// photo-app/frontend/src/lib/api/people.ts

export interface PersonCard {
  id: string;
  name: string | null;
  face_count: number;
  avatar_thumb: string | null;
}

export interface NameDirectoryItem {
  id: string;
  name: string | null;
}

export interface AssetFaceDetail {
  face_id: string;
  person_id: string | null;
  person_name: string | null;
  face_thumb_path: string;
  bbox_x: number;
  bbox_y: number;
  bbox_w: number;
  bbox_h: number;
  score: number;
  is_verified: boolean;
}

export async function fetchPeopleOverview(): Promise<PersonCard[]> {
  const res = await fetch('/api/smart-albums/people', { credentials: 'include' });
  if (!res.ok) throw new Error('Failed to load people identities');
  return res.json();
}

export async function fetchNamesDirectory(): Promise<NameDirectoryItem[]> {
  const res = await fetch('/api/persons/names', { credentials: 'include' });
  if (!res.ok) throw new Error('Failed to load name suggestions');
  return res.json();
}

export async function renamePerson(personId: string, name: string): Promise<boolean> {
  const res = await fetch(`/api/persons/${personId}/name`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({ name })
  });
  if (!res.ok) throw new Error('Failed to rename person');
  return res.json();
}

export async function mergePersons(
  sourcePersonId: string,
  targetPersonId: string
): Promise<boolean> {
  const res = await fetch('/api/persons/merge', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({
      source_person_id: sourcePersonId,
      target_person_id: targetPersonId
    })
  });
  if (!res.ok) throw new Error('Failed to merge persons');
  return res.json();
}

export async function deletePerson(personId: string): Promise<boolean> {
  const res = await fetch(`/api/persons/${personId}`, {
    method: 'DELETE',
    credentials: 'include'
  });
  if (!res.ok) throw new Error('Failed to delete person');
  return res.json();
}

export async function deleteFace(faceId: string): Promise<boolean> {
  const res = await fetch(`/api/faces/${faceId}`, {
    method: 'DELETE',
    credentials: 'include'
  });
  if (!res.ok) throw new Error('Failed to delete face');
  return res.json();
}