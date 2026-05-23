"""ADR 0046 Iter 1 / mb-jbf7 schema verification probe.

Opens the production mockingbird.db read-only and prints:
- `sessions.source` column metadata (migration 018)
- `idx_sessions_source` index DDL
- Recent sessions (so we can see if anything new has landed)
- Distinct source values across the whole table

Run with: `python verify_iter1_schema.py`
"""
import os
import sqlite3
import sys


def main() -> int:
    appdata = os.environ.get("APPDATA")
    if not appdata:
        print("APPDATA not set", file=sys.stderr)
        return 2
    db = os.path.join(appdata, "com.dustin.mockingbird", "mockingbird.db")
    print(f"DB: {db}")
    print(f"Exists: {os.path.exists(db)}")
    if not os.path.exists(db):
        return 2

    uri = f"file:{db}?mode=ro"
    conn = sqlite3.connect(uri, uri=True)

    def section(title: str, sql: str, params: tuple = ()) -> None:
        print(f"\n--- {title} ---")
        try:
            cur = conn.execute(sql, params)
        except sqlite3.OperationalError as e:
            print(f"  (skipped: {e})")
            return
        cols = [d[0] for d in cur.description] if cur.description else []
        if cols:
            print("  " + " | ".join(cols))
        rows = cur.fetchall()
        if not rows:
            print("  (no rows)")
            return
        for r in rows:
            print("  " + " | ".join(str(c) for c in r))

    section(
        "pragma_table_info(sessions) WHERE name = 'source'",
        "SELECT * FROM pragma_table_info('sessions') WHERE name = 'source'",
    )
    section(
        "sqlite_master WHERE name = 'idx_sessions_source'",
        "SELECT name, sql FROM sqlite_master WHERE name = 'idx_sessions_source'",
    )
    section(
        "pragma_table_info(sessions) WHERE name = 'start_mode'",
        "SELECT * FROM pragma_table_info('sessions') WHERE name = 'start_mode'",
    )
    section(
        "schema_meta.schema_version",
        "SELECT key, value FROM schema_meta WHERE key = 'schema_version'",
    )
    section(
        "Recent sessions (8)",
        """SELECT id, source, start_mode, status,
                  substr(started_at, 1, 19) AS started,
                  audio_duration_ms AS dur_ms
           FROM sessions ORDER BY id DESC LIMIT 8""",
    )
    section(
        "Distinct sources across all sessions",
        "SELECT source, COUNT(*) FROM sessions GROUP BY source",
    )
    section(
        "Distinct start_modes across all sessions",
        "SELECT start_mode, COUNT(*) FROM sessions GROUP BY start_mode",
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
