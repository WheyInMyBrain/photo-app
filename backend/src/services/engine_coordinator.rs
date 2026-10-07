// photo-app/backend/src/services/engine_coordinator.rs

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sqlx::PgPool;
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::config::Config;
use crate::services::cache::{ClusterCacheManager, SharedClusterCache};

use media_processing::{ClipEngine, FaceEngine, MediaEngine, TagEngine, YoloEngine};

/// Generic container that manages a resource's lazy loading and idle eviction.
struct ManagedResource<T> {
    name: &'static str,
    instance: RwLock<Option<Arc<T>>>,
    last_accessed: Mutex<Instant>,
    idle_timeout: Duration,
}

extern "C" {
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
        {
            let read_guard = self.instance.read().await;
            if let Some(existing) = &*read_guard {
                *self.last_accessed.lock().await = Instant::now();
                return Ok(existing.clone());
            }
        }

        let mut write_guard = self.instance.write().await;
        if let Some(existing) = &*write_guard {
            *self.last_accessed.lock().await = Instant::now();
            return Ok(existing.clone());
        }

        let before_ram = get_process_rss_mb();
        info!("[RAM: {:.2} MB] -> Loading '{}' into memory...", before_ram, self.name);

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

                *write_guard = None;

                unsafe {
                    mi_collect(true);

                    #[cfg(target_os = "macos")]
                    malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
                }

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

/// Central coordinator for heavy runtime resources:
/// - ClusterCache (~1MB)
/// - AI Inference Engines (ClipEngine ~150MB, Full MediaEngine ~950MB)
#[derive(Clone)]
pub struct EngineCoordinator {
    pool: PgPool,
    models_dir: PathBuf,
    config: Config,

    // Vector cache for person clusters
    cluster_cache: Arc<ManagedResource<SharedClusterCache>>,

    // ONNX AI engines
    clip_engine: Arc<ManagedResource<ClipEngine>>,
    media_engine: Arc<ManagedResource<MediaEngine>>,
}

impl EngineCoordinator {
    pub fn new(pool: PgPool, models_dir: PathBuf, config: Config) -> Self {
        let ai_enabled = config.ai.enabled;

        let coordinator = Self {
            pool,
            models_dir,
            config,
            cluster_cache: Arc::new(ManagedResource::new("ClusterCache", 300)),
            clip_engine: Arc::new(ManagedResource::new("ClipEngine", 180)),
            media_engine: Arc::new(ManagedResource::new("MediaEngine", 180)),
        };

        if ai_enabled {
            let weak_cluster_cache = Arc::downgrade(&coordinator.cluster_cache);
            let weak_clip_engine = Arc::downgrade(&coordinator.clip_engine);
            let weak_media_engine = Arc::downgrade(&coordinator.media_engine);

            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;

                    let cl_cache = weak_cluster_cache.upgrade();
                    let c_engine = weak_clip_engine.upgrade();
                    let m_engine = weak_media_engine.upgrade();

                    if cl_cache.is_none() && c_engine.is_none() && m_engine.is_none() {
                        break;
                    }

                    if let Some(r) = cl_cache { r.try_evict().await; }
                    if let Some(r) = c_engine { r.try_evict().await; }
                    if let Some(r) = m_engine { r.try_evict().await; }
                }
            });
        }

        coordinator
    }

    /// Ensures the Face Cluster centroids are ready in RAM
    pub async fn ensure_cluster_cache(&self) -> Result<SharedClusterCache, String> {
        if !self.config.ai.enabled || !self.config.ai.enable_faces {
            return Err("Face cluster cache disabled in config".to_string());
        }

        let pool = self.pool.clone();
        let arc_shared = self
            .cluster_cache
            .get_or_load(|| async move {
                ClusterCacheManager::load_initial(&pool)
                    .await
                    .map_err(|e| e.to_string())
            })
            .await?;

        Ok((*arc_shared).clone())
    }

    /// Ensures the CLIP text encoder is loaded (~150 MB) for natural language search queries
    pub async fn ensure_search_engine(&self) -> Result<Arc<ClipEngine>, String> {
        if !self.config.ai.enabled || !self.config.ai.enable_clip {
            return Err("CLIP search engine disabled in config".to_string());
        }

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

    /// Ensures the full pipeline engine is loaded for processing ingested uploads
    pub async fn ensure_pipeline_engine(&self) -> Result<Arc<MediaEngine>, String> {
        if !self.config.ai.enabled {
            return Err("AI pipeline engine disabled in config".to_string());
        }

        let m_dir = self.models_dir.clone();
        self.media_engine
            .get_or_load(|| async move {
                tokio::task::spawn_blocking(move || -> Result<MediaEngine, String> {
                    let face = Arc::new(FaceEngine::init(&m_dir).map_err(|e| e.to_string())?);
                    let tag = Arc::new(TagEngine::init(&m_dir).map_err(|e| e.to_string())?);
                    let clip = Arc::new(ClipEngine::init(&m_dir).map_err(|e| e.to_string())?);
                    let yolo = Arc::new(YoloEngine::init(&m_dir).map_err(|e| e.to_string())?);
                    Ok(MediaEngine::new(face, tag, clip, yolo))
                })
                .await
                .map_err(|e| e.to_string())?
            })
            .await
    }

    pub async fn keep_warm(&self) {
        if !self.config.ai.enabled {
            return;
        }
        self.cluster_cache.touch().await;
        self.clip_engine.touch().await;
        self.media_engine.touch().await;
    }
}