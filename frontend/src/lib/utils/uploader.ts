// photo-app/frontend/src/lib/utils/uploader.ts

export const CHUNK_THRESHOLD_BYTES = 75 * 1024 * 1024; // 75MB
export const CHUNK_SIZE_BYTES = 20 * 1024 * 1024;      // 20MB
const MAX_RETRIES = 3;
const CHUNK_TIMEOUT_MS = 30_000; // 60s hard timeout per chunk

// Universal RFC4122 v4 UUID generator (works on HTTP, HTTPS, and mobile Safari)
export function generateUUID(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    try {
      return crypto.randomUUID();
    } catch {}
  }

  // Fallback for non-HTTPS local IP access & older mobile WebKit
  if (typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function') {
    return (([1e7] as any) + -1e3 + -4e3 + -8e3 + -1e11).replace(/[018]/g, (c: number) =>
      (c ^ (crypto.getRandomValues(new Uint8Array(1))[0] & (15 >> (c / 4)))).toString(16)
    );
  }

  // Pure Math.random fallback
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === 'x' ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

export async function uploadDirect(file: File, folderPath: string): Promise<void> {
  const formData = new FormData();
  formData.append('folder', folderPath.trim() || 'root');
  formData.append('file', file);

  const res = await fetch('/api/upload', { method: 'POST', body: formData });
  if (res.status === 401) throw new Error('Session expired.');
  if (!res.ok) throw new Error(await res.text());
}

async function fetchWithRetryAndTimeout(
  url: string,
  options: RequestInit,
  retries = MAX_RETRIES
): Promise<Response> {
  for (let attempt = 0; attempt < retries; attempt++) {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), CHUNK_TIMEOUT_MS);

    try {
      const res = await fetch(url, {
        ...options,
        signal: controller.signal,
      });
      clearTimeout(timeoutId);

      // Immediately bubble 401 Unauthorized without retrying
      if (res.status === 401) {
        throw new Error('Session expired.');
      }

      if (res.ok) {
        return res;
      }

      // If server returned 4xx (except 408/429), retry will not help
      if (res.status >= 400 && res.status < 500 && res.status !== 408 && res.status !== 429) {
        throw new Error(`Server rejected chunk with status ${res.status}`);
      }
    } catch (err: any) {
      clearTimeout(timeoutId);
      if (err.message === 'Session expired.') throw err;
      if (attempt === retries - 1) throw err;

      // Exponential backoff before retry (1s, 2s, etc.)
      await new Promise((resolve) => setTimeout(resolve, 1000 * (attempt + 1)));
    }
  }
  throw new Error('Upload request failed after retries.');
}

export async function uploadChunked(
  file: File,
  folderPath: string,
  onProgress?: (part: number, total: number) => void
): Promise<void> {
  const uploadId = generateUUID();
  const totalChunks = Math.max(1, Math.ceil(file.size / CHUNK_SIZE_BYTES));

  for (let chunkIdx = 0; chunkIdx < totalChunks; chunkIdx++) {
    const start = chunkIdx * CHUNK_SIZE_BYTES;
    const end = Math.min(file.size, start + CHUNK_SIZE_BYTES);
    const slice = file.slice(start, end);

    const params = new URLSearchParams({
      upload_id: uploadId,
      chunk_index: chunkIdx.toString(),
      chunk_size: CHUNK_SIZE_BYTES.toString(),
      total_chunks: totalChunks.toString(),
    });

    try {
      await fetchWithRetryAndTimeout(`/api/upload/chunk?${params.toString()}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/octet-stream' },
        body: slice,
      });
    } catch (err: any) {
      throw new Error(`Failed uploading chunk ${chunkIdx + 1} of ${totalChunks}: ${err.message}`);
    }

    onProgress?.(chunkIdx + 1, totalChunks);
  }

  const finalizeParams = new URLSearchParams({
    upload_id: uploadId,
    file_name: file.name,
    folder: folderPath.trim() || 'root',
  });

  const finalizeRes = await fetchWithRetryAndTimeout(
    `/api/upload/chunk/finalize?${finalizeParams.toString()}`,
    { method: 'POST' }
  );

  if (!finalizeRes.ok) {
    throw new Error(`Failed to finalize ${file.name}`);
  }
}