import type { Catalog, Spec } from '../types';

type Definition = {
  nodes: {id:string;specId?:string;label?:string;caption?:string;kind?:'condition'|'event'|'result';column:number;row:number}[];
  edges: {from:string;to:string;label?:string;fromSide?:Side;toSide?:Side;via?:number[][];labelAt?:number[];feedback?:boolean}[];
};
type Side = 'top'|'right'|'bottom'|'left';
const WIDTH=244, HEIGHT=74, STEP_X=336, STEP_Y=132, PADDING=30;
const func=(id:string,column:number,row:number,caption?:string)=>({id,specId:id,column,row,caption});
const state=(id:string,label:string,column:number,row:number,kind:'condition'|'event'|'result'='result')=>({id,label,column,row,kind});

export const architectureFlow:Catalog['flows'][number]={
  id:'architecture',title:'アプリ全体',trigger:'構成図',description:'端末内の共通処理と外部 API の境界。',result:'共通の application service から各アダプターへ接続する。',
  steps:['infra-desktop','infra-extensions','infra-application','infra-storage','infra-unity','infra-booth'],
};

// 処理順・条件・再開先を明示し、関数名と仕様本文はカタログから取得する。
const diagrams:Record<string,Definition> = {
  'content-selection': {
    nodes:[
      state('received','取り込んだ配布物を開く',0,0,'event'),state('ready','展開済み？',1,0,'condition'),state('stored','原本のみ保存',2,0),
      func('fn-extract',2,1,'明示的に展開する'),func('fn-library-contents',1,1),
      state('available','unitypackage が含まれる？',1,2,'condition'),state('unavailable','原本を保持・Unity への適用不可',2,2),
      state('choose','unitypackage を選択・並べ替え',1,3,'event'),func('fn-content-selection',1,4),
      state('valid','入力に不足・不正がある？',1,5,'condition'),state('invalid','選択・プロジェクトを修正',0,5),
      func('fn-plan',1,6),state('done','競合・差分を確認して適用',2,6),
    ],
    edges:[
      {from:'received',to:'ready'},{from:'ready',to:'stored',label:'いいえ'},
      {from:'stored',to:'fn-extract',label:'中身を展開する'},
      {from:'fn-extract',to:'fn-library-contents'},{from:'ready',to:'fn-library-contents',label:'はい'},
      {from:'fn-library-contents',to:'available'},
      {from:'available',to:'unavailable',label:'含まれない'},{from:'available',to:'choose',label:'含まれる'},
      {from:'choose',to:'fn-content-selection'},
      {from:'fn-content-selection',to:'valid'},{from:'valid',to:'invalid',label:'ある'},
      {from:'valid',to:'fn-plan',label:'なし'},{from:'fn-plan',to:'done'},
    ],
  },
  architecture: {
    nodes:[
      {id:'gui',specId:'infra-desktop',label:'React GUI',caption:'端末内・Tauri command',column:0,row:0},
      {id:'stdio',specId:'infra-extensions',label:'ブラウザ / Unity 拡張',caption:'端末内・stdin / stdout',column:2,row:0},
      {id:'shared',specId:'infra-application',label:'共通 application service',caption:'端末内・Rust のユースケース',column:1,row:1},
      {id:'storage',specId:'infra-storage',label:'SQLite + Files',caption:'端末内・メタデータと原本',column:0,row:2},
      {id:'unity',specId:'infra-unity',label:'Unity Editor',caption:'端末内・Editor アダプター',column:1,row:2},
      {id:'booth',specId:'infra-booth',label:'BOOTH 公開 API',caption:'BOOTH がホスト・商品 JSON',column:2,row:2},
    ],
    edges:[
      {from:'gui',to:'shared',label:'同じ DTO'},
      {from:'stdio',to:'shared',label:'同じ DTO'},
      {from:'shared',to:'storage'},{from:'shared',to:'unity'},
      {from:'shared',to:'booth',label:'HTTPS'},
    ],
  },
  'deep-link-import': {
    nodes:[
      state('received','Deep Link を受信',0,0,'event'),func('fn-deep-link',1,0),func('fn-parse-import-link',2,0),
      func('fn-enqueue-import',2,1),func('fn-run-import',1,1),func('fn-download',0,1),
      state('download-result','ダウンロード結果',0,2,'condition'),state('awaiting-url','URL の再受信待ち',1,2),
      func('fn-import',0,3,'原本の保存と展開'),state('extract','圧縮ファイルを展開？',1,3,'condition'),func('fn-inspect',2,3),
      state('password','パスワードが必要？',2,4,'condition'),
      func('fn-update-import',2,5,'パスワードを入力して再開'),state('done','ライブラリ登録完了',1,5),
    ],
    edges:[
      {from:'received',to:'fn-deep-link'},{from:'fn-deep-link',to:'fn-parse-import-link'},
      {from:'fn-parse-import-link',to:'fn-enqueue-import'},{from:'fn-enqueue-import',to:'fn-run-import'},
      {from:'fn-run-import',to:'fn-download',label:'受付順'},
      {from:'fn-download',to:'download-result'},
      {from:'download-result',to:'awaiting-url',label:'期限切れ・拒否'},
      {from:'awaiting-url',to:'fn-enqueue-import',label:'新しい URL',fromSide:'right',toSide:'bottom',via:[[824,331]],feedback:true},
      {from:'download-result',to:'fn-import',label:'取得完了'},
      {from:'fn-import',to:'extract'},
      {from:'extract',to:'fn-inspect',label:'ON'},
      {from:'extract',to:'done',label:'OFF / 対象外',labelAt:[414,579]},
      {from:'fn-inspect',to:'password'},
      {from:'password',to:'done',label:'不要',fromSide:'left',toSide:'top',via:[[488,595]],labelAt:[610,584]},
      {from:'password',to:'fn-update-import',label:'必要'},
      {from:'fn-update-import',to:'fn-inspect',label:'再開',fromSide:'right',toSide:'right',via:[[1004,727],[1004,463]],feedback:true},
    ],
  },
  import: {
    nodes:[
      state('received','一括インポート',0,0,'event'),func('fn-import',1,0,'原本の保存と展開'),state('metadata','商品情報が必要？',2,0,'condition'),
      func('fn-api',2,1),func('fn-infer',1,1,'更新日時・SHA-256'),
      state('extract','圧縮ファイルを展開？',0,2,'condition'),func('fn-inspect',1,2),
      state('archive-result','解析・展開できた？',2,2,'condition'),
      state('input','パスワード入力待ち',2,3),state('done','原本・バージョンを登録',0,4),
    ],
    edges:[
      {from:'received',to:'fn-import'},{from:'fn-import',to:'metadata'},
      {from:'metadata',to:'fn-api',label:'必要'},
      {from:'fn-api',to:'fn-infer',label:'失敗時も取得日で継続'},
      {from:'metadata',to:'fn-infer',label:'不要 / 取得済み',fromSide:'right',toSide:'top',via:[[1004,67],[1004,133],[488,133]],labelAt:[746,123]},
      {from:'fn-infer',to:'extract'},
      {from:'extract',to:'fn-inspect',label:'ON'},
      {from:'extract',to:'done',label:'OFF / 対象外'},
      {from:'fn-inspect',to:'archive-result'},
      {from:'archive-result',to:'input',label:'要パスワード'},
      {from:'archive-result',to:'done',label:'成功',fromSide:'right',toSide:'right',via:[[1004,331],[1004,529],[300,529],[300,595]],labelAt:[974,319]},
      {from:'input',to:'fn-inspect',label:'入力して再開',fromSide:'left',toSide:'bottom',via:[[488,463]],feedback:true},
    ],
  },
  install: {
    nodes:[
      state('received','適用する',0,0,'event'),func('fn-plan',1,0),func('fn-resolve',2,0),
      state('valid','計画を作成できた？',2,1,'condition'),state('conflict','競合・不足を表示して停止',1,1),
      state('confirm','計画を確認して適用',2,2,'event'),func('fn-install',1,2),func('fn-stage',0,2),
      func('fn-unity',0,3),state('completed','Unity が正常完了？',1,3,'condition'),func('fn-commit',2,3),
      func('fn-recover',1,4),state('done','実績を確定して完了通知',2,4),state('recovered','復旧結果を表示',1,5),
    ],
    edges:[
      {from:'received',to:'fn-plan'},{from:'fn-plan',to:'fn-resolve'},
      {from:'fn-resolve',to:'valid'},{from:'valid',to:'conflict',label:'いいえ'},
      {from:'valid',to:'confirm',label:'はい'},{from:'confirm',to:'fn-install'},
      {from:'fn-install',to:'fn-stage'},{from:'fn-stage',to:'fn-unity'},
      {from:'fn-unity',to:'completed'},{from:'completed',to:'fn-commit',label:'成功'},
      {from:'completed',to:'fn-recover',label:'失敗・中断'},
      {from:'fn-commit',to:'done'},{from:'fn-recover',to:'recovered'},
    ],
  },
  custom: {
    nodes:[
      state('received','カスタムパッケージを作成',0,0,'event'),func('fn-custom',1,0),func('fn-resolve',2,0),
      state('valid','依存を解決できた？',2,1,'condition'),state('invalid','循環・欠損・競合を表示',1,1),
      state('done','共通パッケージを保存',2,2),
    ],
    edges:[
      {from:'received',to:'fn-custom'},{from:'fn-custom',to:'fn-resolve'},
      {from:'fn-resolve',to:'valid'},{from:'valid',to:'invalid',label:'いいえ'},
      {from:'valid',to:'done',label:'はい'},
    ],
  },
  project: {
    nodes:[
      state('received','既存プロジェクトを選択',0,0,'event'),func('fn-link',1,0),func('fn-validate',2,0),
      state('valid','初回起動済みで有効？',2,1,'condition'),state('invalid','エラーを表示して停止',1,1),
      state('done','プロジェクトを登録',2,2),
    ],
    edges:[
      {from:'received',to:'fn-link'},{from:'fn-link',to:'fn-validate'},
      {from:'fn-validate',to:'valid'},{from:'valid',to:'invalid',label:'いいえ'},
      {from:'valid',to:'done',label:'はい'},
    ],
  },
  unlock: {
    nodes:[
      state('received','Unlock',0,0,'event'),func('fn-unlock',1,0),state('notice','初回説明の了承が必要？',2,0,'condition'),
      func('fn-privacy-notice',2,1,'表示だけを隠すことを説明'),state('valid','OS 認証に成功？',1,1,'condition'),state('failed','非表示を維持',0,1),
      func('fn-search',1,2),state('done','期限内だけ表示',0,2),
    ],
    edges:[
      {from:'received',to:'fn-unlock'},{from:'fn-unlock',to:'notice'},
      {from:'notice',to:'fn-privacy-notice',label:'必要'},
      {from:'notice',to:'valid',label:'了承済み',fromSide:'left',via:[[650,67],[650,121],[488,121]],labelAt:[567,113]},
      {from:'fn-privacy-notice',to:'fn-unlock',label:'続ける・再実行',fromSide:'right',toSide:'top',via:[[1004,199],[1004,0],[488,0]],labelAt:[746,14],feedback:true},
      {from:'fn-privacy-notice',to:'failed',label:'キャンセル',fromSide:'left',toSide:'top',via:[[650,199],[650,145],[152,145]],labelAt:[274,136]},
      {from:'valid',to:'failed',label:'失敗・取消'},
      {from:'valid',to:'fn-search',label:'成功'},
      {from:'fn-search',to:'done'},
    ],
  },
};

export function createFlowModel(flow:Catalog['flows'][number],specs:Pick<Spec,'id'|'title'|'subtitle'>[]) {
  const definition=diagrams[flow.id]??{
    nodes:flow.steps.map((id,index)=>func(id,0,index)),
    edges:flow.steps.slice(1).map((id,index)=>({from:flow.steps[index],to:id})),
  };
  const nodes=definition.nodes.map(node=>{
    const spec=specs.find(s=>s.id===node.specId);
    if(node.specId&&!spec)throw new Error('Unknown flow function: '+node.specId);
    return {...node,label:node.label??spec!.title,caption:node.caption??spec?.subtitle,
      x:PADDING+node.column*STEP_X,y:PADDING+node.row*STEP_Y,width:WIDTH,height:HEIGHT};
  });
  const nodeMap=new Map(nodes.map(node=>[node.id,node]));
  const anchor=(node:typeof nodes[number],side:Side)=>{
    if(side==='top')return [node.x+WIDTH/2,node.y];
    if(side==='bottom')return [node.x+WIDTH/2,node.y+HEIGHT];
    if(side==='left')return [node.x,node.y+HEIGHT/2];
    return [node.x+WIDTH,node.y+HEIGHT/2];
  };
  const edges=definition.edges.map((edge,index)=>{
    const from=nodeMap.get(edge.from),to=nodeMap.get(edge.to);
    if(!from||!to)throw new Error('Unknown flow edge: '+edge.from+' → '+edge.to);
    const sameRow=from.row===to.row;
    const fromSide=edge.fromSide??(sameRow?(from.column<to.column?'right':'left'):from.row<to.row?'bottom':'top');
    const toSide=edge.toSide??(sameRow?(from.column<to.column?'left':'right'):from.row<to.row?'top':'bottom');
    const start=anchor(from,fromSide),end=anchor(to,toSide);
    const midY=(start[1]+end[1])/2;
    const between=edge.via??(start[0]===end[0]||start[1]===end[1]?[]:[[start[0],midY],[end[0],midY]]);
    const points=[start,...between,end];
    const segments=points.slice(1).map((p,i)=>({a:points[i],b:p,length:Math.hypot(p[0]-points[i][0],p[1]-points[i][1])}));
    const longest=segments.sort((a,b)=>b.length-a.length)[0];
    const labelAt=edge.labelAt??[(longest.a[0]+longest.b[0])/2,(longest.a[1]+longest.b[1])/2-9];
    return {...edge,id:flow.id+'-'+index,points,labelAt,path:points.map(([x,y],i)=>(i?'L':'M')+x+','+y).join(' ')};
  });
  return {
    nodes,edges,
    width:Math.max(...nodes.map(n=>n.x+n.width),...edges.flatMap(e=>e.points.map(p=>p[0])))+PADDING,
    height:Math.max(...nodes.map(n=>n.y+n.height),...edges.flatMap(e=>e.points.map(p=>p[1])))+PADDING,
  };
}
