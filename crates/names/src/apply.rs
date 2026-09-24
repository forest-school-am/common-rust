//! Applying deltas to plain name columns. JSON-embedded names (an ACL inside a
//! blob, say) are the consumer's own shape and belong in a consumer-supplied
//! apply closure, not here.

use sqlx::SqlitePool;

use crate::{NameDeltas, NameKind, NameTarget, Rename};

/// Rewrite each target column old -> current, users against `deltas.users` and
/// groups against `deltas.groups`. Returns the total number of rows changed.
///
/// Table and column names come from the compile-time [`NameTarget`] table (the
/// `#[derive(NameColumns)]` output), never from input, so they are formatted
/// into the statement directly; the values are always bound.
pub async fn apply_column_renames(
    pool: &SqlitePool,
    targets: &[NameTarget],
    deltas: &NameDeltas,
) -> anyhow::Result<u64> {
    let mut changed = 0u64;
    for target in targets {
        let renames: &[Rename] = match target.kind {
            NameKind::User => &deltas.users,
            NameKind::Group => &deltas.groups,
        };
        if renames.is_empty() {
            continue;
        }
        let sql = format!(
            "UPDATE {table} SET {column} = ? WHERE {column} = ?",
            table = target.table,
            column = target.column,
        );
        for rename in renames {
            let result = sqlx::query(&sql)
                .bind(&rename.current)
                .bind(&rename.old)
                .execute(pool)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "renaming {}.{} {} -> {}: {e}",
                        target.table,
                        target.column,
                        rename.old,
                        rename.current
                    )
                })?;
            changed += result.rows_affected();
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NameColumns, Rename};
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Row;

    // A row struct exercising both kinds and an ignored column.
    #[derive(NameColumns)]
    #[names(table = "forms")]
    #[allow(dead_code)]
    struct FormRow {
        #[name(user)]
        owner: String,
        source_json: String,
    }

    #[derive(NameColumns)]
    #[names(table = "acl")]
    #[allow(dead_code)]
    struct AclRow {
        #[name(user)]
        member: String,
        #[name(group)]
        team: String,
    }

    #[test]
    fn derive_emits_only_marked_columns_with_the_table_and_kind() {
        assert_eq!(
            FormRow::name_targets(),
            &[NameTarget {
                table: "forms",
                column: "owner",
                kind: NameKind::User,
            }]
        );
        assert_eq!(
            AclRow::name_targets(),
            &[
                NameTarget {
                    table: "acl",
                    column: "member",
                    kind: NameKind::User,
                },
                NameTarget {
                    table: "acl",
                    column: "team",
                    kind: NameKind::Group,
                },
            ]
        );
    }

    async fn mem_pool() -> SqlitePool {
        // One connection: a multi-connection pool over ":memory:" gives each
        // connection its own private database.
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    fn deltas(users: &[(&str, &str)], groups: &[(&str, &str)]) -> NameDeltas {
        let map = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(o, c)| Rename {
                    old: (*o).into(),
                    current: (*c).into(),
                })
                .collect()
        };
        NameDeltas {
            now: "cursor".into(),
            users: map(users),
            groups: map(groups),
        }
    }

    #[tokio::test]
    async fn user_and_group_renames_touch_only_their_own_kind() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE acl (member TEXT, team TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        for (m, t) in [("bob", "dev"), ("carol", "dev"), ("bob", "ops")] {
            sqlx::query("INSERT INTO acl (member, team) VALUES (?, ?)")
                .bind(m)
                .bind(t)
                .execute(&pool)
                .await
                .unwrap();
        }

        let d = deltas(&[("bob", "bob2")], &[("dev", "devs")]);
        // bob appears twice (member), dev appears twice (team) = 4 rows changed.
        let changed = apply_column_renames(&pool, AclRow::name_targets(), &d)
            .await
            .unwrap();
        assert_eq!(changed, 4);

        let rows = sqlx::query("SELECT member, team FROM acl ORDER BY member, team")
            .fetch_all(&pool)
            .await
            .unwrap();
        let got: Vec<(String, String)> = rows
            .iter()
            .map(|r| (r.get::<String, _>("member"), r.get::<String, _>("team")))
            .collect();
        assert_eq!(
            got,
            vec![
                ("bob2".to_string(), "devs".to_string()),
                ("bob2".to_string(), "ops".to_string()),
                ("carol".to_string(), "devs".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn no_matching_value_changes_nothing() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE forms (owner TEXT, source_json TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO forms (owner, source_json) VALUES ('alice', '{}')")
            .execute(&pool)
            .await
            .unwrap();

        let d = deltas(&[("bob", "bob2")], &[]);
        let changed = apply_column_renames(&pool, FormRow::name_targets(), &d)
            .await
            .unwrap();
        assert_eq!(changed, 0);
        let owner: String = sqlx::query("SELECT owner FROM forms")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("owner");
        assert_eq!(owner, "alice");
    }

    #[tokio::test]
    async fn empty_deltas_run_no_statements() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE forms (owner TEXT, source_json TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        let d = deltas(&[], &[]);
        let changed = apply_column_renames(&pool, FormRow::name_targets(), &d)
            .await
            .unwrap();
        assert_eq!(changed, 0);
    }
}
