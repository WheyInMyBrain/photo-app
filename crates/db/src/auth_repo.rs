use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool};

#[derive(Clone, Debug, FromRow)]
pub struct UserRecord {
    pub id: String,
    pub username: String,
}

#[derive(Clone, Debug, FromRow)]
pub struct UserCredentialsRecord {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub display_name: Option<String>,
    pub api_key: Option<String>,
}

pub struct AuthRepo;

impl AuthRepo {
    pub async fn find_by_api_key(
        pool: &SqlitePool,
        api_key: &str,
    ) -> Result<Option<UserRecord>, sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, username FROM users WHERE api_key = ",
        );
        qb.push_bind(api_key);
        qb.push(" LIMIT 1");

        qb.build_query_as::<UserRecord>().fetch_optional(pool).await
    }

    pub async fn find_by_id(
        pool: &SqlitePool,
        user_id: &str,
    ) -> Result<Option<UserRecord>, sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, username FROM users WHERE id = ",
        );
        qb.push_bind(user_id);
        qb.push(" LIMIT 1");

        qb.build_query_as::<UserRecord>().fetch_optional(pool).await
    }

    pub async fn username_exists(
        pool: &SqlitePool,
        username: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT COUNT(*) > 0 FROM users WHERE username = ",
        );
        qb.push_bind(username);
        qb.push(" COLLATE NOCASE");

        let exists: bool = qb.build_query_scalar().fetch_one(pool).await?;
        Ok(exists)
    }

    pub async fn get_user_status(
        pool: &SqlitePool,
        user_id: &str,
    ) -> Result<(Option<String>, bool), sqlx::Error> {
        let mut display_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT display_name FROM users WHERE id = ",
        );
        display_qb.push_bind(user_id);
        let display_name: Option<String> = display_qb
            .build_query_scalar()
            .fetch_optional(pool)
            .await?;

        let mut passkey_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT COUNT(*) > 0 FROM passkey_credentials WHERE user_id = ",
        );
        passkey_qb.push_bind(user_id);
        let has_passkey: bool = passkey_qb
            .build_query_scalar()
            .fetch_one(pool)
            .await
            .unwrap_or(false);

        Ok((display_name, has_passkey))
    }

    pub async fn create_user(
        pool: &SqlitePool,
        id: &str,
        username: &str,
        password_hash: &str,
        display_name: Option<&str>,
        api_key: &str,
    ) -> Result<(), sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "INSERT INTO users (id, username, password_hash, display_name, api_key) ",
        );
        qb.push_values(
            std::iter::once((id, username, password_hash, display_name, api_key)),
            |mut b, (i, u, p, d, a)| {
                b.push_bind(i)
                    .push_bind(u)
                    .push_bind(p)
                    .push_bind(d)
                    .push_bind(a);
            },
        );

        qb.build().execute(pool).await?;
        Ok(())
    }

    pub async fn find_for_login(
        pool: &SqlitePool,
        username: &str,
    ) -> Result<Option<UserCredentialsRecord>, sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, username, password_hash, display_name, api_key \
             FROM users WHERE username = ",
        );
        qb.push_bind(username);
        qb.push(" COLLATE NOCASE LIMIT 1");

        qb.build_query_as::<UserCredentialsRecord>()
            .fetch_optional(pool)
            .await
    }

    pub async fn get_api_key(
        pool: &SqlitePool,
        user_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT api_key FROM users WHERE id = ",
        );
        qb.push_bind(user_id);

        qb.build_query_scalar().fetch_optional(pool).await
    }

    pub async fn set_api_key(
        pool: &SqlitePool,
        user_id: &str,
        api_key: &str,
    ) -> Result<(), sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "UPDATE users SET api_key = ",
        );
        qb.push_bind(api_key);
        qb.push(" WHERE id = ");
        qb.push_bind(user_id);

        qb.build().execute(pool).await?;
        Ok(())
    }

    pub async fn upsert_passkey_credential(
        pool: &SqlitePool,
        credential_id: &str,
        user_id: &str,
        name: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "INSERT INTO passkey_credentials (id, user_id, public_key, name) ",
        );
        qb.push_values(
            std::iter::once((credential_id, user_id, name)),
            |mut b, (cid, uid, n)| {
                b.push_bind(cid)
                    .push_bind(uid)
                    .push("X'00'")
                    .push_bind(n);
            },
        );
        qb.push(" ON CONFLICT(id) DO UPDATE SET name = excluded.name");

        qb.build().execute(pool).await?;
        Ok(())
    }
}