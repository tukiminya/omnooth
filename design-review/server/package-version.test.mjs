import {test} from 'node:test';
import assert from 'node:assert/strict';
import {identifyPackageVersion} from '../src/domain/package-version.ts';
const base={contentHash:'ABCDEF12'+'a'.repeat(56),importedAt:'2026-10-27T14:00:00Z'};
test('すべてのファイル名で日付と SHA-256 を使い、明確な旧版名も採用しない',()=>{
  for(const filename of ['Eku_PC_v1_3_0.zip','Dress_v1_01.7z','Dress_1_01.rar','Dress_1.01.tar.gz','Kirisame_ver1.01.zip','1.01.unitypackage','Dress_latest.tar.xz']){
    const result=identifyPackageVersion({...base,filename});
    assert.equal(result.label,'2026-10-27@abcdef12',filename);
    assert.equal(result.versionDate,'2026-10-27');
    assert.equal(result.contentHash,base.contentHash.toLowerCase());
    assert.equal(result.originalFilename,filename);
  }
});
test('対象ファイルの更新日時を優先し、不明・無効なら初回取り込み日時の UTC 日付にする',()=>{
  const input={...base,filename:'Dress.zip'};
  assert.equal(identifyPackageVersion({...input,sourceUpdatedAt:'2026-08-18T18:45:08+09:00'}).versionDate,'2026-08-18');
  assert.equal(identifyPackageVersion({...input,sourceUpdatedAt:'2026-08-18T01:00:00+09:00'}).versionDate,'2026-08-17');
  assert.equal(identifyPackageVersion({...input,sourceUpdatedAt:'invalid'}).versionDate,'2026-10-27');
  assert.equal(identifyPackageVersion({...input,importedAt:'2026-10-27T01:00:00+09:00'}).versionDate,'2026-10-26');
});
test('表示用ハッシュの接頭辞が同じでも、内部識別に SHA-256 全長を保持する',()=>{
  const a=identifyPackageVersion({...base,filename:'Dress.zip'});
  const b=identifyPackageVersion({...base,filename:'Dress.zip',contentHash:'abcdef12'+'b'.repeat(56)});
  assert.equal(a.label,b.label);assert.notEqual(a.contentHash,b.contentHash);
});
test('不正なパス・ハッシュ・取得日時を版へ流し込まない',()=>{
  assert.throws(()=>identifyPackageVersion({...base,filename:'../Dress.zip'}),/INVALID_FILENAME/);
  assert.throws(()=>identifyPackageVersion({...base,filename:'Dress.zip',contentHash:'bad'}),/INVALID_HASH/);
  assert.throws(()=>identifyPackageVersion({...base,filename:'Dress.zip',importedAt:'bad'}),/INVALID_IMPORTED_AT/);
});
