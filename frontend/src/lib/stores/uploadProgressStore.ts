import { writable } from 'svelte/store';
import { authStore } from '$lib/stores/authStore';
import { CHUNK_THRESHOLD_BYTES, uploadChunked } from '$lib/utils/uploader';

export interface ActiveFileProgress {
  fileIdx: number;
  name: string;
  size: number;
  loaded: number;
  percent: number;
  status: 'uploading' | 'chunking' | 'completed' | 'failed';
}

export interface UploadProgressState {
  isOpen: boolean;
  isMinimized: boolean;
  totalFiles: number;
  completedFiles: number;
  failedFiles: number;
  totalBytes: number;
  loadedBytes: number;
  startTime: number;
  activeUploads: ActiveFileProgress[];
  error: string | null;
}

const initialState: UploadProgressState = {
  isOpen: false,
  isMinimized: false,
  totalFiles: 0,
  completedFiles: 0,
  failedFiles: 0,
  totalBytes: 0,
  loadedBytes: 0,
  startTime: 0,
  activeUploads: [],
  error: null
};

function createUploadProgressStore() {
  const { subscribe, update, set } = writable<UploadProgressState>(initialState);

  return {
    subscribe,
    toggleMinimize: () => update((s) => ({ ...s, isMinimized: !s.isMinimized })),
    close: () => set(initialState),

    startBatch: async (
      files: File[],
      folderPath: string,
      onDone?: (summary: { completed: number; failed: number }) => void
    ) => {
      const totalFiles = files.length;
      if (totalFiles === 0) {
        onDone?.({ completed: 0, failed: 0 });
        return;
      }

      const totalBytes = files.reduce((acc, f) => acc + f.size, 0);

      set({
        isOpen: true,
        isMinimized: false,
        totalFiles,
        completedFiles: 0,
        failedFiles: 0,
        totalBytes,
        loadedBytes: 0,
        startTime: Date.now(),
        activeUploads: [],
        error: null
      });

      // Max slots for regular photos; large chunked items run with strict single concurrency
      const MAX_PHOTO_CONCURRENCY = 2;
      let nextIdx = 0;
      let completedCount = 0;
      let failedCount = 0;

      // Track individual file bytes loaded to ensure accurate global progress
      const fileBytesMap = new Map<number, number>();

      function recalculateLoadedBytes(): number {
        let sum = 0;
        for (const bytes of fileBytesMap.values()) {
          sum += bytes;
        }
        return sum;
      }

      const uploadSingle = (file: File, folder: string, fileIdx: number): Promise<void> => {
        return new Promise((resolve, reject) => {
          const xhr = new XMLHttpRequest();
          const formData = new FormData();
          formData.append('file', file);
          if (folder.trim()) formData.append('folder', folder.trim());

          xhr.upload.onprogress = (e) => {
            if (e.lengthComputable) {
              fileBytesMap.set(fileIdx, e.loaded);
              const currentLoaded = recalculateLoadedBytes();

              update((s) => ({
                ...s,
                loadedBytes: currentLoaded,
                activeUploads: s.activeUploads.map((item) =>
                  item.fileIdx === fileIdx
                    ? {
                        ...item,
                        loaded: e.loaded,
                        percent: Math.min(100, Math.round((e.loaded / e.total) * 100))
                      }
                    : item
                )
              }));
            }
          };

          xhr.onload = () => {
            if (xhr.status >= 200 && xhr.status < 300) {
              fileBytesMap.set(fileIdx, file.size);
              resolve();
            } else if (xhr.status === 401) {
              authStore.checkStatus();
              reject(new Error('Session expired.'));
            } else {
              reject(new Error(xhr.responseText || `Status ${xhr.status}`));
            }
          };

          xhr.onerror = () => reject(new Error('Network connection error'));
          xhr.open('POST', '/api/upload');
          xhr.withCredentials = true;
          xhr.send(formData);
        });
      };

      const worker = async () => {
        while (nextIdx < totalFiles) {
          const fileIdx = nextIdx++;
          const file = files[fileIdx];
          const isLargeFile = file.size > CHUNK_THRESHOLD_BYTES;

          fileBytesMap.set(fileIdx, 0);

          // Register in UI
          update((s) => ({
            ...s,
            activeUploads: [
              ...s.activeUploads,
              {
                fileIdx,
                name: file.name,
                size: file.size,
                loaded: 0,
                percent: 0,
                status: isLargeFile ? 'chunking' : 'uploading'
              }
            ]
          }));

          try {
            if (isLargeFile) {
              await uploadChunked(file, folderPath, (part, totalParts) => {
                const approxLoaded = Math.min(file.size, (part / totalParts) * file.size);
                fileBytesMap.set(fileIdx, approxLoaded);
                const currentLoaded = recalculateLoadedBytes();

                update((s) => ({
                  ...s,
                  loadedBytes: currentLoaded,
                  activeUploads: s.activeUploads.map((item) =>
                    item.fileIdx === fileIdx
                      ? {
                          ...item,
                          loaded: approxLoaded,
                          percent: Math.min(100, Math.round((part / totalParts) * 100))
                        }
                      : item
                  )
                }));
              });
              // Ensure 100% byte count on completion
              fileBytesMap.set(fileIdx, file.size);
            } else {
              await uploadSingle(file, folderPath, fileIdx);
            }

            completedCount++;
            const currentLoaded = recalculateLoadedBytes();

            // Clear finished item from active progress list
            update((s) => ({
              ...s,
              completedFiles: completedCount,
              loadedBytes: currentLoaded,
              activeUploads: s.activeUploads.filter((item) => item.fileIdx !== fileIdx)
            }));
          } catch (err: any) {
            failedCount++;
            // Revert failed partial bytes so global math stays accurate
            fileBytesMap.set(fileIdx, 0);
            const currentLoaded = recalculateLoadedBytes();

            update((s) => ({
              ...s,
              failedFiles: failedCount,
              loadedBytes: currentLoaded,
              activeUploads: s.activeUploads.map((item) =>
                item.fileIdx === fileIdx ? { ...item, status: 'failed', percent: 0 } : item
              ),
              error: `Upload failed for "${file.name}": ${err.message}`
            }));
          }
        }
      };

      try {
        // Run workers with controlled pool size
        const poolSize = Math.min(MAX_PHOTO_CONCURRENCY, totalFiles);
        const pool = Array.from({ length: poolSize }, () => worker());
        await Promise.all(pool);
      } catch (e: any) {
        update((s) => ({ ...s, error: e.message || 'Batch upload encountered an unexpected error.' }));
      } finally {
        onDone?.({ completed: completedCount, failed: failedCount });
      }
    }
  };
}

export const uploadProgressStore = createUploadProgressStore();