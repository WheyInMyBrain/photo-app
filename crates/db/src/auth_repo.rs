use sqlx::{FromRow, PgPool, Row};
use uuid::Uuid;

#[derive(Clone, Debug, FromRow)]
pub struct UserRecord {
    pub id: Uuid,
    pub username: String,
}

#[derive(Clone, Debug, FromRow)]
pub struct UserCredentialsRecord {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
    pub display_name: Option<String>,
    pub api_key: Option<String>,
}

pub struct AuthRepo;

impl AuthRepo {
    pub async fn find_by_api_key(
        pool: &PgPool,
        api_key: &str,
    ) -> Result<Option<UserRecord>, sqlx::Error> {
        sqlx::query_as::<_, UserRecord>(
            r#"
            SELECT id, username 
            FROM users 
            WHERE api_key = $1 
            LIMIT 1
            "#,
        )
        .bind(api_key)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_id(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Option<UserRecord>, sqlx::Error> {
        sqlx::query_as::<_, UserRecord>(
            r#"
            SELECT id, username 
            FROM users 
            WHERE id = $1 
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn username_exists(
        pool: &PgPool,
        username: &str,
    ) -> Result<bool, sqlx::Error> {
        let exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1 
                FROM users 
                WHERE LOWER(username) = LOWER($1)
            )
            "#,
        )
        .bind(username)
        .fetch_one(pool)
        .await?;

        Ok(exists)
    }

    /// Single roundtrip query resolving display_name and passkey existence simultaneously
    pub async fn get_user_status(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<(Option<String>, bool), sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT 
                u.display_name,
                EXISTS (
                    SELECT 1 
                    FROM passkey_credentials pc 
                    WHERE pc.user_id = u.id
                ) AS has_passkey
            FROM users u
            WHERE u.id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => {
                let display_name: Option<String> = r.get("display_name");
                let has_passkey: bool = r.get("has_passkey");
                Ok((display_name, has_passkey))
            }
            None => Ok((None, false)),
        }
    }

    pub async fn create_user(
        pool: &PgPool,
        id: Uuid,
        username: &str,
        password_hash: &str,
        display_name: Option<&str>,
        api_key: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO users (id, username, password_hash, display_name, api_key)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(id)
        .bind(username)
        .bind(password_hash)
        .bind(display_name)
        .bind(api_key)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn find_for_login(
        pool: &PgPool,
        username: &str,
    ) -> Result<Option<UserCredentialsRecord>, sqlx::Error> {
        sqlx::query_as::<_, UserCredentialsRecord>(
            r#"
            SELECT id, username, password_hash, display_name, api_key
            FROM users 
            WHERE LOWER(username) = LOWER($1) 
            LIMIT 1
            "#,
        )
        .bind(username)
        .fetch_optional(pool)
        .await
    }

    pub async fn get_api_key(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(
            r#"
            SELECT api_key 
            FROM users 
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn set_api_key(
        pool: &PgPool,
        user_id: Uuid,
        api_key: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE users 
            SET api_key = $1, updated_at = CURRENT_TIMESTAMP 
            WHERE id = $2
            "#,
        )
        .bind(api_key)
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn upsert_passkey_credential(
        pool: &PgPool,
        credential_id: &str,
        user_id: Uuid,
        name: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO passkey_credentials (id, user_id, public_key, name)
            VALUES ($1, $2, '\x00'::bytea, $3)
            ON CONFLICT (id) DO UPDATE SET 
                name = EXCLUDED.name
            "#,
        )
        .bind(credential_id)
        .bind(user_id)
        .bind(name)
        .execute(pool)
        .await?;

        Ok(())
    }
}