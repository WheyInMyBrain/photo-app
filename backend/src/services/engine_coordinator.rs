use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::services::clip_cache::ClipCacheManager;
use crate::services::cluster_cache::{ClusterCacheManager, SharedClusterCache};

use media_processing::{ClipEngine, FaceEngine, MediaEngine, TagEngine};

/// Generic container that manages a resource's lazy loading and idle eviction.
struct ManagedResource<T> {
    name: &'static str,
    instance: RwLock<Option<Arc<T>>>,
    last_accessed: Mutex<Instant>,
    idle_timeout: Duration,
}

// Direct FFI bindings to tell allocators to release memory arenas to the OS
extern "C" {
    // mimalloc's internal collect function (force = true releases cached pages)
    fn mi_collect(force: bool);

    #[cfg(target_os = "macos")]
    fn malloc_zone_pressure_relief(zone: *mut libc::c_void, goal: libc::size_t) -> libc::size_t;
}

/// Returns current physical RAM usage (RSS) in Megabytes
pub fn get_process_rss_mb() -> f64 {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::mem::MaybeUninit;

        #[allow(deprecated)]
        let task = libc::mach_task_self();

        let mut info = MaybeUninit::<libc::mach_task_basic_info>::uninit();
        let mut count = (std::mem::size_of::<libc::mach_task_basic_info>()
            / std::mem::size_of::<libc::natural_t>()) as libc::mach_msg_type_number_t;

        let kerr = libc::task_info(
            task,
            libc::MACH_TASK_BASIC_INFO,
            info.as_mut_ptr() as libc::task_info_t,
            &mut count,
        );

        if kerr == libc::KERN_SUCCESS {
            let info = info.assume_init();
            return (info.resident_size as f64) / (1024.0 * 1024.0);
        }
        0.0
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let parts: Vec<&str> = statm.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(pages) = parts[1].parse::<u64>() {
                    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 };
                    return ((pages * page_size) as f64) / (1024.0 * 1024.0);
                }
            }
        }
        0.0
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0.0
    }
}

impl<T: Send + Sync + 'static> ManagedResource<T> {
    fn new(name: &'static str, idle_timeout_secs: u64) -> Self {
        Self {
            name,
            instance: RwLock::new(None),
            last_accessed: Mutex::new(Instant::now()),
            idle_timeout: Duration::from_secs(idle_timeout_secs),
        }
    }

    async fn get_or_load<F, Fut, E>(&self, loader: F) -> Result<Arc<T>, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        // 1. Fast read-lock check
        {
            let read_guard = self.instance.read().await;
            if let Some(existing) = &*read_guard {
                *self.last_accessed.lock().await = Instant::now();
                return Ok(existing.clone());
            }
        }

        // 2. Write-lock check and load
        let mut write_guard = self.instance.write().await;
        if let Some(existing) = &*write_guard {
            *self.last_accessed.lock().await = Instant::now();
            return Ok(existing.clone());
        }

        let before_ram = get_process_rss_mb();
        info!(
            "[RAM: {:.2} MB] -> Loading '{}' into memory...",
            before_ram, self.name
        );

        let start = Instant::now();
        let loaded = loader().await?;
        let arc = Arc::new(loaded);
        *write_guard = Some(arc.clone());
        *self.last_accessed.lock().await = Instant::now();

        let after_ram = get_process_rss_mb();
        info!(
            "[RAM: {:.2} MB] -> '{}' loaded in {:.2?}. (Delta: +{:.2} MB)",
            after_ram,
            self.name,
            start.elapsed(),
            (after_ram - before_ram).max(0.0)
        );

        Ok(arc)
    }

    async fn touch(&self) {
        *self.last_accessed.lock().await = Instant::now();
    }

    async fn try_evict(&self) {
        let elapsed = {
            let last = self.last_accessed.lock().await;
            last.elapsed()
        };

        if elapsed > self.idle_timeout {
            let mut write_guard = self.instance.write().await;
            if write_guard.is_some() {
                let before_ram = get_process_rss_mb();
                info!(
                    "[RAM: {:.2} MB] -> Idle timeout reached for '{}'. Evicting...",
                    before_ram, self.name
                );

                *write_guard = None; // Drops Rust & C++ session handles

                // Force mimalloc and macOS system allocator to release cached arenas
                unsafe {
                    mi_collect(true);

                    #[cfg(target_os = "macos")]
                    malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
                }

                // Brief pause so the OS kernel updates resident page statistics
                tokio::time::sleep(Duration::from_millis(50)).await;

                let after_ram = get_process_rss_mb();
                info!(
                    "[RAM: {:.2} MB] -> Evicted '{}'. (Reclaimed: {:.2} MB)",
                    after_ram,
                    self.name,
                    (before_ram - after_ram).max(0.0)
                );
            }
        }
    }
}

/// Central coordinator for all transient, heavy resources:
/// - Tier 2: Vector caches (ClipCache ~20MB, ClusterCache ~1MB)
/// - Tier 3: AI Inference Engines (ClipEngine ~150MB, Full MediaEngine ~2GB)
#[derive(Clone)]
pub struct EngineCoordinator {
    pool: SqlitePool,
    models_dir: PathBuf,

    // Tier 2: In-memory vector caches (5-minute idle eviction)
    clip_cache: Arc<ManagedResource<ClipCacheManager>>,
    cluster_cache: Arc<ManagedResource<SharedClusterCache>>,

    // Tier 3: AI Inference engines (3-minute idle eviction)
    clip_engine: Arc<ManagedResource<ClipEngine>>,
    media_engine: Arc<ManagedResource<MediaEngine>>,
}

impl EngineCoordinator {
    pub fn new(pool: SqlitePool, models_dir: PathBuf) -> Self {
        let coordinator = Self {
            pool,
            models_dir,
            // Caches: 5 minutes idle timeout (300s)
            clip_cache: Arc::new(ManagedResource::new("ClipCache", 300)),
            cluster_cache: Arc::new(ManagedResource::new("ClusterCache", 300)),
            // ONNX Models: 3 minutes idle timeout (180s)
            clip_engine: Arc::new(ManagedResource::new("ClipEngine", 180)),
            media_engine: Arc::new(ManagedResource::new("MediaEngine", 180)),
        };

        // Start a single lightweight background task to monitor idle evictions every 30 seconds
        let weak_clip_cache = Arc::downgrade(&coordinator.clip_cache);
        let weak_cluster_cache = Arc::downgrade(&coordinator.cluster_cache);
        let weak_clip_engine = Arc::downgrade(&coordinator.clip_engine);
        let weak_media_engine = Arc::downgrade(&coordinator.media_engine);

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;

                // If all containers have dropped, app is shutting down
                let c_cache = weak_clip_cache.upgrade();
                let cl_cache = weak_cluster_cache.upgrade();
                let c_engine = weak_clip_engine.upgrade();
                let m_engine = weak_media_engine.upgrade();

                if c_cache.is_none() && cl_cache.is_none() && c_engine.is_none() && m_engine.is_none() {
                    break;
                }

                if let Some(r) = c_cache { r.try_evict().await; }
                if let Some(r) = cl_cache { r.try_evict().await; }
                if let Some(r) = c_engine { r.try_evict().await; }
                if let Some(r) = m_engine { r.try_evict().await; }
            }
        });

        coordinator
    }

    // -------------------------------------------------------------------------
    // Tier 2: Lightweight Cache Accessors (~20 MB total)
    // -------------------------------------------------------------------------

    /// Ensures the CLIP vector embedding cache is ready in RAM (for similarity lookups)
    pub async fn ensure_clip_cache(&self) -> Result<Arc<ClipCacheManager>, String> {
        let pool = self.pool.clone();
        self.clip_cache
            .get_or_load(|| async move {
                ClipCacheManager::load_initial(&pool)
                    .await
                    .map_err(|e| e.to_string())
            })
            .await
    }

    /// Ensures the Face Cluster centroids are ready in RAM
    pub async fn ensure_cluster_cache(&self) -> Result<SharedClusterCache, String> {
        let pool = self.pool.clone();
        let arc_shared = self
            .cluster_cache
            .get_or_load(|| async move {
                ClusterCacheManager::load_initial(&pool)
                    .await
                    .map_err(|e| e.to_string())
            })
            .await?;

        // SharedClusterCache is already Arc<RwLock<Vec<KnownPersonCluster>>>
        Ok((*arc_shared).clone())
    }

    // -------------------------------------------------------------------------
    // Tier 3: AI Inference Model Accessors
    // -------------------------------------------------------------------------

    /// Ensures only the CLIP text encoder is loaded (~150 MB) for natural language search queries
    pub async fn ensure_search_engine(&self) -> Result<Arc<ClipEngine>, String> {
        let m_dir = self.models_dir.clone();
        self.clip_engine
            .get_or_load(|| async move {
                tokio::task::spawn_blocking(move || {
                    ClipEngine::init(&m_dir).map_err(|e| e.to_string())
                })
                .await
                .map_err(|e| e.to_string())?
            })
            .await
    }

    /// Ensures the full pipeline engine is loaded (~2 GB) for processing ingested uploads
    pub async fn ensure_pipeline_engine(&self) -> Result<Arc<MediaEngine>, String> {
        let m_dir = self.models_dir.clone();
        self.media_engine
            .get_or_load(|| async move {
                tokio::task::spawn_blocking(move || -> Result<MediaEngine, String> {
                    let face = Arc::new(FaceEngine::init(&m_dir).map_err(|e| e.to_string())?);
                    let tag = Arc::new(TagEngine::init(&m_dir).map_err(|e| e.to_string())?);
                    let clip = Arc::new(ClipEngine::init(&m_dir).map_err(|e| e.to_string())?);
                    Ok(MediaEngine::new(face, tag, clip))
                })
                .await
                .map_err(|e| e.to_string())?
            })
            .await
    }

    /// Touch running resources so they stay hot while a long batch is actively processing
    pub async fn keep_warm(&self) {
        self.clip_cache.touch().await;
        self.cluster_cache.touch().await;
        self.clip_engine.touch().await;
        self.media_engine.touch().await;
    }
}