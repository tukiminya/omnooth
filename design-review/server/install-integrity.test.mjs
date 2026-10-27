// 設計レビュー用の最小スキーマ。製品用マイグレーションではない。
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {DatabaseSync} from 'node:sqlite';

function fixture(t){
  const db=new DatabaseSync(':memory:');t.after(()=>db.close());
  db.exec(`
    PRAGMA foreign_keys=ON;
    CREATE TABLE projects(id TEXT PRIMARY KEY);
    CREATE TABLE packages(id TEXT PRIMARY KEY);
    CREATE TABLE package_versions(id TEXT PRIMARY KEY,package_id TEXT NOT NULL REFERENCES packages(id),UNIQUE(package_id,id));
    CREATE TABLE package_files(id TEXT PRIMARY KEY,version_id TEXT NOT NULL REFERENCES package_versions(id),UNIQUE(version_id,id));
    CREATE TABLE project_packages(project_id TEXT NOT NULL REFERENCES projects(id),package_id TEXT NOT NULL REFERENCES packages(id),version_id TEXT NOT NULL,
      PRIMARY KEY(project_id,package_id),UNIQUE(project_id,package_id,version_id),
      FOREIGN KEY(package_id,version_id) REFERENCES package_versions(package_id,id));
    CREATE TABLE project_selections(project_id TEXT NOT NULL,file_id TEXT NOT NULL,package_id TEXT NOT NULL,version_id TEXT NOT NULL,apply_order INTEGER NOT NULL CHECK(apply_order>=0),
      PRIMARY KEY(project_id,file_id),UNIQUE(project_id,package_id,file_id),UNIQUE(project_id,version_id,apply_order),
      FOREIGN KEY(project_id,package_id,version_id) REFERENCES project_packages(project_id,package_id,version_id) DEFERRABLE INITIALLY DEFERRED,
      FOREIGN KEY(version_id,file_id) REFERENCES package_files(version_id,id) DEFERRABLE INITIALLY DEFERRED);
    CREATE TABLE project_files(project_id TEXT NOT NULL,relative_path TEXT NOT NULL,package_id TEXT NOT NULL,source_file_id TEXT NOT NULL,
      PRIMARY KEY(project_id,relative_path),
      FOREIGN KEY(project_id,package_id,source_file_id) REFERENCES project_selections(project_id,package_id,file_id) DEFERRABLE INITIALLY DEFERRED);
    CREATE TABLE install_operations(package_id TEXT NOT NULL,operation TEXT NOT NULL,from_version_id TEXT,to_version_id TEXT,
      FOREIGN KEY(package_id,from_version_id) REFERENCES package_versions(package_id,id),
      FOREIGN KEY(package_id,to_version_id) REFERENCES package_versions(package_id,id),
      CHECK((operation='add' AND from_version_id IS NULL AND to_version_id IS NOT NULL)
        OR (operation='remove' AND from_version_id IS NOT NULL AND to_version_id IS NULL)
        OR (operation='update' AND from_version_id IS NOT NULL AND to_version_id IS NOT NULL AND from_version_id!=to_version_id)
        OR (operation='reselect' AND from_version_id IS NOT NULL AND from_version_id=to_version_id)));
    INSERT INTO projects VALUES('p');INSERT INTO packages VALUES('a'),('b');
    INSERT INTO package_versions VALUES('a1','a'),('a2','a'),('b1','b');
    INSERT INTO package_files VALUES('f1','a1'),('f2','a2'),('fb','b1');
    INSERT INTO project_packages VALUES('p','a','a1');
    INSERT INTO project_selections VALUES('p','f1','a','a1',0);
    INSERT INTO project_files VALUES('p','Assets/A.prefab','a','f1');
  `);return db;
}
test('選択・所有権・変更ジャーナルへ別商品の版や未選択ファイルを混ぜられない',t=>{
  const db=fixture(t);
  assert.throws(()=>db.exec("INSERT INTO project_packages VALUES('p','b','a1')"),/FOREIGN KEY/);
  assert.throws(()=>db.exec("INSERT INTO project_selections VALUES('p','f2','a','a1',1)"),/FOREIGN KEY/);
  assert.throws(()=>db.exec("INSERT INTO project_files VALUES('p','Assets/B.prefab','a','fb')"),/FOREIGN KEY/);
  assert.throws(()=>db.exec("INSERT INTO install_operations VALUES('a','update','a1','b1')"),/FOREIGN KEY/);
  assert.throws(()=>db.exec("INSERT INTO install_operations VALUES('a','remove',NULL,NULL)"),/CHECK/);
  db.exec("INSERT INTO install_operations VALUES('a','reselect','a1','a1')");
  assert.deepEqual(db.prepare('PRAGMA foreign_key_check').all(),[]);
});
test('版・選択・所有台帳を一括更新でき、整合性が崩れたままの確定は拒否する',t=>{
  const db=fixture(t);
  db.exec(`BEGIN;UPDATE project_packages SET version_id='a2';DELETE FROM project_selections;
    INSERT INTO project_selections VALUES('p','f2','a','a2',0);
    UPDATE project_files SET source_file_id='f2';COMMIT;`);
  assert.equal(db.prepare('SELECT source_file_id FROM project_files').get().source_file_id,'f2');
  db.exec('BEGIN;DELETE FROM project_selections;');
  assert.throws(()=>db.exec('COMMIT'),/FOREIGN KEY/);db.exec('ROLLBACK');
  assert.equal(db.prepare('SELECT count(*) AS n FROM project_selections').get().n,1);
  assert.deepEqual(db.prepare('PRAGMA foreign_key_check').all(),[]);
});
