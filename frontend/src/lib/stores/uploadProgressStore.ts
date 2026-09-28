// photo-app/frontend/src/lib/stores/uploadProgressStore.ts
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
  totalBytes: number;
  loadedBytes: number;
  startTime: number;
  activeUploads: ActiveFileProgress[]; // Array for direct Svelte reactivity
  error: string | null;
}

const initialState: UploadProgressState = {
  isOpen: false,
  isMinimized: false,
  totalFiles: 0,
  completedFiles: 0,
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

    startBatch: async (files: File[], folderPath: string, onDone: () => void) => {
      const totalFiles = files.length;
      const totalBytes = files.reduce((acc, f) => acc + f.size, 0);

      set({
        isOpen: true,
        isMinimized: false,
        totalFiles,
        completedFiles: 0,
        totalBytes,
        loadedBytes: 0,
        startTime: Date.now(),
        activeUploads: [],
        error: null
      });

      const CONCURRENCY = 3;
      let nextIdx = 0;
      let completedCount = 0;
      let totalLoadedAccumulator = 0;
      const fileBytesLoaded = new Array(totalFiles).fill(0);

      const uploadSingle = (file: File, folder: string, fileIdx: number): Promise<void> => {
        return new Promise((resolve, reject) => {
          const xhr = new XMLHttpRequest();
          const formData = new FormData();
          formData.append('file', file);
          if (folder.trim()) formData.append('folder', folder.trim());

          xhr.upload.onprogress = (e) => {
            if (e.lengthComputable) {
              const diff = e.loaded - fileBytesLoaded[fileIdx];
              fileBytesLoaded[fileIdx] = e.loaded;
              totalLoadedAccumulator += diff;

              update((s) => {
                const updatedActive = s.activeUploads.map((item) => {
                  if (item.fileIdx === fileIdx) {
                    return {
                      ...item,
                      loaded: e.loaded,
                      percent: Math.round((e.loaded / e.total) * 100)
                    };
                  }
                  return item;
                });
                return { ...s, loadedBytes: totalLoadedAccumulator, activeUploads: updatedActive };
              });
            }
          };

          xhr.onload = () => {
            if (xhr.status >= 200 && xhr.status < 300) {
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

          // Add to active uploads array (up to 3 concurrent slots)
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
                status: file.size > CHUNK_THRESHOLD_BYTES ? 'chunking' : 'uploading'
              }
            ]
          }));

          try {
            if (file.size > CHUNK_THRESHOLD_BYTES) {
              let prevPartLoaded = 0;
              await uploadChunked(file, folderPath, (part, totalParts) => {
                const approxLoaded = Math.min(file.size, (part / totalParts) * file.size);
                const diff = approxLoaded - prevPartLoaded;
                prevPartLoaded = approxLoaded;
                totalLoadedAccumulator += diff;

                update((s) => {
                  const updatedActive = s.activeUploads.map((item) => {
                    if (item.fileIdx === fileIdx) {
                      return {
                        ...item,
                        loaded: approxLoaded,
                        percent: Math.round((part / totalParts) * 100)
                      };
                    }
                    return item;
                  });
                  return { ...s, loadedBytes: totalLoadedAccumulator, activeUploads: updatedActive };
                });
              });
            } else {
              await uploadSingle(file, folderPath, fileIdx);
            }

            completedCount++;
            // Remove completed item from active slots array
            update((s) => ({
              ...s,
              completedFiles: completedCount,
              activeUploads: s.activeUploads.filter((item) => item.fileIdx !== fileIdx)
            }));
          } catch (err: any) {
            update((s) => ({
              ...s,
              activeUploads: s.activeUploads.map((item) =>
                item.fileIdx === fileIdx ? { ...item, status: 'failed' } : item
              ),
              error: `Failed "${file.name}": ${err.message}`
            }));
          }
        }
      };

      try {
        const pool = Array.from({ length: Math.min(CONCURRENCY, totalFiles) }, () => worker());
        await Promise.all(pool);
        onDone();
      } catch (e: any) {
        update((s) => ({ ...s, error: e.message || 'Batch upload failed.' }));
      }
    }
  };
}

export const uploadProgressStore = createUploadProgressStore();