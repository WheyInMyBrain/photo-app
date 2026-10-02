// photo-app/frontend/src/lib/api/upload.ts
import { CHUNK_SIZE_BYTES, generateUUID } from '$lib/utils/uploader';

const CHUNK_TIMEOUT_MS = 120_000; // 2 minutes per chunk
const MAX_RETRIES = 3;

export interface DuplicateCheckResult {
  exists: boolean;
  assetId?: string;
  message?: string;
}

export interface StartBatchResult {
  batch_id: string;
}

export interface FinishBatchResult {
  released_jobs: number;
  status: string;
}

/** 1. Start an explicit batch session */
export async function apiStartBatch(): Promise<string | undefined> {
  try {
    const res = await fetch('/api/upload/batch/start', {
      method: 'POST',
      credentials: 'include',
    });
    if (res.ok) {
      const data: StartBatchResult = await res.json();
      return data.batch_id;
    }
  } catch (err) {
    console.warn('Failed to start upload batch session, proceeding direct:', err);
  }
  return undefined;
}

/** 2. Check for duplicate by SHA256 */
export async function apiCheckDuplicate(
  sha256: string,
  fileName: string,
  folderPath: string
): Promise<DuplicateCheckResult> {
  const cleanFolder = folderPath.trim();
  try {
    const res = await fetch('/api/upload/check', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({
        sha256,
        file_name: fileName,
        folder: cleanFolder || undefined,
      }),
    });

    if (res.ok) {
      const data = await res.json();
      return {
        exists: Boolean(data.exists),
        assetId: data.asset_id,
        message: data.message,
      };
    }
  } catch (err) {
    console.warn('Duplicate pre-check failed, continuing to upload:', err);
  }
  return { exists: false };
}

/** 3. Upload a single file using XHR for granular progress tracking */
export function apiUploadSingleXhr(
  file: File,
  folderPath: string,
  batchId?: string,
  onProgress?: (loaded: number, total: number) => void
): Promise<void> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    const formData = new FormData();
    formData.append('file', file);

    const cleanFolder = folderPath.trim();
    if (cleanFolder) {
      formData.append('folder', cleanFolder);
    }

    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable && onProgress) {
        onProgress(e.loaded, e.total);
      }
    };

    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        resolve();
      } else if (xhr.status === 401) {
        reject(new Error('Session expired.'));
      } else {
        reject(new Error(xhr.responseText || `Status ${xhr.status}`));
      }
    };

    xhr.onerror = () => reject(new Error('Network connection error'));

    const params = new URLSearchParams();
    if (cleanFolder) params.set('folder', cleanFolder);
    if (batchId) params.set('batch_id', batchId);

    const queryStr = params.toString() ? `?${params.toString()}` : '';
    xhr.open('POST', `/api/upload${queryStr}`);
    xhr.withCredentials = true;
    xhr.send(formData);
  });
}

/** Helper: Robust XHR chunk sender with retry and custom timeout */
function sendChunkXhr(url: string, slice: Blob, retries = MAX_RETRIES): Promise<void> {
  return new Promise((resolve, reject) => {
    function attempt(remainingAttempts: number) {
      const xhr = new XMLHttpRequest();
      xhr.open('POST', url, true);
      xhr.withCredentials = true;
      xhr.setRequestHeader('Content-Type', 'application/octet-stream');
      xhr.timeout = CHUNK_TIMEOUT_MS;

      xhr.onload = () => {
        if (xhr.status >= 200 && xhr.status < 300) {
          resolve();
        } else if (xhr.status === 401) {
          reject(new Error('Session expired.'));
        } else if (remainingAttempts > 0 && (xhr.status >= 500 || xhr.status === 408)) {
          setTimeout(() => attempt(remainingAttempts - 1), 1000);
        } else {
          reject(new Error(xhr.responseText || `Chunk status ${xhr.status}`));
        }
      };

      xhr.onerror = () => {
        if (remainingAttempts > 0) {
          setTimeout(() => attempt(remainingAttempts - 1), 1000);
        } else {
          reject(new Error('Network error uploading chunk'));
        }
      };

      xhr.ontimeout = () => {
        if (remainingAttempts > 0) {
          setTimeout(() => attempt(remainingAttempts - 1), 1000);
        } else {
          reject(new Error('Chunk upload timed out after 120s'));
        }
      };

      xhr.send(slice);
    }

    attempt(retries);
  });
}

/** 4. Upload large files via slice chunks and finalize */
export async function apiUploadChunked(
  file: File,
  folderPath: string,
  batchId?: string,
  onProgress?: (part: number, total: number) => void
): Promise<void> {
  const uploadId = generateUUID();
  const totalChunks = Math.max(1, Math.ceil(file.size / CHUNK_SIZE_BYTES));
  const cleanFolder = folderPath.trim();

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

    // Use raw binary XHR stream instead of fetch/AbortController
    await sendChunkXhr(`/api/upload/chunk?${params.toString()}`, slice);

    onProgress?.(chunkIdx + 1, totalChunks);
  }

  const finalizeParams = new URLSearchParams({
    upload_id: uploadId,
    file_name: file.name,
    ...(cleanFolder ? { folder: cleanFolder } : {}),
    ...(batchId ? { batch_id: batchId } : {}),
  });

  const finalizeRes = await fetch(`/api/upload/chunk/finalize?${finalizeParams.toString()}`, {
    method: 'POST',
    credentials: 'include',
  });

  if (!finalizeRes.ok) {
    const errText = await finalizeRes.text().catch(() => '');
    throw new Error(`Failed to finalize "${file.name}": ${errText}`);
  }
}

/** 5. Commit the batch: tell backend to transition staged jobs to pending */
export async function apiFinishBatch(batchId: string): Promise<FinishBatchResult | undefined> {
  try {
    const res = await fetch('/api/upload/batch/finish', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ batch_id: batchId }),
    });
    if (res.ok) {
      return await res.json();
    }
  } catch (err) {
    console.error('Failed committing batch:', err);
  }
  return undefined;
}