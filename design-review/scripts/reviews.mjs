import { DatabaseSync } from 'node:sqlite';
import { existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { catalog } from '../data/catalog.mjs';
const path=resolve(dirname(fileURLToPath(import.meta.url)),'../data/reviews.sqlite');
if (!existsSync(path)) {
  console.log('レビューはまだありません。pnpm dev でアプリを起動してください。');
} else {
  const db=new DatabaseSync(path,{readOnly:true});
  const rows=db.prepare('SELECT * FROM comments ORDER BY createdAt,rowid').all();
  for (const row of rows) {
    const spec=catalog.specs.find(s=>s.id===row.targetId);
    console.log('\n['+row.status+'] '+row.targetId+' / '+(spec?.title??'')+' / '+row.kind);
    console.log(row.author+' · '+row.createdAt+(row.parentId?' · 返信先 '+row.parentId:''));
    console.log(row.body);
  }
  if (!rows.length) console.log('コメントはまだありません。');
  console.log('\nレビュー状態:');
  console.log(JSON.stringify(db.prepare('SELECT * FROM decisions ORDER BY updatedAt').all(),null,2));
  db.close();
}
