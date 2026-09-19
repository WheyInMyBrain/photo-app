pub mod album_repo;
pub mod asset_repo;
pub mod auth_repo;
pub mod cache_repo;
pub mod domain;
pub mod init;
pub mod job_repo;
pub mod person_repo;
pub mod scrapes_repo;
pub mod tag_repo;
pub mod ingestion_repo;
pub mod backup_repo;

pub use album_repo::AlbumRepo;
pub use asset_repo::AssetRepo;
pub use auth_repo::{AuthRepo, UserCredentialsRecord, UserRecord};
pub use cache_repo::{CacheRepo, RawClipRecord, RawClusterRecord};
pub use init::init_db_pool;
pub use job_repo::JobRepo;
pub use person_repo::PersonRepo;
pub use scrapes_repo::{ScrapedMediaItemRecord, ScrapedPostRecord, ScrapesRepo};
pub use tag_repo::TagRepo;
pub use ingestion_repo::{
    IngestionDetectedFace, IngestionNewPerson, IngestionPayload, IngestionRepo,
    IngestionUpdatedCluster, AiEnrichmentPayload,
};

pub use domain::*;