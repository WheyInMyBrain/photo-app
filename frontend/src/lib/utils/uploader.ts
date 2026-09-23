export const CHUNK_THRESHOLD_BYTES = 75 * 1024 * 1024; // 75MB
export const CHUNK_SIZE_BYTES = 20 * 1024 * 1024;      // 20MB

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

export async function uploadChunked(
  file: File,
  folderPath: string,
  onProgress?: (part: number, total: number) => void
): Promise<void> {
  const uploadId = crypto.randomUUID();
  const totalChunks = Math.ceil(file.size / CHUNK_SIZE_BYTES);

  for (let chunkIdx = 0; chunkIdx < totalChunks; chunkIdx++) {
    const start = chunkIdx * CHUNK_SIZE_BYTES;
    const end = Math.min(file.size, start + CHUNK_SIZE_BYTES);
    const slice = file.slice(start, end);

    onProgress?.(chunkIdx + 1, totalChunks);

    const params = new URLSearchParams({
      upload_id: uploadId,
      chunk_index: chunkIdx.toString(),
      chunk_size: CHUNK_SIZE_BYTES.toString(),
      total_chunks: totalChunks.toString(),
    });

    const res = await fetch(`/api/upload/chunk?${params.toString()}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/octet-stream' },
      body: slice,
    });

    if (res.status === 401) throw new Error('Session expired.');
    if (!res.ok) throw new Error(`Failed uploading chunk ${chunkIdx + 1}`);
  }

  const finalizeParams = new URLSearchParams({
    upload_id: uploadId,
    file_name: file.name,
    folder: folderPath.trim() || 'root',
  });

  const finalizeRes = await fetch(`/api/upload/chunk/finalize?${finalizeParams.toString()}`, {
    method: 'POST',
  });

  if (!finalizeRes.ok) throw new Error(`Failed to finalize ${file.name}`);
}