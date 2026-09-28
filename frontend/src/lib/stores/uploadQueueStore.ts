// photo-app/frontend/src/lib/stores/uploadQueueStore.ts
import { writable } from 'svelte/store';
import { authStore } from '$lib/stores/authStore';
import {
  CHUNK_THRESHOLD_BYTES,
  uploadChunked
} from '$lib/utils/uploader';
import type { InspectPreview, UploadMode } from '$lib/types/upload';

export interface UploadQueueState {
  isUploading: boolean;
  uploadProgress: number;
  statusMessage: string;
  uploadError: string;
  uploadMode: UploadMode;
  folderPath: string;
  stagedFiles: File[];
  linkUrl: string;
  linkPreview: InspectPreview | null;
  selectedLinkItems: Set<string>;
}

const initialState: UploadQueueState = {
  isUploading: false,
  uploadProgress: 0,
  statusMessage: '',
  uploadError: '',
  uploadMode: 'files',
  folderPath: '',
  stagedFiles: [],
  linkUrl: '',
  linkPreview: null,
  selectedLinkItems: new Set()
};

function uploadSingleWithProgress(
  file: File,
  folder: string,
  onProgress?: (percent: number) => void
): Promise<void> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    const formData = new FormData();
    formData.append('file', file);
    if (folder.trim()) {
      formData.append('folder', folder.trim());
    }

    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable && onProgress) {
        onProgress(Math.round((e.loaded / e.total) * 100));
      }
    };

    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        resolve();
      } else if (xhr.status === 401) {
        authStore.checkStatus();
        reject(new Error('Session expired. Please log in again.'));
      } else {
        reject(new Error(xhr.responseText || `Upload failed with status ${xhr.status}`));
      }
    };

    xhr.onerror = () => reject(new Error('Network error or connection terminated.'));
    xhr.open('POST', '/api/upload');
    xhr.withCredentials = true;
    xhr.send(formData);
  });
}

function createUploadQueueStore() {
  const { subscribe, update, set } = writable<UploadQueueState>(initialState);

  return {
    subscribe,
    setMode: (mode: UploadMode) => update((s) => ({ ...s, uploadMode: mode, uploadError: '' })),
    setFolderPath: (path: string) => update((s) => ({ ...s, folderPath: path })),
    setLinkUrl: (url: string) => update((s) => ({ ...s, linkUrl: url })),
    clearLinkPreview: () => update((s) => ({ ...s, linkPreview: null, uploadError: '' })),
    
    addFiles: (files: FileList | File[]) => {
      const valid = Array.from(files).filter(
        (f) =>
          f.type.startsWith('image/') ||
          f.type.startsWith('video/') ||
          /\.(heic|heif|mov|mp4|jpg|jpeg|png|webp|gif|mkv|webm)$/i.test(f.name)
      );
      update((s) => ({ ...s, stagedFiles: [...s.stagedFiles, ...valid] }));
    },

    removeFile: (index: number) => {
      update((s) => ({ ...s, stagedFiles: s.stagedFiles.filter((_, i) => i !== index) }));
    },

    toggleLinkItem: (id: string) => {
      update((s) => {
        const next = new Set(s.selectedLinkItems);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return { ...s, selectedLinkItems: next };
      });
    },

    inspectLink: async () => {
      let currentUrl = '';
      update((s) => {
        currentUrl = s.linkUrl.trim();
        return { ...s, isUploading: true, uploadError: '', statusMessage: 'Inspecting link...' };
      });

      if (!currentUrl) return;

      try {
        const res = await fetch('/api/upload/inspect', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ url: currentUrl })
        });

        if (res.status === 401) {
          authStore.checkStatus();
          throw new Error('Session expired.');
        }

        if (!res.ok) {
          const errorText = await res.text();
          throw new Error(errorText || `Server error (${res.status})`);
        }

        const data = await res.json();
        const previewData: InspectPreview | null = data.Preview ?? (data.items ? data : null);

        if (previewData && Array.isArray(previewData.items)) {
          update((s) => ({
            ...s,
            linkPreview: previewData,
            folderPath: previewData.suggested_folder || s.folderPath,
            selectedLinkItems: new Set(previewData.items.map((i) => i.id)),
            isUploading: false,
            statusMessage: ''
          }));
        } else {
          throw new Error('No supported media found.');
        }
      } catch (e: any) {
        update((s) => ({ ...s, isUploading: false, uploadError: e.message || 'Inspection failed.' }));
      }
    },

    executeUpload: async (onComplete: (count: number) => void) => {
      let state: UploadQueueState;
      const unsubscribe = subscribe((s) => (state = s));
      unsubscribe();

      if (state!.isUploading) return;

      // Web link import path
      if (state!.uploadMode === 'link') {
        if (!state!.linkPreview || state!.selectedLinkItems.size === 0) return;
        update((s) => ({ ...s, isUploading: true, uploadProgress: 15, statusMessage: 'Importing items...' }));

        const payload = {
          platform: state!.linkPreview.platform,
          folder: state!.folderPath.trim().replace(/^\/+|\/+$/g, '') || undefined,
          selected_items: state!.linkPreview.items.filter((i) => state!.selectedLinkItems.has(i.id))
        };

        try {
          const res = await fetch('/api/upload/commit', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(payload)
          });
          if (!res.ok) throw new Error(await res.text());
          const data = await res.json();
          onComplete(data.total_uploaded || payload.selected_items.length);
        } catch (e: any) {
          update((s) => ({ ...s, isUploading: false, uploadError: `Import failed: ${e.message}` }));
        }
        return;
      }

      // Parallel file upload path (Concurrency = 3)
      const totalCount = state!.stagedFiles.length;
      if (totalCount === 0) return;

      update((s) => ({
        ...s,
        isUploading: true,
        uploadError: '',
        uploadProgress: 0,
        statusMessage: `Uploading ${totalCount} items...`
      }));

      const progressMap = new Map<number, number>();
      let completedCount = 0;
      let nextQueueIdx = 0;
      const cleanPath = state!.folderPath.trim().replace(/^\/+|\/+$/g, '');

      function updateAggregate() {
        let sum = 0;
        for (let i = 0; i < totalCount; i++) sum += progressMap.get(i) || 0;
        update((s) => ({ ...s, uploadProgress: Math.round(sum / totalCount) }));
      }

      async function worker() {
        while (nextQueueIdx < totalCount) {
          const fileIdx = nextQueueIdx++;
          const file = state!.stagedFiles[fileIdx];

          try {
            if (file.size > CHUNK_THRESHOLD_BYTES) {
              await uploadChunked(file, cleanPath, (part, total) => {
                progressMap.set(fileIdx, Math.round((part / total) * 100));
                updateAggregate();
              });
            } else {
              await uploadSingleWithProgress(file, cleanPath, (pct) => {
                progressMap.set(fileIdx, pct);
                updateAggregate();
              });
            }

            progressMap.set(fileIdx, 100);
            completedCount++;
            updateAggregate();
            update((s) => ({ ...s, statusMessage: `Uploaded ${completedCount}/${totalCount}` }));
          } catch (err: any) {
            throw new Error(`"${file.name}": ${err.message || err}`);
          }
        }
      }

      try {
        const pool = Array.from({ length: Math.min(3, totalCount) }, () => worker());
        await Promise.all(pool);
        onComplete(totalCount);
      } catch (e: any) {
        update((s) => ({ ...s, isUploading: false, uploadError: e.message || 'Upload failed.' }));
      }
    },

    reset: () => set(initialState)
  };
}

export const uploadQueue = createUploadQueueStore();