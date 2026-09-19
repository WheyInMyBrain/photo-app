use sqlx::{Row, SqlitePool};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct BackupCandidate {
    pub asset_id: String,
    pub user_id: String,
    pub username: String,
    pub disk_rel_path: String,
    pub suggested_remote_path: String,
    pub sha256: String,
    pub is_deleted: bool,
}

pub struct BackupRepo;

impl BackupRepo {
    /// Fetch all eligible assets for users with `backup_enabled = 1`
    /// that are either:
    /// 1. Not yet backed up
    /// 2. Modified (synced_sha256 != sha256)
    /// 3. Soft-deleted (need to be removed from remote)
    pub async fn fetch_pending_sync_items(
        pool: &SqlitePool,
    ) -> Result<Vec<BackupCandidate>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT 
                a.id AS asset_id,
                a.user_id,
                u.username,
                a.rel_path AS disk_rel_path,
                a.folder_path,
                a.file_name,
                a.sha256,
                (a.deleted_at IS NOT NULL) AS is_deleted,
                b.synced_sha256
            FROM assets a
            JOIN users u ON a.user_id = u.id
            LEFT JOIN asset_backups b ON a.id = b.asset_id
            WHERE u.backup_enabled = 1
              AND (
                  b.asset_id IS NULL                     -- Never backed up
                  OR b.synced_sha256 != a.sha256          -- Content modified
                  OR (a.deleted_at IS NOT NULL AND b.asset_id IS NOT NULL) -- Soft deleted
              )
            ORDER BY a.user_id, a.folder_path, a.created_at ASC
            "#,
        )
        .fetch_all(pool)
        .await?;

        let mut seen_paths: HashSet<String> = HashSet::new();
        let mut candidates = Vec::with_capacity(rows.len());

        for r in rows {
            let asset_id: String = r.get("asset_id");
            let user_id: String = r.get("user_id");
            let username: String = r.get("username");
            let disk_rel_path: String = r.get("disk_rel_path");
            let folder_path: String = r.get("folder_path");
            let file_name: String = r.get("file_name");
            let sha256: String = r.get("sha256");
            let is_deleted: bool = r.get("is_deleted");

            // Compute human-readable, collision-free remote path:
            // "users/{username}/{folder}/{filename}" (or filename_1.ext if collision)
            let base_folder = folder_path.trim_matches('/');
            let clean_folder = if base_folder.is_empty() || base_folder == "root" {
                String::new()
            } else {
                format!("{base_folder}/")
            };

            let remote_path = Self::deduplicate_path(
                &format!("users/{}/{}{}", username, clean_folder, file_name),
                &mut seen_paths,
            );

            candidates.push(BackupCandidate {
                asset_id,
                user_id,
                username,
                disk_rel_path,
                suggested_remote_path: remote_path,
                sha256,
                is_deleted,
            });
        }

        Ok(candidates)
    }

    fn deduplicate_path(path: &str, seen: &mut HashSet<String>) -> String {
        if !seen.contains(path) {
            seen.insert(path.to_string());
            return path.to_string();
        }

        let (stem, ext) = match path.rsplit_once('.') {
            Some((s, e)) => (s, format!(".{}", e)),
            None => (path, String::new()),
        };

        let mut counter = 1;
        loop {
            let candidate = format!("{}_{}{}", stem, counter, ext);
            if !seen.contains(&candidate) {
                seen.insert(candidate.clone());
                return candidate;
            }
            counter += 1;
        }
    }

    pub async fn mark_synced(
        pool: &SqlitePool,
        asset_id: &str,
        user_id: &str,
        remote_path: &str,
        sha256: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO asset_backups (asset_id, user_id, remote_path, synced_sha256, backed_up_at)
            VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
            ON CONFLICT(asset_id) DO UPDATE SET
                remote_path = excluded.remote_path,
                synced_sha256 = excluded.synced_sha256,
                backed_up_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .bind(remote_path)
        .bind(sha256)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn remove_backup_record(
        pool: &SqlitePool,
        asset_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM asset_backups WHERE asset_id = ?1")
            .bind(asset_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}