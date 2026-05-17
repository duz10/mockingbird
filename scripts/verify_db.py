"""Quick DB-state checker for post-migration verification."""
import sqlite3, sys
conn = sqlite3.connect(r'C:\Users\dboyd\AppData\Roaming\com.dustin.mockingbird\mockingbird.db')
cur = conn.cursor()
print('schema_version:', cur.execute("SELECT value FROM schema_meta WHERE key='schema_version'").fetchone()[0], flush=True)
print('prompts table:', flush=True)
for r in cur.execute('SELECT id, mode_slug, version, length(body) FROM prompts ORDER BY id').fetchall():
    print(f'  id={r[0]}  mode={r[1]:10s}  v={r[2]}  body_len={r[3]}', flush=True)
print('modes.normal.prompt_id:', cur.execute("SELECT prompt_id FROM modes WHERE slug='normal'").fetchone()[0], flush=True)
sys.stdout.flush()
