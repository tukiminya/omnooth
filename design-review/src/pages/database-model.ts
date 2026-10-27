import type { Spec } from '../types';

export type DatabaseModel = {
  tables: {
    name: string;
    specId: string;
    label: string;
    summary: string;
    fields: {name: string; type: string; constraint: string; description: string; primary: boolean; foreign: boolean}[];
  }[];
  relations: {
    id: string;
    parent: string;
    parentField: string;
    child: string;
    childField: string;
    optional: boolean;
    label: string;
    description: string;
  }[];
};

// 同じ設計本文から列と FK を読む。ER 図のためにレビュー対象の本文を複製しない。
export function createDatabaseModel(specs: Pick<Spec,'id'|'category'|'title'|'subtitle'|'summary'|'fields'>[]): DatabaseModel {
  const tables: DatabaseModel['tables'] = [];
  for (const spec of specs.filter(s=>s.category==='database')) {
    for (const name of spec.title.split(' / ')) {
      const split = spec.title.includes(' / ');
      const fields = (spec.fields??[]).filter(field=>!split||field.name.startsWith(name+'.')).map(field=>({
        ...field,
        name: split ? field.name.slice(name.length+1) : field.name,
        primary: /\bPK\b/.test(field.constraint),
        foreign: /\bFK\b/.test(field.constraint),
      }));
      tables.push({
        name, specId:spec.id, summary:spec.summary,
        label:name==='tags'?'タグ・カテゴリー':name==='item_tags'?'商品とタグの中間テーブル':spec.subtitle,
        fields,
      });
    }
  }
  const relations: DatabaseModel['relations'] = [];
  for (const child of tables) {
    for (const field of child.fields) {
      const reference = /FK → ([\w]+)\.([\w]+)/.exec(field.constraint);
      if (!reference) continue;
      const [,parent,parentField] = reference;
      const optional = /\bNULL\b/.test(field.constraint)&&!/NOT NULL/.test(field.constraint);
      const label = field.name==='owner_version_id'?'依存の定義元':field.name==='required_version_id'?'必要なバージョン':
        field.name==='display_parent_id'?'表示上の親':field.name==='from_version_id'?'変更前':field.name==='to_version_id'?'変更後':field.description;
      relations.push({
        id:child.name+'.'+field.name,parent,parentField,child:child.name,childField:field.name,optional,label,
        description: child.name+' の '+field.name+' は '+parent+'.'+parentField+' を参照します。'+
          (optional?'参照先は0件または1件（NULL 可）。':'参照先は必ず1件。')+
          '参照先の1件には、このテーブルの0件以上のレコードを紐づけられます。',
      });
    }
  }
  return {tables,relations};
}

export const databaseViews = [
  {id:'library',title:'商品とファイル',description:'ショップ → 商品 → パッケージ → バージョン → 収録ファイル。カスタムも同じパッケージとして保存します。',
    rows:[['shops','items','packages'],['','package_versions','package_files']]},
  {id:'dependencies',title:'カスタムの依存',description:'依存の定義元と、必要とするバージョンは別々の外部キーです。表示上の親も依存関係と分けて管理します。',
    rows:[['packages','package_versions','package_dependencies']]},
  {id:'import',title:'ファイルの取り込み',description:'Deep Link のキューと、登録したバージョンの対応。商品情報を取得する前でもキューへ追加できます。',
    rows:[['items','library_import_jobs','library_import_results'],['packages','package_versions','']]},
  {id:'installation',title:'プロジェクトと適用',description:'プロジェクトの適用済みパッケージと、ファイルの所有台帳。実績はパッケージの特定バージョンを参照します。',
    rows:[['projects','project_packages','package_versions'],['project_files','install_jobs','packages'],['','project_selections','package_files']]},
  {id:'selections',title:'選択した収録ファイル',description:'配布原本の版から選んだ unitypackage と、Unity 内で配置した各ファイルの対応。',
    rows:[['projects','project_selections','package_files'],['project_files','packages','package_versions']]},
  {id:'history',title:'実行・復旧の履歴',description:'プロジェクトごとに適用ジョブを作り、その下に変更操作を記録。変更前と変更後は、それぞれ任意のバージョン参照です。',
    rows:[['projects','install_jobs','install_operations'],['','packages','package_versions']]},
  {id:'tags',title:'タグと設定',description:'商品とタグの多対多を item_tags で表します。app_settings は他のテーブルを参照しない、アプリ全体の設定です。',
    rows:[['items','item_tags','tags'],['','app_settings','']]},
  {id:'all',title:'全テーブル',description:'テーブルを選ぶと、直接つながる関係を強調します。',
    rows:[['shops','items','packages','package_versions'],['tags','item_tags','package_dependencies','package_files'],
      ['projects','project_packages','install_operations','project_files'],['app_settings','install_jobs','library_import_jobs','library_import_results'],
      ['','project_selections','','']]},
];
